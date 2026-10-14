use cow_utils::CowUtils;
use oxc_angular_compiler::{
    ast::{
        expression::{AngularExpression, LiteralValue},
        r3::{R3BoundAttribute, R3Element, R3Template, R3Visitor, visit_all},
    },
    parser::html::decode_entities_in_string,
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{AngularTemplate, angular_binding_name},
};

fn prefer_static_string_properties_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Using a property is more efficient than binding a static string.")
        .with_label(span)
}

// Properties that behave differently as attributes: `textContent` is not HTML-decoded as one.
// Angular maps a bound `innerHtml` to `innerHTML`, but not a static attribute.
const PROPERTY_ONLY_DOM_APIS: [&str; 5] =
    ["innerHTML", "innerHtml", "innerText", "outerHTML", "textContent"];

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PreferStaticStringProperties(Box<PreferStaticStringPropertiesConfig>);

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct PreferStaticStringPropertiesConfig {
    /// Property names this rule ignores.
    ignore: Vec<String>,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that a static string is assigned with an attribute rather than bound with a
    /// property binding. Animation bindings, `attr.`, `class.` and `style.` bindings,
    /// structural directives and the property-only DOM APIs `innerHTML` (or `innerHtml`),
    /// `innerText`, `outerHTML` and `textContent` are not checked.
    ///
    /// ### Why is this bad?
    ///
    /// A bound string literal is evaluated on every change detection although it never
    /// changes; an attribute states that the value is static.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <img [alt]="'Profile image'">
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <img alt="Profile image">
    /// <img [alt]="description">
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - `[innerHtml]` is not reported, since a static `innerHtml` attribute does not set
    ///   `innerHTML`.
    PreferStaticStringProperties,
    angular_template,
    style,
    fix,
    config = PreferStaticStringProperties,
    version = "next",
    short_description = "Ensures that static string values use property assignment instead of property binding.",
);

impl Rule for PreferStaticStringProperties {
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
    rule: &'c PreferStaticStringPropertiesConfig,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
}

impl Visitor<'_, '_> {
    fn check(&self, input: &R3BoundAttribute) {
        let name = input.name.as_str();
        if PROPERTY_ONLY_DOM_APIS.contains(&name) || self.rule.ignore.iter().any(|i| i == name) {
            return;
        }
        let key = angular_binding_name(self.ctx, self.template, input.key_span);
        if key.contains(['@', '.']) {
            return;
        }
        let AngularExpression::LiteralPrimitive(literal) = &input.value else { return };
        let LiteralValue::String(value) = &literal.value else { return };
        let Some(value_span) = input.value_span else { return };
        // A literal in double quotes sits in a single-quoted attribute, which the fix keeps.
        let source =
            decode_entities_in_string(self.ctx.source_range(self.template.span(value_span)));
        let (quote, escaped_quote) =
            if source.trim_start().starts_with('"') { ('\'', "&#39;") } else { ('"', "&quot;") };
        let value = value.cow_replace('&', "&amp;");
        let value = value.cow_replace(quote, escaped_quote);
        let span = self.template.span(input.source_span);
        self.ctx.diagnostic_with_fix(prefer_static_string_properties_diagnostic(span), |fixer| {
            fixer.replace(span, format!("{name}={quote}{value}{quote}"))
        });
    }
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        element.inputs.iter().for_each(|input| self.check(input));
        self.visit_element_children(element);
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        // A structural directive's template holds copies of the element's inputs.
        if template.template_attrs.is_empty() {
            template.inputs.iter().for_each(|input| self.check(input));
        }
        visit_all(self, &template.children);
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        (r#"<my-component [name]="foo"/>"#, None),
        (r#"<my-component [name]="42"/>"#, None),
        (r#"<my-component [name]="true"/>"#, None),
        (r#"<my-component [name]="null"/>"#, None),
        (r#"<my-component [name]="undefined"/>"#, None),
        (r#"<my-component name="foo"/>"#, None),
        (r#"<my-component *name="'foo'"/>"#, None),
        (r#"<input *ngSwitchCase="'foo'" />"#, None),
        (r#"<ng-container *ngIf="'foo'" />"#, None),
        (r#"<my-component [style.color]="'foo'" />"#, None),
        (r#"<my-component [attr.aria-label]="'foo'" />"#, None),
        (r#"<my-component [class.foo]="'foo'" />"#, None),
        (r#"<my-component [@fade]="'foo'" />"#, None),
        (r#"<code [textContent]="'<I>'"></code>"#, None),
        (r#"<div [innerHTML]="'<strong>text</strong>'"></div>"#, None),
        (r#"<span [innerText]="'text'"></span>"#, None),
        (r#"<div [outerHTML]="'<p>text</p>'"></div>"#, None),
        (r#"<div [innerHtml]="'<b>text</b>'"></div>"#, None),
        (r#"<i [name]="('foo')"></i>"#, None),
        (r#"<i [name]="`foo`"></i>"#, None),
        (r#"<my-component [customProp]="'value'"/>"#, Some(json!([{ "ignore": ["customProp"] }]))),
        (r#"<my-component name="{{ 'foo' }}"/>"#, None),
        (r#"<ng-template [ngIf]="show"><span></span></ng-template>"#, None),
    ];

    let fail = vec![
        (r#"<my-component [name]="'foo'"/>"#, None),
        (r#"<my-component [name]="   'foo'   "/>"#, None),
        (r#"<my-component [name]=' "foo" '/>"#, None),
        (r#"<my-component [name]="'say &quot;hi&quot;'"/>"#, None),
        (r#"<my-component [name]='"it&apos;s"'/>"#, None),
        (r#"<my-component [name]="'a &amp;quot; b'"/>"#, None),
        (
            r#"<code [highlightAuto]="'<ng-template #ref=&quot;poly&quot;>x</ng-template>'"></code>"#,
            None,
        ),
        (r#"<my-component bind-name="'foo'"/>"#, None),
        // Reported once, on the element rather than on the structural directive's template.
        (r#"<img *ngIf="show" [alt]="'Profile'">"#, None),
        (r#"<ng-template [ngIf]="show" [title]="'x'"><span></span></ng-template>"#, None),
        (r#"@if (show) { <img [alt]="'Profile'"> }"#, None),
    ];

    let fix = vec![
        (r#"<my-component [name]="'foo'"/>"#, r#"<my-component name="foo"/>"#),
        (r#"<my-component [name]="   'foo'   "/>"#, r#"<my-component name="foo"/>"#),
        (r#"<my-component [name]=' "foo" '/>"#, "<my-component name='foo'/>"),
        (
            r#"<my-component [name]="'say &quot;hi&quot;'"/>"#,
            r#"<my-component name="say &quot;hi&quot;"/>"#,
        ),
        (r#"<my-component [name]='"it&apos;s"'/>"#, "<my-component name='it&#39;s'/>"),
        (
            r#"<my-component [name]="'a &amp;quot; b'"/>"#,
            r#"<my-component name="a &amp;quot; b"/>"#,
        ),
        (
            r#"<code [highlightAuto]="'<ng-template #ref=&quot;poly&quot;>x</ng-template>'"></code>"#,
            r#"<code highlightAuto="<ng-template #ref=&quot;poly&quot;>x</ng-template>"></code>"#,
        ),
        (r#"<my-component bind-name="'foo'"/>"#, r#"<my-component name="foo"/>"#),
        (r#"<my-component [name]="&quot;foo&quot;"/>"#, "<my-component name='foo'/>"),
        (r#"<img *ngIf="show" [alt]="'Profile'">"#, r#"<img *ngIf="show" alt="Profile">"#),
        (
            r#"<ng-template [ngIf]="show" [title]="'x'"></ng-template>"#,
            r#"<ng-template [ngIf]="show" title="x"></ng-template>"#,
        ),
    ];

    Tester::new(
        PreferStaticStringProperties::NAME,
        PreferStaticStringProperties::PLUGIN,
        pass,
        fail,
    )
    .change_rule_path_extension("html")
    .expect_fix(fix)
    .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: `<img [alt]=\"description\">` }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: `<img [alt]=\"'Profile'\">` }) class A {}",
    ];
    let inline_fix = vec![(
        "import { Component } from '@angular/core'; @Component({ template: `<img [alt]=\"'Profile'\">` }) class A {}",
        "import { Component } from '@angular/core'; @Component({ template: `<img alt=\"Profile\">` }) class A {}",
    )];
    Tester::new(
        PreferStaticStringProperties::NAME,
        PreferStaticStringProperties::PLUGIN,
        inline_pass,
        inline_fail,
    )
    .change_rule_path_extension("ts")
    .expect_fix(inline_fix)
    .with_snapshot_suffix("inline")
    .test_and_snapshot();
}
