use oxc_angular_compiler::ast::r3::{R3Element, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{context::LintContext, rule::Rule, utils::AngularTemplate};

fn no_distracting_elements_diagnostic(element: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Do not use <{element}> elements as they can create visual accessibility issues and are deprecated"
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDistractingElements;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the distracting `<marquee>` and `<blink>` elements.
    ///
    /// ### Why is this bad?
    ///
    /// Moving and blinking content is hard to read and can cause seizures; both elements are
    /// deprecated.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <marquee>News</marquee>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <p>News</p>
    /// ```
    NoDistractingElements,
    angular_template,
    correctness,
    fix,
    version = "next",
    short_description = "Enforces that no distracting elements are used",
);

impl Rule for NoDistractingElements {
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
        let name = element.name.as_str();
        if name.eq_ignore_ascii_case("marquee") || name.eq_ignore_ascii_case("blink") {
            let span = self.template.span(element.source_span);
            self.ctx.diagnostic_with_fix(no_distracting_elements_diagnostic(name, span), |fixer| {
                fixer.delete_range(span)
            });
        }
        self.visit_element_children(element);
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec!["<div></div>", "<app-marquee></app-marquee>"];

    let fail = vec!["<marquee></marquee>", "<blink></blink>", "<div><MARQUEE>x</MARQUEE></div>"];

    let fix = vec![("<div><marquee>x</marquee></div>", "<div></div>")];

    Tester::new(NoDistractingElements::NAME, NoDistractingElements::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
