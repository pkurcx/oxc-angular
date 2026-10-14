use lazy_regex::{Regex, RegexBuilder};
use oxc_angular_compiler::ast::{
    expression::{AngularExpression, LiteralValue},
    r3::{R3Element, R3Visitor, visit_all},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, de::Error};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::AngularTemplate,
};

fn missing_type_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Type for <button> is missing").with_label(span)
}

fn invalid_type_diagnostic(value: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("\"{value}\" can not be used as a type for <button>"))
        .with_label(span)
}

const VALID_BUTTON_TYPES: [&str; 3] = ["button", "submit", "reset"];

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct ButtonHasType(Box<ButtonHasTypeConfig>);

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct ButtonHasTypeConfig {
    /// Directive names that, when present on the element, cause it to be ignored. Entries
    /// wrapped in slashes, e.g. `/^tui/`, are treated as regular expressions.
    ignore_with_directives: IgnoredDirectives,
}

/// The `ignoreWithDirectives` option: exact names, or `/pattern/flags` regular expressions.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub(super) struct IgnoredDirectives(
    #[serde(deserialize_with = "deserialize_directive_matchers")]
    #[schemars(with = "Vec<String>")]
    Vec<DirectiveMatcher>,
);

#[derive(Debug, Clone)]
enum DirectiveMatcher {
    Name(String),
    Pattern(Regex),
}

impl IgnoredDirectives {
    /// Whether an attribute or input of the element names an ignored directive.
    pub(super) fn matches(&self, element: &R3Element) -> bool {
        let names = element.inputs.iter().map(|input| input.name.as_str());
        let mut names = names.chain(element.attributes.iter().map(|attr| attr.name.as_str()));
        !self.0.is_empty()
            && names.any(|name| {
                self.0.iter().any(|matcher| match matcher {
                    DirectiveMatcher::Name(entry) => entry == name,
                    DirectiveMatcher::Pattern(pattern) => pattern.is_match(name),
                })
            })
    }
}

fn deserialize_directive_matchers<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<DirectiveMatcher>, D::Error> {
    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .map(|entry| {
            // As `/^\/(.+)\/([a-z]*)$/` in `@angular-eslint`.
            let regex = entry.strip_prefix('/').and_then(|rest| {
                let (pattern, flags) = rest.rsplit_once('/')?;
                (!pattern.is_empty() && flags.bytes().all(|b| b.is_ascii_lowercase()))
                    .then_some((pattern, flags))
            });
            let Some((pattern, flags)) = regex else {
                return Ok(DirectiveMatcher::Name(entry));
            };
            let mut builder = RegexBuilder::new(pattern);
            for flag in flags.chars() {
                match flag {
                    'i' => builder.case_insensitive(true),
                    'm' => builder.multi_line(true),
                    's' => builder.dot_matches_new_line(true),
                    'd' | 'g' | 'u' | 'v' | 'y' => &mut builder,
                    _ => return Err(D::Error::custom(format!("invalid regex flags in `{entry}`"))),
                };
            }
            builder.build().map(DirectiveMatcher::Pattern).map_err(D::Error::custom)
        })
        .collect()
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that a `<button>` has a valid `type`: `button`, `submit` or `reset`.
    ///
    /// ### Why is this bad?
    ///
    /// A button without a type submits its form when clicked. Stating the type makes the
    /// intent clear and prevents unintended form submissions.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <button></button>
    /// <button type="whatever"></button>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <button type="button"></button>
    /// <button [attr.type]="'submit'"></button>
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - `ignoreWithDirectives` patterns use Rust regex syntax; the `g` and `y` flags are accepted
    ///   but have no effect.
    ButtonHasType,
    angular_template,
    restriction,
    config = ButtonHasType,
    version = "next",
    short_description = "Ensures that a button has a valid type specified",
);

impl Rule for ButtonHasType {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { rule: &self.0, ctx, template }, template.nodes);
        }
    }
}

struct Visitor<'c, 'a> {
    rule: &'c ButtonHasTypeConfig,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        if element.name.eq_ignore_ascii_case("button") {
            self.check(element);
        }
        self.visit_element_children(element);
    }
}

impl Visitor<'_, '_> {
    fn check(&self, element: &R3Element) {
        let has_type = element.attributes.iter().any(|attr| attr.name == "type")
            || element.inputs.iter().any(|input| input.name == "type");
        if !has_type && !self.rule.ignore_with_directives.matches(element) {
            let span = self.template.span(element.source_span);
            self.ctx.diagnostic(missing_type_diagnostic(span));
        }

        let invalid_attribute = element
            .attributes
            .iter()
            .find(|attr| attr.name == "type" && !VALID_BUTTON_TYPES.contains(&attr.value.as_str()))
            .map(|attr| (attr.value.as_str(), attr.source_span));
        // Only literal strings, since the type of a bound value is unknown.
        let invalid_input = || {
            element.inputs.iter().find_map(|input| match &input.value {
                AngularExpression::LiteralPrimitive(literal) if input.name == "type" => {
                    match &literal.value {
                        LiteralValue::String(value)
                            if !VALID_BUTTON_TYPES.contains(&value.as_str()) =>
                        {
                            Some((value.as_str(), input.source_span))
                        }
                        _ => None,
                    }
                }
                _ => None,
            })
        };
        if let Some((value, span)) = invalid_attribute.or_else(invalid_input) {
            self.ctx.diagnostic(invalid_type_diagnostic(value, self.template.span(span)));
        }
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        (r#"<button [attr.type]="'button'"></button>"#, None),
        (r#"<button [attr.type]="'submit'"></button>"#, None),
        (r#"<button [attr.type]="'reset'"></button>"#, None),
        (r#"<button type="button"></button>"#, None),
        (r#"<BUTTON type="button"></BUTTON>"#, None),
        (r#"<button type="submit"></button>"#, None),
        (r#"<button type="reset"></button>"#, None),
        (r#"<button class="primary" type="submit"></button>"#, None),
        (r#"<button (click)="onClick()" type="button"></button>"#, None),
        (r#"<button [class.primary]="true" [attr.type]="'submit'"></button>"#, None),
        (r#"<button [disabled]="true" [attr.type]="'button'"></button>"#, None),
        ("<button myButton></button>", Some(json!([{ "ignoreWithDirectives": ["myButton"] }]))),
        ("<button [myButton]></button>", Some(json!([{ "ignoreWithDirectives": ["myButton"] }]))),
        (
            r#"<button tuiButton></button><button [tuiOption]="option"></button>"#,
            Some(json!([{ "ignoreWithDirectives": ["/^tui/"] }])),
        ),
        ("<button TuiButton></button>", Some(json!([{ "ignoreWithDirectives": ["/^tui/i"] }]))),
        // A bound value of unknown type.
        (r#"<button [type]="buttonType"></button>"#, None),
        (r#"<button *ngIf="show" type="button"></button>"#, None),
        (r#"<ng-template><button type="submit"></button></ng-template>"#, None),
        ("<app-button></app-button>", None),
    ];

    let fail = vec![
        ("<button></button>", None),
        ("<BUTTON></BUTTON>", None),
        (r#"<button (click)="onClick()"></button>"#, None),
        (r#"<button type="whatever"></button>"#, None),
        (r#"<button [attr.type]="'whatever'"></button>"#, None),
        (
            "<button myDirective></button>",
            Some(json!([{ "ignoreWithDirectives": ["myButton", "uiButton"] }])),
        ),
        ("<button myButton></button>", Some(json!([{ "ignoreWithDirectives": ["/^tui/"] }]))),
        ("<button tuiButton></button>", Some(json!([{ "ignoreWithDirectives": ["tui"] }]))),
        (
            r#"<button myButton type="whatever"></button>"#,
            Some(json!([{ "ignoreWithDirectives": ["myButton"] }])),
        ),
        (r#"<button *ngIf="show"></button>"#, None),
        (r#"<button *ngIf="show" type="whatever"></button>"#, None),
        ("<ng-template><button></button></ng-template>", None),
        ("@if (show) { <button></button> }", None),
    ];

    Tester::new(ButtonHasType::NAME, ButtonHasType::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<button type=\"button\"></button>' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<button></button>' }) class A {}",
    ];
    Tester::new(ButtonHasType::NAME, ButtonHasType::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
