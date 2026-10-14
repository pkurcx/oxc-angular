use oxc_angular_compiler::ast::r3::{
    R3BoundAttribute, R3Element, R3Template, R3Visitor, visit_all,
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{AngularTemplate, visit_element_in_eslint_order, visit_template_in_eslint_order},
};

fn prefer_control_flow_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Use built-in control flow instead of directive {name}."))
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferControlFlow;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that the built-in control flow (`@if`, `@for`, `@switch`) is used instead of the
    /// `ngIf`, `ngFor` and `ngSwitch` directives.
    ///
    /// ### Why is this bad?
    ///
    /// The built-in control flow needs no imports, type-checks better, and `@for` requires a
    /// `track` expression, which avoids re-rendering whole lists.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div *ngIf="visible">Hi</div>
    /// <li *ngFor="let item of items">{{ item }}</li>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// @if (visible) { <div>Hi</div> }
    /// @for (item of items; track item.id) { <li>{{ item }}</li> }
    /// ```
    PreferControlFlow,
    angular_template,
    style,
    version = "next",
    short_description = "Ensures that the built-in control flow is used.",
);

impl Rule for PreferControlFlow {
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
        visit_element_in_eslint_order(self, element);
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        visit_template_in_eslint_order(self, template);
    }

    fn visit_bound_attribute(&mut self, attr: &R3BoundAttribute<'a>) {
        let name = attr.name.as_str();
        if matches!(name, "ngForOf" | "ngIf" | "ngSwitch") {
            let span = self.template.span(attr.source_span);
            self.ctx.diagnostic(prefer_control_flow_diagnostic(name, span));
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "@if (x) { <div>x</div> }",
        "@for (x of xs; track x) { <li>{{ x }}</li> }",
        r#"<div [ngClass]="x" [ngStyle]="y"></div>"#,
        r#"<div [ngSwitchCase]="1"></div>"#,
    ];

    let fail = vec![
        r#"<div *ngIf="x"></div>"#,
        r#"<li *ngFor="let x of xs; trackBy: id"></li>"#,
        r#"<div [ngSwitch]="x"><span *ngSwitchCase="1"></span></div>"#,
        r#"<ng-template [ngIf]="x"><p></p></ng-template>"#,
        r#"<div *ngIf="x" [ngSwitch]="y"></div>"#,
    ];

    Tester::new(PreferControlFlow::NAME, PreferControlFlow::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();
}
