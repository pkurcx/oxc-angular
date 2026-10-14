use oxc_angular_compiler::ast::r3::{R3Element, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{AngularTemplate, angular_binding_name, angular_original_attribute_name},
};

fn no_duplicate_attributes_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Duplicate attribute `{name}`")).with_label(span)
}

// Angular merges these attributes with their bindings.
// https://angular.dev/guide/templates/class-binding#styling-precedence
const STYLE_PRECEDENCE_ATTRIBUTES: [&str; 2] = ["class", "style"];

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NoDuplicateAttributes(Box<NoDuplicateAttributesConfig>);

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct NoDuplicateAttributesConfig {
    /// Allow a two-way binding next to an output of the same event, as in
    /// `[(ngModel)]="value" (ngModelChange)="changed()"`.
    allow_two_way_data_binding: bool,
    /// Allow the `class` and `style` attributes next to their bindings, which Angular merges.
    /// See <https://angular.dev/guide/templates/class-binding#styling-precedence>.
    allow_style_precedence_duplicates: bool,
    /// Input or output names that may appear more than once.
    ignore: Vec<String>,
}

impl Default for NoDuplicateAttributesConfig {
    fn default() -> Self {
        Self {
            allow_two_way_data_binding: true,
            allow_style_precedence_duplicates: false,
            ignore: vec![],
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that an element has no duplicate attributes, inputs or outputs.
    ///
    /// ### Why is this bad?
    ///
    /// Angular uses only one of the duplicates and silently ignores the others, so a binding
    /// that looks set has no effect.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <input [name]="foo" [name]="bar">
    /// <input [name]="foo" name="bar">
    /// <input (change)="foo()" (change)="bar()">
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <input [name]="foo" (change)="bar()">
    /// <input [(ngModel)]="model" (ngModelChange)="modelChanged()">
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - The suggestion never joins the attributes on either side of the removed one.
    NoDuplicateAttributes,
    angular_template,
    suspicious,
    suggestion,
    config = NoDuplicateAttributes,
    version = "next",
    short_description = "Ensures that there are no duplicate input properties or output event listeners",
);

impl Rule for NoDuplicateAttributes {
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
    rule: &'c NoDuplicateAttributesConfig,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
}

/// An attribute, input or output as `@angular-eslint`'s `getOriginalAttributeName` names it.
#[derive(Clone, Copy)]
struct Attribute<'a> {
    name: &'a str,
    span: Span,
}

fn duplicates<'a>(attributes: &[Attribute<'a>]) -> Vec<Attribute<'a>> {
    attributes
        .iter()
        .enumerate()
        .filter(|(i, attr)| {
            attributes.iter().enumerate().any(|(j, other)| *i != j && other.name == attr.name)
        })
        .map(|(_, attr)| *attr)
        .collect()
}

impl<'a> Visitor<'_, 'a> {
    fn check(&self, element: &R3Element<'a>) {
        let inputs: Vec<_> = element
            .inputs
            .iter()
            .map(|input| Attribute {
                name: angular_original_attribute_name(self.ctx, self.template, input.key_span),
                span: input.source_span,
            })
            .collect();
        let attributes: Vec<_> = element
            .attributes
            .iter()
            .map(|attr| Attribute { name: attr.name.as_str(), span: attr.source_span })
            .collect();
        let outputs: Vec<_> = element
            .outputs
            .iter()
            // A two-way binding's output shares its span with the input.
            .filter(|output| {
                !self.rule.allow_two_way_data_binding
                    || !element.inputs.iter().any(|input| input.source_span == output.source_span)
            })
            .map(|output| {
                let key = angular_binding_name(self.ctx, self.template, output.key_span);
                let is_two_way = output.name.strip_suffix("Change") == Some(key);
                let name = if is_two_way { output.name.as_str() } else { key };
                Attribute { name, span: output.source_span }
            })
            .collect();

        let is_precedence = |attr: &Attribute| STYLE_PRECEDENCE_ATTRIBUTES.contains(&attr.name);
        let mut found =
            if self.rule.allow_style_precedence_duplicates && inputs.iter().any(is_precedence) {
                // Each group is checked on its own, so `class` next to `[class]` is allowed.
                let (precedence_inputs, other_inputs): (Vec<_>, Vec<_>) =
                    inputs.iter().partition(|attr| is_precedence(attr));
                let (precedence_attributes, other_attributes): (Vec<_>, Vec<_>) =
                    attributes.iter().partition(|attr| is_precedence(attr));
                [precedence_inputs, precedence_attributes, other_inputs, other_attributes]
                    .iter()
                    .flat_map(|group| duplicates(group))
                    .collect()
            } else {
                duplicates(&[inputs, attributes].concat())
            };
        found.extend(duplicates(&outputs));

        for attr in found {
            if self.rule.ignore.iter().any(|name| name == attr.name) {
                continue;
            }
            let span = self.template.span(attr.span);
            let name = attr.name;
            // Takes the space before the attribute too, unless that would join its neighbours.
            let source = self.ctx.source_text().as_bytes();
            let space_before =
                span.start > 0 && source[span.start as usize - 1].is_ascii_whitespace();
            let separated_after = source
                .get(span.end as usize)
                .is_none_or(|c| c.is_ascii_whitespace() || matches!(c, b'>' | b'/'));
            let removal = if space_before && separated_after {
                Span::new(span.start - 1, span.end)
            } else {
                span
            };
            self.ctx.diagnostic_with_suggestion(
                no_duplicate_attributes_diagnostic(name, span),
                |fixer| {
                    fixer.delete_range(removal).with_message(format!("Remove attribute `{name}`"))
                },
            );
        }
    }
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        self.check(element);
        self.visit_element_children(element);
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        (r#"<input name="foo">"#, None),
        (r#"<input [name]="foo">"#, None),
        (r#"<input (change)="bar()">"#, None),
        (r#"<input [(ngModel)]="foo">"#, None),
        (r#"<input [(ngModel)]="model" (ngModelChange)="modelChanged()">"#, None),
        (
            r#"<div (@fade.start)="animationStarted($event)" (@fade.done)="animationDone($event)"></div>"#,
            None,
        ),
        (
            r#"<div (window:keydown)="windowKeydown($event)" (document:keydown)="documentKeydown($event)" (document:keyup)="documentKeyup($event)" (keyup)="keyup($event)" (keydown)="keydown($event)"></div>"#,
            None,
        ),
        (r#"<div [style.width.px]="col.width" [width]="col.width"></div>"#, None),
        (r#"<button [class.disabled]="!enabled" [disabled]="!enabled"></button>"#, None),
        (r#"<button [@.disabled]="!enabled" [.disabled]="!enabled"></button>"#, None),
        (r#"<div [style.width]="col.width + 'px'" [width]="col.width"></div>"#, None),
        (
            r#"
      <div class="foo" name="bar" [class]="dynamic"></div>
    "#,
            Some(json!([{ "allowStylePrecedenceDuplicates": true }])),
        ),
        (
            r#"
      <div style="color: blue" [style]="styleExpression"></div>
    "#,
            Some(json!([{ "allowStylePrecedenceDuplicates": true }])),
        ),
        // Only elements are checked, as in `@angular-eslint`.
        (r#"<ng-template [ngIf]="a" [ngIf]="b"><span></span></ng-template>"#, None),
        (r#"<div *ngIf="show" [name]="foo"></div>"#, None),
    ];

    let fail = vec![
        (r#"<input [name]="foo" [name]="bar">"#, None),
        (r#"<input [name]="foo" name="bar">"#, None),
        (r#"<input name="foo" name="bar">"#, None),
        (r#"<input (change)="foo($event)" (change)="bar($event)">"#, None),
        (r#"<input [(ngModel)]="model" [(ngModel)]="otherModel">"#, None),
        (r#"<input [name]="foo" [other]="bam" [name]="bar">"#, None),
        (r#"<input [name]="foo" [name]="bar" [name]="bam">"#, None),
        (r#"<input [(ngModel)]="model" [name]="foo" [(ngModel)]="otherModel" name="bar">"#, None),
        (
            r#"<input (@fade.start)="animationStarted($event)" (@fade.start)="animationStarted($event)">"#,
            None,
        ),
        (
            r#"<input (window:resize)="windowResized($event)" (resize)="resize()" (window:resize)="windowResized2($event)">"#,
            None,
        ),
        (
            r#"<input [(ngModel)]="model" (ngModelChange)="modelChanged()">"#,
            Some(json!([{ "allowTwoWayDataBinding": false }])),
        ),
        (
            r#"<input [name]="foo" class="css-static" name="bar" [class]="dynamic">"#,
            Some(json!([{ "ignore": ["class"] }])),
        ),
        (
            r#"<input class="foo" class="bar" [class]="dynamic">"#,
            Some(json!([{ "allowStylePrecedenceDuplicates": true }])),
        ),
        (
            r#"<input style="color: blue" [style]="styleExpression" style="width:50px">"#,
            Some(json!([{ "allowStylePrecedenceDuplicates": true }])),
        ),
        (r#"<div class="a" [class]="b"></div>"#, None),
        (r#"<input [attr.name]="foo" name="bar">"#, None),
        (r#"<input bindon-ngModel="model" [(ngModel)]="otherModel">"#, None),
        // Reported once, on the element rather than on the structural directive's template.
        (r#"<div *ngIf="show" [name]="foo" [name]="bar"></div>"#, None),
        (r#"<ng-template [ngIf]="show"><input name="a" name="b"></ng-template>"#, None),
        (r#"@if (show) { <input (change)="a()" (change)="b()"> }"#, None),
    ];

    // Suggestions on one element are applied together, except adjacent ones.
    let fix = vec![
        (r#"<input [name]="foo" [name]="bar">"#, r#"<input [name]="bar">"#, None),
        (r#"<input [name]="foo" name="bar">"#, r#"<input name="bar">"#, None),
        (r#"<input [name]="foo" [other]="bam" [name]="bar">"#, r#"<input [other]="bam">"#, None),
        (
            r#"<input (change)="foo($event)" (change)="bar($event)">"#,
            r#"<input (change)="bar($event)">"#,
            None,
        ),
        (
            r#"<input [(ngModel)]="model" [(ngModel)]="otherModel">"#,
            r#"<input [(ngModel)]="otherModel">"#,
            None,
        ),
        (r#"<input [name]="foo" [name]="bar" [name]="bam">"#, r#"<input [name]="bar">"#, None),
        (
            r#"<input [(ngModel)]="model" [name]="foo" [(ngModel)]="otherModel" name="bar">"#,
            r#"<input [name]="foo" name="bar">"#,
            None,
        ),
        (
            r#"<input (@fade.start)="animationStarted($event)" (@fade.start)="animationStarted($event)">"#,
            r#"<input (@fade.start)="animationStarted($event)">"#,
            None,
        ),
        (
            r#"<input (window:resize)="windowResized($event)" (resize)="resize()" (window:resize)="windowResized2($event)">"#,
            r#"<input (resize)="resize()">"#,
            None,
        ),
        (
            r#"<input [(ngModel)]="model" (ngModelChange)="modelChanged()">"#,
            r#"<input (ngModelChange)="modelChanged()">"#,
            Some(json!([{ "allowTwoWayDataBinding": false }])),
        ),
        (
            r#"<input [name]="foo" class="css-static" name="bar" [class]="dynamic">"#,
            r#"<input class="css-static" [class]="dynamic">"#,
            Some(json!([{ "ignore": ["class"] }])),
        ),
        (
            r#"<input class="foo" class="bar" [class]="dynamic">"#,
            r#"<input class="bar" [class]="dynamic">"#,
            Some(json!([{ "allowStylePrecedenceDuplicates": true }])),
        ),
        (
            r#"<input style="color: blue" [style]="styleExpression" style="width:50px">"#,
            r#"<input [style]="styleExpression">"#,
            Some(json!([{ "allowStylePrecedenceDuplicates": true }])),
        ),
        (
            r#"<div *ngIf="show" [name]="foo" [name]="bar"></div>"#,
            r#"<div *ngIf="show" [name]="bar"></div>"#,
            None,
        ),
        (r#"<input name="foo"name="bar">"#, r#"<input name="bar">"#, None),
    ];

    Tester::new(NoDuplicateAttributes::NAME, NoDuplicateAttributes::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<input [name]=\"a\" (change)=\"b()\">' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<input [name]=\"a\" name=\"b\">' }) class A {}",
    ];
    let inline_fix = vec![(
        "import { Component } from '@angular/core'; @Component({ template: '<input [name]=\"a\" [name]=\"b\">' }) class A {}",
        "import { Component } from '@angular/core'; @Component({ template: '<input [name]=\"b\">' }) class A {}",
    )];
    Tester::new(
        NoDuplicateAttributes::NAME,
        NoDuplicateAttributes::PLUGIN,
        inline_pass,
        inline_fail,
    )
    .change_rule_path_extension("ts")
    .expect_fix(inline_fix)
    .with_snapshot_suffix("inline")
    .test_and_snapshot();
}
