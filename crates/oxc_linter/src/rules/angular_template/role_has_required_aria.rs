use oxc_angular_compiler::ast::r3::{R3Element, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{AngularTemplate, is_angular_dom_element},
};

fn role_has_required_aria_diagnostic(
    element: &str,
    role: &str,
    missing: &str,
    span: Span,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The {element} with role=\"{role}\" does not have required ARIA properties: {missing}"
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RoleHasRequiredAria;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that elements with an ARIA `role` have all the properties that role requires,
    /// unless the element provides them natively, like `<input type="checkbox" role="switch">`.
    ///
    /// ### Why is this bad?
    ///
    /// Assistive technology reads the state of a role from its required properties; a
    /// `role="checkbox"` without `aria-checked` has no state to announce.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <span role="checkbox"></span>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <span role="checkbox" [attr.aria-checked]="checked"></span>
    /// ```
    RoleHasRequiredAria,
    angular_template,
    correctness,
    suggestion,
    version = "next",
    short_description = "Ensures elements with ARIA roles have all required properties for that role.",
);

// From `aria-query` 5.3.2.
const REQUIRED_PROPS: &[(&str, &[&str])] = &[
    ("checkbox", &["aria-checked"]),
    ("combobox", &["aria-controls", "aria-expanded"]),
    ("heading", &["aria-level"]),
    ("menuitemcheckbox", &["aria-checked"]),
    ("menuitemradio", &["aria-checked"]),
    ("meter", &["aria-valuenow"]),
    ("option", &["aria-selected"]),
    ("radio", &["aria-checked"]),
    ("scrollbar", &["aria-controls", "aria-valuenow"]),
    ("slider", &["aria-valuenow"]),
    ("switch", &["aria-checked"]),
    ("treeitem", &["aria-selected"]),
];

/// An element, a static attribute it needs, and the roles whose properties it provides.
type SemanticRoleElement =
    (&'static str, Option<(&'static str, &'static str)>, &'static [&'static str]);

// From `axobject-query` 4.1.0.
const SEMANTIC_ROLE_ELEMENTS: &[SemanticRoleElement] = &[
    ("input", Some(("type", "checkbox")), &["checkbox", "switch"]),
    ("input", Some(("type", "radio")), &["radio"]),
    ("input", Some(("type", "range")), &["slider"]),
    ("select", None, &["combobox"]),
    ("option", None, &["option"]),
    ("h1", None, &["heading"]),
    ("h2", None, &["heading"]),
    ("h3", None, &["heading"]),
    ("h4", None, &["heading"]),
    ("h5", None, &["heading"]),
    ("h6", None, &["heading"]),
];

impl Rule for RoleHasRequiredAria {
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

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        if is_angular_dom_element(&element.name, true) {
            for attr in element.attributes.iter().filter(|attr| attr.name == "role") {
                let role = attr.value.as_str();
                let Some((_, required)) = REQUIRED_PROPS.iter().find(|(r, _)| *r == role) else {
                    continue;
                };
                if is_semantic_role_element(element, role) {
                    continue;
                }
                let has = |prop: &str| {
                    element.attributes.iter().any(|a| a.name == prop)
                        || element.inputs.iter().any(|i| i.name == prop)
                };
                let missing: Vec<&str> =
                    required.iter().copied().filter(|prop| !has(prop)).collect();
                if missing.is_empty() {
                    continue;
                }
                let span = self.template.span(attr.source_span);
                let diagnostic = role_has_required_aria_diagnostic(
                    &element.name,
                    role,
                    &missing.join(", "),
                    span,
                );
                self.ctx.diagnostic_with_suggestion(diagnostic, |fixer| {
                    fixer
                        .delete_range(Span::new(span.start - 1, span.end))
                        .with_message(format!("Remove role `{role}`"))
                });
            }
        }
        self.visit_element_children(element);
    }
}

fn is_semantic_role_element(element: &R3Element, role: &str) -> bool {
    SEMANTIC_ROLE_ELEMENTS.iter().any(|(name, attribute, roles)| {
        element.name == *name
            && roles.contains(&role)
            && attribute.is_none_or(|(attr_name, value)| {
                element.attributes.iter().any(|a| a.name == attr_name && a.value == value)
            })
    })
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<span role="checkbox" aria-checked="false"></span>"#,
        r#"<span role="checkbox" [attr.aria-checked]="checked"></span>"#,
        r#"<div role="combobox" aria-expanded="false" aria-controls="x"></div>"#,
        r#"<input type="checkbox" role="switch">"#,
        r#"<input type="range" role="slider">"#,
        r#"<h1 role="heading"></h1>"#,
        r#"<div role="button"></div>"#,
        r#"<app-x role="checkbox"></app-x>"#,
        r#"<div [attr.role]="role"></div>"#,
    ];

    let fail = vec![
        r#"<span role="checkbox"></span>"#,
        r#"<div role="combobox" aria-expanded="false"></div>"#,
        r#"<div role="scrollbar"></div>"#,
        r#"<DIV role="treeitem"></DIV>"#,
        r#"<input type="text" role="switch">"#,
    ];

    let fix = vec![(r#"<span role="checkbox"></span>"#, "<span></span>")];

    Tester::new(RoleHasRequiredAria::NAME, RoleHasRequiredAria::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
