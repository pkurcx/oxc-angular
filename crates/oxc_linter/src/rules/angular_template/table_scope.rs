use oxc_angular_compiler::ast::r3::{R3Element, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{AngularTemplate, is_angular_dom_element},
};

fn table_scope_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The `scope` attribute should only be on the `<th>` element")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct TableScope;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that the `scope` attribute is only used on the `<th>` element.
    ///
    /// ### Why is this bad?
    ///
    /// Screen readers use `scope` to associate table headers with cells; on any other element it
    /// has no meaning and may confuse them.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <td scope="col"></td>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <th scope="col"></th>
    /// ```
    TableScope,
    angular_template,
    correctness,
    fix,
    version = "next",
    short_description = "Ensures that the `scope` attribute is only used on the `<th>` element",
);

impl Rule for TableScope {
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
        if name != "th" && name != "TH" && is_angular_dom_element(name, true) {
            let attributes = element.attributes.iter().map(|a| (a.name.as_str(), a.source_span));
            let inputs = element.inputs.iter().map(|a| (a.name.as_str(), a.source_span));
            for (_, span) in attributes.chain(inputs).filter(|(name, _)| *name == "scope") {
                let span = self.template.span(span);
                self.ctx.diagnostic_with_fix(table_scope_diagnostic(span), |fixer| {
                    fixer.delete_range(Span::new(span.start - 1, span.end))
                });
            }
        }
        self.visit_element_children(element);
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<th scope="col"></th>"#,
        r#"<th [attr.scope]="scope"></th>"#,
        r#"<app-cell scope="row"></app-cell>"#,
        r#"<TH scope="col"></TH>"#,
    ];

    let fail = vec![
        r#"<div scope="col"></div>"#,
        r#"<td [attr.scope]="scope"></td>"#,
        r#"<DIV scope="row"></DIV>"#,
    ];

    let fix = vec![(r#"<td scope="col"></td>"#, "<td></td>")];

    Tester::new(TableScope::NAME, TableScope::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
