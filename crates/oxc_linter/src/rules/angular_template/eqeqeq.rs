use oxc_angular_compiler::ast::expression::{AngularExpression, BinaryOperator, LiteralValue};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::for_each_expression,
};

fn eqeqeq_diagnostic(actual: &str, expected: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Expected `{expected}` but received `{actual}`"))
        .with_help(format!("Replace `{actual}` with `{expected}`"))
        .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[schemars(rename = "AngularTemplateEqeqeqConfig")]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Eqeqeq {
    /// Allow `==` and `!=` when one side is `null` or `undefined`.
    allow_null_or_undefined: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires `===` and `!==` in place of `==` and `!=` in templates.
    ///
    /// ### Why is this bad?
    ///
    /// `==` and `!=` coerce their operands, so `'0' == 0` and `null == undefined` are both true.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// @if (status == 'done') { <p>Done</p> }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// @if (status === 'done') { <p>Done</p> }
    /// ```
    Eqeqeq,
    angular_template,
    suspicious,
    conditional_fix_suggestion,
    config = Eqeqeq,
    version = "next",
    short_description = "Requires `===` and `!==` in place of `==` and `!=`",
);

impl Rule for Eqeqeq {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            for_each_expression(template.nodes, &mut |expression, _| {
                let AngularExpression::Binary(binary) = expression else { return };
                let (actual, expected) = match binary.operation {
                    BinaryOperator::Equal => ("==", "==="),
                    BinaryOperator::NotEqual => ("!=", "!=="),
                    _ => return,
                };
                if self.allow_null_or_undefined && (is_nil(&binary.left) || is_nil(&binary.right)) {
                    return;
                }
                let left = template.expression_span(binary.left.source_span());
                let right = template.expression_span(binary.right.source_span());
                let Some(operator) =
                    ctx.source_range(Span::new(left.end, right.start)).find(actual)
                else {
                    return;
                };
                #[expect(clippy::cast_possible_truncation)]
                let operator = Span::sized(left.end + operator as u32, 2);
                let diagnostic = eqeqeq_diagnostic(
                    actual,
                    expected,
                    template.expression_span(binary.source_span),
                );
                let fix =
                    |fixer: crate::fixer::RuleFixer<'_, '_>| fixer.replace(operator, expected);
                // Only a non-numeric string can't compare equal across types, so only then is the fix safe.
                if is_non_numeric_string(&binary.left) || is_non_numeric_string(&binary.right) {
                    ctx.diagnostic_with_fix(diagnostic, fix);
                } else {
                    ctx.diagnostic_with_suggestion(diagnostic, fix);
                }
            });
        }
    }
}

fn is_nil(expression: &AngularExpression) -> bool {
    matches!(
        expression,
        AngularExpression::LiteralPrimitive(literal)
            if matches!(literal.value, LiteralValue::Null | LiteralValue::Undefined)
    )
}

fn is_non_numeric_string(expression: &AngularExpression) -> bool {
    matches!(
        expression,
        AngularExpression::LiteralPrimitive(literal)
            if matches!(&literal.value, LiteralValue::String(value)
                if !value.trim().parse::<f64>().is_ok_and(f64::is_finite))
    )
}

#[test]
fn test() {
    use serde_json::json;

    use crate::{fixer::FixKind, tester::Tester};

    let pass = vec![
        ("{{ a === 1 }}", None),
        (r#"<div [class.x]="a !== b"></div>"#, None),
        (r#"<button (click)="x = a === b"></button>"#, None),
        ("@if (a == null) { x }", Some(json!([{ "allowNullOrUndefined": true }]))),
        ("{{ undefined != b }}", Some(json!([{ "allowNullOrUndefined": true }]))),
    ];

    let fail = vec![
        ("{{ a == 1 }}", None),
        (r#"<div *ngIf="a != b"></div>"#, None),
        (r#"<div [class.x]="(a | async) == 'x'"></div>"#, None),
        ("@if (a == null) { x }", None),
        ("@for (x of xs; track x) { {{ x.y == 2 }} }", None),
        ("@switch (a == b) { @case (true) { x } }", None),
        ("@defer (when ready == true) { <x-cmp/> }", None),
        ("@let v = a != b;", None),
        // Reported once, though the structural template holds a copy of the input.
        (r#"<div *ngIf="x" [class.a]="a == b"></div>"#, None),
        (r#"<button (click)="ok = a == 'b'"></button>"#, None),
        ("{{ a == b }}", Some(json!([{ "allowNullOrUndefined": true }]))),
    ];

    let fix = vec![
        ("{{ a == 'b' }}", "{{ a === 'b' }}", None, FixKind::SafeFix),
        ("{{ 'b' != a }}", "{{ 'b' !== a }}", None, FixKind::SafeFix),
        // Comparing to a numeric string or a non-string is only a suggestion.
        ("{{ a == '1' }}", "{{ a == '1' }}", None, FixKind::SafeFix),
        ("{{ a == '1' }}", "{{ a === '1' }}", None, FixKind::Suggestion),
        ("{{ a == 1 }}", "{{ a === 1 }}", None, FixKind::Suggestion),
    ];

    Tester::new(Eqeqeq::NAME, Eqeqeq::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
