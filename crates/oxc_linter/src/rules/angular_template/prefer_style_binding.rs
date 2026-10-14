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

fn prefer_style_binding_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Consider using [style] or [style.property] bindings instead of [ngStyle] where applicable.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferStyleBinding;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Suggests `[style]` and `[style.property]` bindings over `[ngStyle]`.
    ///
    /// ### Why is this bad?
    ///
    /// Style bindings are simpler and faster than the `NgStyle` directive, and the Angular style
    /// guide recommends them. `NgStyle` is still needed to pick up mutations of the bound
    /// object. Keys with a unit suffix, as in `{ 'max-width.px': width }`, can be written as
    /// `[style.max-width.px]` bindings.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div [ngStyle]="{ color: textColor }"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div [style.color]="textColor"></div>
    /// <div [style]="{ color: textColor }"></div>
    /// ```
    PreferStyleBinding,
    angular_template,
    style,
    version = "next",
    short_description = "Suggests using [style] bindings over ngStyle where applicable",
);

impl Rule for PreferStyleBinding {
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
        if attr.name == "ngStyle" {
            let span = self.template.span(attr.source_span);
            self.ctx.diagnostic(prefer_style_binding_diagnostic(span));
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<div [style.color]="textColor"></div>"#,
        r#"<div style="color: red"></div>"#,
        r#"<div style="margin: 0" [style.color]="textColor"></div>"#,
        r#"<div [style.color]="textColor" [style.background-color]="backgroundColor"></div>"#,
        r#"<section [style.height.px]="sectionHeightInPixels()"></section>"#,
        r#"<div [style]="{ color: textColor, 'background-color': backgroundColor }"></div>"#,
        r#"<div [style]="'display: flex; padding: 8px'"></div>"#,
        r#"<div [style]="styles"></div>"#,
        r#"<div [style]="getStyles()"></div>"#,
        r#"<button [style.display]="isExpanded() ? 'block' : 'none'">Toggle</button>"#,
        r#"<input type="text" [style.width.px]="inputWidth" [value]="inputValue">"#,
        r#"<div [ngClass]="{ active: isActive }"></div>"#,
        r#"<div [class.active]="isActive"></div>"#,
        r#"<div [myNgStyle]="styles"></div>"#,
        r#"<div *ngIf="show" [style.color]="textColor"></div>"#,
    ];

    let fail = vec![
        r#"<div [ngStyle]="{ color: textColor, 'background-color': backgroundColor }"></div>"#,
        r#"<div [ngStyle]="styles"></div>"#,
        r#"<div [ngStyle]="getStyles()"></div>"#,
        r#"<button [ngStyle]="isActive ? activeStyles : inactiveStyles"></button>"#,
        r#"<div style="margin: 0" [ngStyle]="dynamicStyles"></div>"#,
        r#"<div [ngStyle]="{ color: textColor }" [style.font-weight]="'bold'"></div>"#,
        r#"<input type="text" [ngStyle]="inputStyles" [value]="text">"#,
        r#"<app-card [ngStyle]="cardStyles"></app-card>"#,
        r#"<div [ngStyle]="{ 'max-width.px': width }"></div>"#,
        r#"<div bind-ngStyle="styles"></div>"#,
        // Reported once, on the element rather than on the structural directive's template.
        r#"<div *ngIf="show" [ngStyle]="styles"></div>"#,
        r#"<ng-template [ngIf]="show" [ngStyle]="styles"><span></span></ng-template>"#,
        r#"@for (item of items; track item) { <span [ngStyle]="item.styles"></span> }"#,
    ];

    Tester::new(PreferStyleBinding::NAME, PreferStyleBinding::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<div [style.color]=\"c\"></div>' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<div [ngStyle]=\"styles\"></div>' }) class A {}",
    ];
    Tester::new(PreferStyleBinding::NAME, PreferStyleBinding::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
