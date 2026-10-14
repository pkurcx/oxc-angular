use oxc_angular_compiler::ast::{
    expression::{AngularExpression, LiteralValue},
    r3::{R3Element, R3Visitor, visit_all},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_ecmascript::StringToNumber;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    fixer::RuleFixer,
    rule::Rule,
    utils::{AngularTemplate, is_angular_dom_element},
};

fn no_positive_tabindex_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The `tabindex` attribute should not be positive").with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoPositiveTabindex;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that the `tabindex` attribute is not positive.
    ///
    /// ### Why is this bad?
    ///
    /// A positive `tabindex` moves the element ahead of the natural tab order, which follows
    /// the document. Keyboard users then meet a focus order that does not match the page.
    /// Use `tabindex="0"` to make an element focusable in document order, or `tabindex="-1"`
    /// to only focus it from code.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div tabindex="5"></div>
    /// <div [attr.tabindex]="21"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div tabindex="0"></div>
    /// <div [attr.tabindex]="-1"></div>
    /// ```
    NoPositiveTabindex,
    angular_template,
    pedantic,
    suggestion,
    version = "next",
    short_description = "Ensures that the `tabindex` attribute is not positive",
);

impl Rule for NoPositiveTabindex {
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
        self.visit_element_children(element);
        // Bound values only on native elements, static ones on any element, as the selector
        // `Element[name=…] > BoundAttribute[…], TextAttribute[…]` of `@angular-eslint` does.
        if is_angular_dom_element(&element.name, true) {
            for input in element.inputs.iter().filter(|input| input.name == "tabindex") {
                let AngularExpression::LiteralPrimitive(literal) = &input.value else { continue };
                let positive = match &literal.value {
                    LiteralValue::Number(value) => *value > 0.0,
                    LiteralValue::String(value) => value.as_str().string_to_number() > 0.0,
                    LiteralValue::Boolean(value) => *value,
                    _ => false,
                };
                if positive && let Some(span) = input.value_span {
                    self.report(span);
                }
            }
        }
        for attr in element.attributes.iter().filter(|attr| attr.name == "tabindex") {
            if attr.value.as_str().string_to_number() > 0.0
                && let Some(span) = attr.value_span
            {
                self.report(span);
            }
        }
    }
}

impl Visitor<'_, '_> {
    fn report(&self, value_span: Span) {
        let span = self.template.span(value_span);
        let fixer = RuleFixer::new(FixKind::Suggestion, self.ctx);
        let suggestions = ["-1", "0"].map(|tabindex| {
            fixer.replace(span, tabindex).with_message(format!("Use `tabindex=\"{tabindex}\"`"))
        });
        self.ctx.diagnostic_with_suggestions(no_positive_tabindex_diagnostic(span), suggestions);
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "<span></span>",
        r#"<span id="2"></span>"#,
        "<span tabindex></span>",
        r#"<span tabindex="-1"></span>"#,
        r#"<span tabindex="0"></span>"#,
        r#"<span [attr.tabindex]="-1"></span>"#,
        r#"<span [attr.tabindex]="0"></span>"#,
        r#"<span [attr.tabindex]="tabIndex"></span>"#,
        r#"<span [attr.tabindex]="null"></span>"#,
        r#"<span [attr.tabindex]="undefined"></span>"#,
        r#"<app-test [tabindex]="1"></app-test>"#,
        r#"<span tabindex="abc"></span>"#,
        r#"<span tabindex="{{ 1 }}"></span>"#,
        r#"<span [tabIndex]="1"></span>"#,
        // The attributes of `<ng-template>` are not checked, as in `@angular-eslint`.
        r#"<ng-template tabindex="1"></ng-template>"#,
        r#"<ng-template [tabindex]="1"></ng-template>"#,
        r#"<div *ngIf="show" tabindex="0"></div>"#,
    ];

    let fail = vec![
        r#"<div tabindex="5"></div>"#,
        r#"<DIV tabindex="5"></DIV>"#,
        r#"<div [attr.tabindex]="21"></div>"#,
        r#"<div [tabindex]="'2'"></div>"#,
        r#"<div [tabindex]="true"></div>"#,
        r#"<span tabindex=" 0x10 "></span>"#,
        // Static values are checked on any element.
        r#"<app-test tabindex="1"></app-test>"#,
        r#"<div *ngIf="show" tabindex="1"></div>"#,
        r#"<div *ngIf="show" [attr.tabindex]="1"></div>"#,
        r#"<ng-template><span tabindex="1"></span></ng-template>"#,
    ];

    let fix = vec![
        (
            r#"<div tabindex="5"></div>"#,
            (r#"<div tabindex="-1"></div>"#, r#"<div tabindex="0"></div>"#),
        ),
        (
            r#"<DIV tabindex="5"></DIV>"#,
            (r#"<DIV tabindex="-1"></DIV>"#, r#"<DIV tabindex="0"></DIV>"#),
        ),
        (
            r#"<div [attr.tabindex]="21"></div>"#,
            (r#"<div [attr.tabindex]="-1"></div>"#, r#"<div [attr.tabindex]="0"></div>"#),
        ),
    ];

    Tester::new(NoPositiveTabindex::NAME, NoPositiveTabindex::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<span tabindex=\"0\"></span>' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<span tabindex=\"3\"></span>' }) class A {}",
    ];
    let inline_fix = vec![(
        "import { Component } from '@angular/core'; @Component({ template: '<span tabindex=\"3\"></span>' }) class A {}",
        (
            "import { Component } from '@angular/core'; @Component({ template: '<span tabindex=\"-1\"></span>' }) class A {}",
            "import { Component } from '@angular/core'; @Component({ template: '<span tabindex=\"0\"></span>' }) class A {}",
        ),
    )];
    Tester::new(NoPositiveTabindex::NAME, NoPositiveTabindex::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .expect_fix(inline_fix)
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
