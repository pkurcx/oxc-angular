use oxc_angular_compiler::ast::r3::{R3BoundEvent, R3Template, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{AngularTemplate, visit_template_in_eslint_order},
};

fn banana_in_box_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Invalid binding syntax. Use [(expr)] instead").with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct BananaInBox;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that the two-way data binding syntax is correct.
    ///
    /// ### Why is this bad?
    ///
    /// `([ngModel])="value"` has the parentheses outside the brackets, which Angular reads as an
    /// event binding to an event named `[ngModel]`, so the value never updates.
    /// Two-way binding is written `[(ngModel)]`, a banana in a box.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <input ([ngModel])="name">
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <input [(ngModel)]="name">
    /// ```
    BananaInBox,
    angular_template,
    correctness,
    fix,
    version = "next",
    short_description = "Ensures that the two-way data binding syntax is correct",
);

impl Rule for BananaInBox {
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
    fn visit_bound_event(&mut self, event: &R3BoundEvent<'a>) {
        let name = event.name.as_str();
        let Some(inner) = name.strip_prefix('[').and_then(|name| name.strip_suffix(']')) else {
            return;
        };
        let span = self.template.span(event.source_span);
        // `name` is written between the outer parentheses: `([name])`.
        #[expect(clippy::cast_possible_truncation)]
        let key = Span::sized(span.start, name.len() as u32 + 2);
        self.ctx.diagnostic_with_fix(banana_in_box_diagnostic(span), |fixer| {
            fixer.replace(key, format!("[({inner})]"))
        });
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        visit_template_in_eslint_order(self, template);
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<input type="text" [ngModel]="value" (ngModelChange)="update($event)">"#,
        r#"<input type="text" [(ngModel)]="value">"#,
        r#"<app-item (change)="save()"></app-item>"#,
    ];

    let fail = vec![
        r#"<input type="text" ([ngModel])="value">"#,
        r#"<app-item ([value])="item"><div ([open])="isOpen"></div></app-item>"#,
        r#"<input *ngIf="show" ([ngModel])="value">"#,
    ];

    let fix = vec![
        (r#"<input ([ngModel])="value">"#, r#"<input [(ngModel)]="value">"#),
        (r#"<a ([x])="b"></a><b ([y])="c"></b>"#, r#"<a [(x)]="b"></a><b [(y)]="c"></b>"#),
    ];

    Tester::new(BananaInBox::NAME, BananaInBox::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<input [(x)]=\"y\">' }) class A {}",
        // Not an Angular component, so not a template.
        "@Component({ template: '<input ([x])=\"y\">' }) class A {}",
        // Escape sequences: skipped rather than misparsed.
        "import { Component } from '@angular/core'; @Component({ template: '<div *ngIf=\"m === \\'e\\'\">x</div>' }) class A {}",
        "import { Component } from '@angular/core'; @Component({ template: `\n<!-- eslint-disable-next-line @angular-eslint/template/banana-in-box -->\n<input ([x])=\"y\">` }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<input ([x])=\"y\">' }) class A {}",
        "import { Component } from '@angular/core'; @Component({ selector: 'a', template: `\n  <input ([x])=\"y\">\n` }) class A {}",
    ];
    let inline_fix = vec![(
        "import { Component } from '@angular/core'; @Component({ template: '<a ([x])=\"y\"></a>' }) class A {}",
        "import { Component } from '@angular/core'; @Component({ template: '<a [(x)]=\"y\"></a>' }) class A {}",
    )];
    Tester::new(BananaInBox::NAME, BananaInBox::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .expect_fix(inline_fix)
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
