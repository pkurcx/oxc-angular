use cow_utils::CowUtils;
use oxc_angular_compiler::ast::r3::{R3Element, R3Node, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{AngularTemplate, angular_attribute_value},
};

fn alt_text_diagnostic(element: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("<{element}/> element must have a text alternative."))
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct AltText;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces a text alternative on `<img>`, `<area>`, `<object>` and `<input type="image">`:
    /// `alt`, `aria-label` or `aria-labelledby` as an attribute or binding, `title` or content
    /// for `<object>`.
    ///
    /// ### Why is this bad?
    ///
    /// Screen readers announce these elements by their text alternative; without one, users
    /// get the file name or nothing.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <img src="logo.png">
    /// <input type="image" src="submit.png">
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <img src="logo.png" alt="Company logo">
    /// <input type="image" src="submit.png" [attr.aria-label]="label">
    /// ```
    AltText,
    angular_template,
    correctness,
    version = "next",
    short_description = "Enforces alternate text for elements which require the alt, aria-label, aria-labelledby attributes.",
);

impl Rule for AltText {
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
        if !self.is_valid(element) {
            let span = self.template.span(element.source_span);
            self.ctx.diagnostic(alt_text_diagnostic(&element.name, span));
        }
        self.visit_element_children(element);
    }
}

impl<'a> Visitor<'_, 'a> {
    fn is_valid(&self, element: &R3Element<'a>) -> bool {
        let names = || {
            let attributes = element.attributes.iter().map(|attr| attr.name.as_str());
            attributes.chain(element.inputs.iter().map(|input| input.name.as_str()))
        };
        let has = |pred: &dyn Fn(&str) -> bool| names().any(pred);
        let is_alt = |name: &str| name == "alt";
        let is_aria_label = |name: &str| name == "aria-label" || name == "aria-labelledby";
        match element.name.cow_to_ascii_lowercase().as_ref() {
            "img" => has(&is_alt),
            "area" => has(&|name| is_alt(name) || is_aria_label(name)),
            "object" => {
                has(&|name| name == "title" || is_aria_label(name))
                    || match element.children.first() {
                        Some(R3Node::Text(text)) => !text.value.is_empty(),
                        Some(R3Node::BoundText(_)) => true,
                        _ => false,
                    }
            }
            "input" => {
                let is_image = angular_attribute_value(self.ctx, self.template, element, "type")
                    .and_then(|value| value.as_str().map(|v| v == "image"))
                    .unwrap_or(false);
                !is_image || has(&|name| is_alt(name) || is_aria_label(name))
            }
            _ => true,
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<img src="foo" alt="Foo eating a sandwich.">"#,
        r#"<img src="bar" [attr.alt]="altText">"#,
        r#"<img src="baz" [alt]="altText">"#,
        r#"<img src="qux" alt="">"#,
        r#"<object aria-label="foo"></object>"#,
        r#"<object title="foo"></object>"#,
        "<object>Foo</object>",
        r#"<object [attr.aria-labelledby]="id"></object>"#,
        r#"<area alt="foo">"#,
        r#"<area [attr.aria-label]="label">"#,
        r#"<input type="text">"#,
        r#"<input type="image" alt="foo">"#,
        r#"<input type="image" aria-labelledby="id">"#,
    ];

    let fail = vec![
        r#"<img src="foo">"#,
        r#"<IMG src="foo">"#,
        "<object></object>",
        "<object><span>x</span></object>",
        r#"<area href="x">"#,
        r#"<input type="image">"#,
        r#"<input [type]="'image'">"#,
    ];

    Tester::new(AltText::NAME, AltText::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();
}
