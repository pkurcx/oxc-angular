use oxc_angular_compiler::ast::r3::{R3Element, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{AngularTemplate, is_angular_dom_element},
};

fn no_autofocus_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "The `autofocus` attribute should not be used, as it reduces usability and accessibility for users",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoAutofocus;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that the `autofocus` attribute is not used, except inside a `<dialog>`.
    ///
    /// ### Why is this bad?
    ///
    /// Moving focus on page load disorients screen reader users and skips the content before
    /// the focused element. Inside a modal `<dialog>`, autofocus is the recommended practice.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <input autofocus>
    /// <button [autofocus]="true"></button>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <input>
    /// <dialog><button autofocus>Close</button></dialog>
    /// ```
    NoAutofocus,
    angular_template,
    correctness,
    fix,
    version = "next",
    short_description = "Ensures that the `autofocus` attribute is not used",
);

impl Rule for NoAutofocus {
    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { ctx, template, dialogs: 0 }, template.nodes);
        }
    }
}

struct Visitor<'c, 'a> {
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
    dialogs: u32,
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        let is_dialog = element.name.eq_ignore_ascii_case("dialog");
        self.dialogs += u32::from(is_dialog);
        if self.dialogs == 0 && is_angular_dom_element(&element.name, false) {
            let attributes = element.attributes.iter().map(|a| (a.name.as_str(), a.source_span));
            let inputs = element.inputs.iter().map(|a| (a.name.as_str(), a.source_span));
            for (_, span) in attributes.chain(inputs).filter(|(name, _)| *name == "autofocus") {
                let span = self.template.span(span);
                // Also removes the space before the attribute.
                self.ctx.diagnostic_with_fix(no_autofocus_diagnostic(span), |fixer| {
                    fixer.delete_range(Span::new(span.start - 1, span.end))
                });
            }
        }
        self.visit_element_children(element);
        self.dialogs -= u32::from(is_dialog);
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<input type="text">"#,
        "<app-input autofocus></app-input>",
        "<dialog autofocus></dialog>",
        "<dialog><div><button autofocus>Close</button></div></dialog>",
        r#"<dialog><button [attr.autofocus]="true">Close</button></dialog>"#,
    ];

    let fail = vec![
        "<input autofocus>",
        r#"<button autofocus="autofocus"></button>"#,
        r#"<div [autofocus]="true"></div>"#,
        r#"<div *ngIf="x"><textarea autofocus></textarea></div>"#,
        r#"<input [attr.autofocus]="false">"#,
    ];

    let fix = vec![
        (r#"<input type="text" autofocus>"#, r#"<input type="text">"#),
        (r#"<div [autofocus]="true"></div>"#, "<div></div>"),
    ];

    Tester::new(NoAutofocus::NAME, NoAutofocus::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
