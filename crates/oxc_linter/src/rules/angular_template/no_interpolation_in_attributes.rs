use oxc_angular_compiler::ast::{
    expression::AngularExpression,
    r3::{R3BoundAttribute, R3Template, R3Visitor, visit_all},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{AngularTemplate, visit_template_in_eslint_order, walk_expression},
};

fn no_interpolation_in_attributes_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Use property binding [attribute]=\"value\" instead of interpolation {{ value }} for an attribute.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoInterpolationInAttributes {
    /// Allow interpolation that is only part of the value, as in `alt="{{ name }} is online"`.
    allow_substring_interpolation: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that property binding is used instead of interpolation in attributes.
    ///
    /// ### Why is this bad?
    ///
    /// `name="{{ value }}"` always produces a string, while `[name]="value"` keeps the bound
    /// value's type and makes the binding explicit.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <input name="{{ foo }}">
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <input [name]="foo">
    /// ```
    NoInterpolationInAttributes,
    angular_template,
    style,
    conditional_fix,
    config = NoInterpolationInAttributes,
    version = "next",
    short_description = "Ensures that property-binding is used instead of interpolation in attributes.",
);

impl Rule for NoInterpolationInAttributes {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { rule: self, ctx, template }, template.nodes);
        }
    }
}

struct Visitor<'c, 'a> {
    rule: &'c NoInterpolationInAttributes,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_bound_attribute(&mut self, attr: &R3BoundAttribute<'a>) {
        walk_expression(&attr.value, None, &mut |expression, _| {
            let AngularExpression::Interpolation(interpolation) = expression else { return };
            let is_full = interpolation.strings.iter().all(|s| s.is_empty());
            if self.rule.allow_substring_interpolation && !is_full {
                return;
            }
            let ctx = self.ctx;
            let diagnostic = no_interpolation_in_attributes_diagnostic(
                self.template.expression_span(interpolation.source_span),
            );
            let Some(value_span) = attr.value_span.filter(|_| is_full) else {
                ctx.diagnostic(diagnostic);
                return;
            };
            let name = ctx.source_range(self.template.span(attr.key_span)).trim();
            let value = ctx.source_range(self.template.span(value_span));
            let Some(inner) = value.strip_prefix("{{").and_then(|v| v.strip_suffix("}}")) else {
                ctx.diagnostic(diagnostic);
                return;
            };
            let replacement = format!("[{name}]=\"{}\"", inner.trim());
            let span = self.template.span(attr.source_span);
            ctx.diagnostic_with_fix(diagnostic, |fixer| fixer.replace(span, replacement));
        });
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        visit_template_in_eslint_order(self, template);
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        (r#"<input type="text" [name]="foo">"#, None),
        (r#"<input type="text" name="foo" [(ngModel)]="foo">"#, None),
        (r#"<input type="text" [name]="foo + 'bar'">"#, None),
        (r#"<input type="text" [name]="foo | bar">"#, None),
        ("<div>{{ content }}</div>", None),
        (
            r#"<img alt="{{username}} is online">"#,
            Some(json!([{ "allowSubstringInterpolation": true }])),
        ),
    ];

    let fail = vec![
        (r#"<input type="text" name="{{ foo }}">"#, None),
        (r#"<input type="text" name="{{ foo }}bar">"#, None),
        (r#"<input type="text" attr.data-myExample="{{ foo }}">"#, None),
        (r#"<img alt="{{username}} is online" src="{{src}}">"#, None),
        (
            r#"<img alt="{{username}} is online" src="{{src}}">"#,
            Some(json!([{ "allowSubstringInterpolation": true }])),
        ),
        (r#"<img *ngIf="x" src="{{ url }}">"#, None),
    ];

    let fix = vec![
        (r#"<input type="text" name="{{ foo }}">"#, r#"<input type="text" [name]="foo">"#),
        (r#"<input attr.data-myExample="{{ foo }}">"#, r#"<input [attr.data-myExample]="foo">"#),
        (r#"<input name="{{ foo }}bar">"#, r#"<input name="{{ foo }}bar">"#),
    ];

    Tester::new(NoInterpolationInAttributes::NAME, NoInterpolationInAttributes::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
