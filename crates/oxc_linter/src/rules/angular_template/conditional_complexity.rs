use oxc_angular_compiler::ast::{
    expression::AngularExpression,
    r3::{
        R3BoundAttribute, R3Element, R3IfBlockBranch, R3LetDeclaration, R3SwitchBlock, R3Template,
        R3Visitor, visit_all,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{
        AngularTemplate, for_each_expression, visit_element_in_eslint_order,
        visit_template_in_eslint_order,
    },
};

fn conditional_complexity_diagnostic(total: u32, max: u32, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The conditional complexity {total} exceeds the defined limit {max}"
    ))
    .with_label(span)
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ConditionalComplexity {
    /// The maximum number of binary and conditional operators in one expression.
    max_complexity: u32,
}

impl Default for ConditionalComplexity {
    fn default() -> Self {
        Self { max_complexity: 5 }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Limits the number of binary and conditional (`? :`) operators in a template expression.
    /// Bindings, interpolations, `@if`, `@switch` and `@let` expressions are checked.
    ///
    /// ### Why is this bad?
    ///
    /// Long conditions are hard to read and test in a template; move them into the component.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule, with the default `{ "maxComplexity": 5 }`:
    /// ```html
    /// <div *ngIf="a === 1 || b === 2 || c === 3 || d === 4"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div *ngIf="isVisible()"></div>
    /// ```
    ConditionalComplexity,
    angular_template,
    pedantic,
    config = ConditionalComplexity,
    version = "next",
    short_description = "The conditional complexity should not exceed a rational limit",
);

impl Rule for ConditionalComplexity {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { max: self.max_complexity, ctx, template }, template.nodes);
            // Interpolations are checked one expression at a time, wherever they are.
            for_each_expression(template.nodes, &mut |expression, _| {
                if let AngularExpression::Interpolation(interpolation) = expression {
                    for expression in &interpolation.expressions {
                        check(self.max_complexity, ctx, template, expression);
                    }
                }
            });
        }
    }
}

fn check(max: u32, ctx: &LintContext, template: &AngularTemplate, expression: &AngularExpression) {
    let total = total_complexity(expression);
    report(max, total, ctx, template, expression);
}

/// Bindings and control flow expressions are unwrapped once more than interpolations, as
/// `@angular-eslint` does, so `(a || b) | async` counts its operators.
fn check_unwrapped(
    max: u32,
    ctx: &LintContext,
    template: &AngularTemplate,
    expression: &AngularExpression,
) {
    let total = total_complexity(unwrap(expression));
    report(max, total, ctx, template, expression);
}

fn report(
    max: u32,
    total: u32,
    ctx: &LintContext,
    template: &AngularTemplate,
    expression: &AngularExpression,
) {
    if total > max {
        let span = template.expression_span(expression.source_span());
        ctx.diagnostic(conditional_complexity_diagnostic(total, max, span));
    }
}

struct Visitor<'c, 'a> {
    max: u32,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        visit_element_in_eslint_order(self, element);
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        visit_template_in_eslint_order(self, template);
    }

    fn visit_bound_attribute(&mut self, attr: &R3BoundAttribute<'a>) {
        if attr.value_span.is_some() && !matches!(attr.value, AngularExpression::Interpolation(_)) {
            check_unwrapped(self.max, self.ctx, self.template, &attr.value);
        }
    }

    fn visit_if_block_branch(&mut self, branch: &R3IfBlockBranch<'a>) {
        visit_all(self, &branch.children);
        if let Some(expression) = &branch.expression {
            check_unwrapped(self.max, self.ctx, self.template, expression);
        }
    }

    fn visit_switch_block(&mut self, block: &R3SwitchBlock<'a>) {
        for group in &block.groups {
            self.visit_switch_block_case_group(group);
        }
        check_unwrapped(self.max, self.ctx, self.template, &block.expression);
    }

    fn visit_let_declaration(&mut self, declaration: &R3LetDeclaration<'a>) {
        check_unwrapped(self.max, self.ctx, self.template, &declaration.value);
    }
}

/// Unwraps parentheses, then one pipe, as `@angular-eslint` does.
fn unwrap<'e, 'a>(expression: &'e AngularExpression<'a>) -> &'e AngularExpression<'a> {
    let expression = match expression {
        AngularExpression::ParenthesizedExpression(e) => &e.expression,
        _ => expression,
    };
    match expression {
        AngularExpression::BindingPipe(pipe) => &pipe.exp,
        _ => expression,
    }
}

fn unwrap_parentheses<'e, 'a>(expression: &'e AngularExpression<'a>) -> &'e AngularExpression<'a> {
    match expression {
        AngularExpression::ParenthesizedExpression(e) => &e.expression,
        _ => expression,
    }
}

fn is_binary_or_conditional(expression: &AngularExpression) -> bool {
    matches!(
        expression,
        AngularExpression::Binary(_)
            | AngularExpression::Unary(_)
            | AngularExpression::Conditional(_)
    )
}

/// Each binary or conditional operator counts one. Angular's unary `-x` and `+x` are binary
/// expressions with `x` on the right.
fn total_complexity(expression: &AngularExpression) -> u32 {
    let operands = |left: Option<&AngularExpression>, right: &AngularExpression| {
        [left, Some(right)]
            .into_iter()
            .flatten()
            .filter(|operand| is_binary_or_conditional(unwrap_parentheses(operand)))
            .map(total_complexity)
            .sum::<u32>()
    };
    match unwrap(expression) {
        AngularExpression::Binary(binary) => 1 + operands(Some(&binary.left), &binary.right),
        AngularExpression::Unary(unary) => 1 + operands(None, &unary.expr),
        AngularExpression::Conditional(conditional) => {
            1 + total_complexity(&conditional.condition)
                + total_complexity(&conditional.true_exp)
                + total_complexity(&conditional.false_exp)
        }
        _ => 0,
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        (
            r#"<div *ngIf="a === '1' || (b === '2' && c.d !== e)">Enter your card details</div>"#,
            None,
        ),
        (r#"<div *ngIf="isValid; then thenTemplateRef; else elseTemplateRef">Content</div>"#, None),
        (r#"<div class="col" id="one-two-three-four-five-six-seven-eight-{{9}}"></div>"#, None),
        (r#"<div [@fadeIn]="isVisible">Content</div>"#, None),
        (
            r#"<div *ngIf="a === '1' || (b === '2' && c.d !== e)"></div>"#,
            Some(json!([{ "maxComplexity": 5 }])),
        ),
        ("{{ a ? b : c }}", None),
        (
            r#"<div [class.x]="a === 1 || b === 2 || c === 3 || d === 4 || e === 5"></div>"#,
            Some(json!([{ "maxComplexity": 9 }])),
        ),
        // Events are not checked.
        (r#"<button (click)="a && b && c && d && e && f && g"></button>"#, None),
    ];

    let fail = vec![
        (
            r#"<div *ngIf="a === '3' || (b === '3' && c.d !== '1' && e.f !== '6' && q !== g)"></div>"#,
            None,
        ),
        (
            r#"<div *ngIf="(a === '3' || (b === '3' && c.d !== '1' && e.f !== '6' && q !== g)) | async"></div>"#,
            Some(json!([{ "maxComplexity": 4 }])),
        ),
        ("{{ a === 1 || b === 2 || c === 3 || d === 4 }}", Some(json!([{ "maxComplexity": 6 }]))),
        (
            "@if (a === 1 || b === 2 || c === 3 || d === 4) { x }",
            Some(json!([{ "maxComplexity": 6 }])),
        ),
        (
            "@switch (a ? b ? c : d : e ? f : g) { @default { x } }",
            Some(json!([{ "maxComplexity": 2 }])),
        ),
        ("@let v = a && b && c;", Some(json!([{ "maxComplexity": 1 }]))),
        (r#"<img alt="{{ a && b && c }} text">"#, Some(json!([{ "maxComplexity": 1 }]))),
        // Upstream cases; the expected total is in each comment.
        // 4
        (
            r#"<ng-container *ngIf="control && control.invalid && (control.touched || (showOnDirty && control.dirty))"></ng-container>"#,
            Some(json!([{ "maxComplexity": 3 }])),
        ),
        // 5
        (
            r#"<ng-container *ngIf="control && control.invalid && (control.touched || (showOnDirty && control.dirty)) && control.errors | keys as errorKeys">{{ errorKeys }}</ng-container>"#,
            Some(json!([{ "maxComplexity": 2 }])),
        ),
        // 4
        (
            r#"<ng-container *ngIf="(control | isDefined) && control.invalid && (control.touched || (showOnDirty && control.dirty))"></ng-container>"#,
            Some(json!([{ "maxComplexity": 3 }])),
        ),
        // 7
        (r#"<div [class.px-4]="a <= b || (b > c && c >= d && d < e)"></div>"#, None),
        // 9
        (
            r#"<div [class.my-2]="a === 'x' ? v === c : b === 3 ? 0 : c > 3 && d === 9 ? 9 : 'xa'"></div>"#,
            None,
        ),
        // 8
        (
            "{{ test.xyz }} {{ ab > 2 && cd === 9 && control?.invalid && (control.touched || (showOnDirty && control.dirty)) ? 'some value' : 'another value' }} {{ control.touched }}",
            None,
        ),
        // 3
        (
            "@if ((a && b && c && d) | async; as result) { {{ result }} }",
            Some(json!([{ "maxComplexity": 2 }])),
        ),
        // 3
        (
            "@if (a) { First } @else if (b && c && d && e) { Second } @else { Third }",
            Some(json!([{ "maxComplexity": 2 }])),
        ),
    ];

    Tester::new(ConditionalComplexity::NAME, ConditionalComplexity::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();
}
