use oxc_angular_compiler::ast::{
    expression::{AngularExpression, LiteralValue},
    r3::{R3Element, R3Visitor, visit_all},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    globals::AriaProperty,
    rule::Rule,
    rules::jsx_a11y::aria_proptypes::{AriaPropType, get_aria_prop_type},
    utils::{AngularTemplate, is_angular_dom_element},
};

fn invalid_attribute_diagnostic(attribute: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("The `{attribute}` is an invalid ARIA attribute")).with_label(span)
}

fn invalid_value_diagnostic(attribute: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("The `{attribute}` has an invalid value."))
        .with_help("Check the valid values at https://raw.githack.com/w3c/aria/stable/#roles")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ValidAria;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that ARIA attributes exist and that their static values are valid.
    ///
    /// ### Why is this bad?
    ///
    /// Assistive technology ignores misspelled ARIA attributes and invalid values, so the
    /// element is announced without the intended state.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div aria-labeledby="title"></div>
    /// <input aria-required="yes">
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div aria-labelledby="title"></div>
    /// <input aria-required="true">
    /// ```
    ValidAria,
    angular_template,
    correctness,
    suggestion,
    version = "next",
    short_description = "Ensures that correct ARIA attributes and respective values are used",
);

impl Rule for ValidAria {
    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { ctx, template }, template.nodes);
        }
    }
}

struct Visitor<'c, 'a> {
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
}

/// An attribute value as `@angular-eslint` checks it.
enum Value<'a> {
    Str(&'a str),
    Number(f64),
    Bool(bool),
    Nil,
}

impl Visitor<'_, '_> {
    fn check(&self, name: &str, value: Option<Value>, span: Span) {
        let span = self.template.span(span);
        let Ok(property) = AriaProperty::try_from(name) else {
            self.ctx.diagnostic_with_suggestion(
                invalid_attribute_diagnostic(name, span),
                |fixer| {
                    fixer
                        .delete_range(Span::new(span.start - 1, span.end))
                        .with_message(format!("Remove attribute `{name}`"))
                },
            );
            return;
        };
        // Bindings other than literals can't be checked.
        let Some(value) = value else { return };
        if !is_valid(&get_aria_prop_type(property), &value) {
            self.ctx.diagnostic(invalid_value_diagnostic(name, span));
        }
    }
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        if is_angular_dom_element(&element.name, true) {
            let is_aria = |name: &str| name.len() > 5 && name.starts_with("aria-");
            for attr in element.attributes.iter().filter(|attr| is_aria(&attr.name)) {
                self.check(&attr.name, Some(Value::Str(attr.value.as_str())), attr.source_span);
            }
            for input in element.inputs.iter().filter(|input| is_aria(&input.name)) {
                let value = match &input.value {
                    AngularExpression::LiteralPrimitive(literal) => Some(match &literal.value {
                        LiteralValue::String(value) => Value::Str(value.as_str()),
                        LiteralValue::Number(value) => Value::Number(*value),
                        LiteralValue::Boolean(value) => Value::Bool(*value),
                        LiteralValue::Null | LiteralValue::Undefined => Value::Nil,
                    }),
                    // A literal array or object is checked as `undefined`.
                    AngularExpression::LiteralArray(_) | AngularExpression::LiteralMap(_) => {
                        Some(Value::Nil)
                    }
                    _ => None,
                };
                self.check(&input.name, value, input.source_span);
            }
        }
        self.visit_element_children(element);
    }
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => Some(*n),
        Value::Str(s) => {
            let s = s.trim();
            // `parseFloat` needs a leading number, which `Number("")` would not.
            let starts_numeric = s
                .trim_start_matches(['+', '-'])
                .starts_with(|c: char| c.is_ascii_digit() || c == '.');
            if starts_numeric { s.parse().ok() } else { None }
        }
        _ => None,
    }
}

fn is_boolean_like(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Str("true") => Some(true),
        Value::Str("false") => Some(false),
        _ => None,
    }
}

fn is_valid(property_type: &AriaPropType, value: &Value) -> bool {
    let is_nil = matches!(value, Value::Nil);
    match property_type {
        AriaPropType::Boolean => is_boolean_like(value).is_some(),
        AriaPropType::OptionalBoolean => is_nil || is_boolean_like(value).is_some(),
        AriaPropType::Tristate => {
            is_nil || matches!(value, Value::Str("mixed")) || is_boolean_like(value).is_some()
        }
        AriaPropType::Id | AriaPropType::IdList => true,
        AriaPropType::Integer => {
            as_number(value).is_some_and(|n| n.is_finite() && n.fract() == 0.0)
        }
        AriaPropType::Number => as_number(value).is_some_and(f64::is_finite),
        AriaPropType::String => matches!(value, Value::Str(_)),
        AriaPropType::Token(values) => {
            let token = match (is_boolean_like(value), value) {
                (Some(b), _) => {
                    if b {
                        "true"
                    } else {
                        "false"
                    }
                }
                (None, Value::Str(s)) => s,
                _ => return false,
            };
            values.iter().any(|v| v == token)
        }
        AriaPropType::TokenList(values) => match value {
            // An empty list is one empty token, as JavaScript's `"".split(/\s+/)` gives.
            Value::Str(s) => {
                let s = s.trim();
                !s.is_empty() && s.split_whitespace().all(|token| values.iter().any(|v| v == token))
            }
            _ => false,
        },
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<input aria-labelledby="Text">"#,
        r#"<input [attr.aria-label]="ariaLabel">"#,
        r#"<button [attr.aria-pressed]="isPressed"></button>"#,
        r#"<input aria-required="true">"#,
        r#"<input [attr.aria-required]="true">"#,
        r#"<div aria-checked="mixed"></div>"#,
        r#"<div aria-level="2"></div>"#,
        r#"<div [attr.aria-valuenow]="2.5"></div>"#,
        r#"<div aria-haspopup="true"></div>"#,
        r#"<div aria-live="polite"></div>"#,
        r#"<div aria-relevant="additions text"></div>"#,
        r#"<app-x aria-bogus="x"></app-x>"#,
    ];

    let fail = vec![
        r#"<input aria-labeledby="Text">"#,
        r#"<div [attr.aria-bogus]="x"></div>"#,
        r#"<input aria-required="yes">"#,
        r#"<div aria-level="2.5"></div>"#,
        r#"<div [attr.aria-valuenow]="'x'"></div>"#,
        r#"<div aria-live="loud"></div>"#,
        r#"<div aria-relevant="additions bogus"></div>"#,
        r#"<div aria-relevant="   "></div>"#,
        r#"<div [attr.aria-busy]="[]"></div>"#,
        r#"<DIV aria-hidden="nope"></DIV>"#,
        // A valueless attribute is an empty string, which is not a boolean.
        "<div aria-expanded></div>",
    ];

    let fix = vec![(r#"<input aria-labeledby="Text">"#, "<input>")];

    Tester::new(ValidAria::NAME, ValidAria::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
