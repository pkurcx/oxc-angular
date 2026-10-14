use cow_utils::CowUtils;
use oxc_angular_compiler::ast::r3::{R3Element, R3Node};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::angular_elements,
};

fn label_has_associated_control_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("A label component must be associated with a form element").with_label(span)
}

const CONTROL_COMPONENTS: [&str; 6] =
    ["input", "meter", "output", "progress", "select", "textarea"];

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LabelComponent {
    /// Element name of the label component.
    selector: String,
    /// Inputs that associate it with a control, like `for`.
    #[serde(default)]
    inputs: Vec<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
pub struct LabelHasAssociatedControl(Box<AngularLabelHasAssociatedControlConfig>);

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct AngularLabelHasAssociatedControlConfig {
    /// Require the `for` value to match the `id` of a control in the same template.
    check_ids: bool,
    /// Element names of controls, in addition to `input`, `meter`, `output`, `progress`,
    /// `select` and `textarea`.
    control_components: Vec<String>,
    /// Label components, in addition to `label` with the `for` and `htmlFor` inputs.
    label_components: Vec<LabelComponent>,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that a label element or component is associated with a form control, through a
    /// `for` attribute or by containing the control.
    ///
    /// ### Why is this bad?
    ///
    /// A label that is not associated with its control is not announced with it, and clicking
    /// the label does not focus the control.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <label>Name</label>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <label for="name">Name</label>
    /// <label>Name <input id="name"></label>
    /// ```
    LabelHasAssociatedControl,
    angular_template,
    correctness,
    config = LabelHasAssociatedControl,
    version = "next",
    short_description = "Ensures that a label element/component is associated with a form element",
);

impl Rule for LabelHasAssociatedControl {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        let mut label_components = vec![LabelComponent {
            selector: "label".to_string(),
            inputs: vec!["for".to_string(), "htmlFor".to_string()],
        }];
        for component in &self.0.label_components {
            match label_components.iter_mut().find(|c| c.selector == component.selector) {
                Some(existing) => *existing = component.clone(),
                None => label_components.push(component.clone()),
            }
        }
        let controls: Vec<&str> = CONTROL_COMPONENTS
            .into_iter()
            .chain(self.0.control_components.iter().map(String::as_str))
            .collect();

        // A template's labels are checked against its own controls; `@angular-eslint` lints each
        // inline template as a file.
        for template in ctx.angular_templates() {
            let elements = angular_elements(template.nodes);
            let control_ids: Vec<&str> = elements
                .iter()
                .filter(|element| {
                    controls.contains(&element.name.cow_to_ascii_lowercase().as_ref())
                })
                .flat_map(|element| {
                    element
                        .attributes
                        .iter()
                        .filter(|attr| attr.name == "id")
                        .map(|attr| attr.value.as_str())
                })
                .collect();

            for label in &elements {
                let name = label.name.cow_to_ascii_lowercase();
                let Some(component) = label_components.iter().find(|c| c.selector == name.as_ref())
                else {
                    continue;
                };
                // The first `for`-like attribute or input; only an attribute has a text value.
                let for_value = component.inputs.iter().find_map(|input| {
                    let attribute =
                        label.attributes.iter().find(|attr| attr.name == input.as_str());
                    let bound = label.inputs.iter().any(|i| i.name == input.as_str());
                    attribute.map(|attr| Some(attr.value.as_str())).or(bound.then_some(None))
                });
                let has_for = match for_value {
                    // A bound value never equals a control's `id`, as in `@angular-eslint`.
                    Some(value) if self.0.check_ids => {
                        value.is_some_and(|value| control_ids.contains(&value))
                    }
                    Some(_) => true,
                    None => false,
                };
                if !has_for && !contains_control(label, &controls) {
                    let span = template.span(label.source_span);
                    ctx.diagnostic(label_has_associated_control_diagnostic(span));
                }
            }
        }
    }
}

/// Whether a control is nested in `element` through elements only, as `@angular-eslint` looks
/// for it.
fn contains_control(element: &R3Element, controls: &[&str]) -> bool {
    element.children.iter().any(|child| match child {
        R3Node::Element(child) => {
            controls.contains(&child.name.as_str()) || contains_control(child, controls)
        }
        _ => false,
    })
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        (r#"<label for="name"></label>"#, None),
        (r#"<label [for]="id"></label>"#, None),
        (r#"<label [attr.for]="id"></label>"#, None),
        (r#"<label [htmlFor]="id"></label>"#, None),
        ("<label>Name <input></label>", None),
        ("<label>Name <span><select></select></span></label>", None),
        (
            r#"<app-label [target]="x"></app-label>"#,
            Some(
                json!([{ "labelComponents": [{ "selector": "app-label", "inputs": ["target"] }] }]),
            ),
        ),
        (
            "<label><app-input></app-input></label>",
            Some(json!([{ "controlComponents": ["app-input"] }])),
        ),
        (r#"<label for="n"></label><input id="n">"#, Some(json!([{ "checkIds": true }]))),
        ("<div></div>", None),
    ];

    let fail = vec![
        ("<label>Name</label>", None),
        ("<label><span>Name</span></label>", None),
        (r#"<label><input *ngIf="x"></label>"#, None),
        (
            "<app-label></app-label>",
            Some(
                json!([{ "labelComponents": [{ "selector": "app-label", "inputs": ["target"] }] }]),
            ),
        ),
        (r#"<label for="n"></label><input id="m">"#, Some(json!([{ "checkIds": true }]))),
        (r#"<label [for]="n"></label><input id="n">"#, Some(json!([{ "checkIds": true }]))),
    ];

    Tester::new(LabelHasAssociatedControl::NAME, LabelHasAssociatedControl::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    // A label's control must be in its own inline template.
    let label_and_control = "import { Component } from '@angular/core';
@Component({ template: '<label for=\"n\"></label><input id=\"n\">' }) class A {}";
    let split = "import { Component } from '@angular/core';
@Component({ template: '<label for=\"n\"></label>' }) class A {}
@Component({ template: '<input id=\"n\">' }) class B {}";
    Tester::new(
        LabelHasAssociatedControl::NAME,
        LabelHasAssociatedControl::PLUGIN,
        vec![(label_and_control, Some(json!([{ "checkIds": true }])))],
        vec![(split, Some(json!([{ "checkIds": true }])))],
    )
    .change_rule_path_extension("ts")
    .with_snapshot_suffix("inline")
    .test_and_snapshot();
}
