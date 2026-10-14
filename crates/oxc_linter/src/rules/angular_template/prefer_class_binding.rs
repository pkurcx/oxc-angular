use oxc_angular_compiler::ast::{
    expression::{AngularExpression, LiteralMapKey},
    r3::{R3BoundAttribute, R3Element, R3Template, R3Visitor, visit_all},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{AngularTemplate, visit_element_in_eslint_order, visit_template_in_eslint_order},
};

fn prefer_class_binding_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Consider using [class] bindings instead of [ngClass] where applicable.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferClassBinding;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Suggests `[class]` and `[class.name]` bindings over `[ngClass]`. An `[ngClass]` object
    /// with a key holding several space-separated classes is allowed, since `[class]` does not
    /// support it.
    ///
    /// ### Why is this bad?
    ///
    /// Class bindings are simpler and faster than the `NgClass` directive. `NgClass` is still
    /// needed for space-separated class names in one key, or to pick up mutations of the
    /// bound object.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div [ngClass]="{ active: isActive }"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div [class.active]="isActive"></div>
    /// <div [ngClass]="{ 'btn btn-primary': isPrimary }"></div>
    /// ```
    PreferClassBinding,
    angular_template,
    style,
    version = "next",
    short_description = "Suggests using [class] bindings over ngClass where applicable",
);

impl Rule for PreferClassBinding {
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
        if attr.name == "ngClass" && !requires_ng_class(attr) {
            let span = self.template.span(attr.source_span);
            self.ctx.diagnostic(prefer_class_binding_diagnostic(span));
        }
    }
}

/// Whether an object key holds space-separated class names, which `[class]` does not support.
fn requires_ng_class(attr: &R3BoundAttribute) -> bool {
    let AngularExpression::LiteralMap(map) = &attr.value else { return false };
    attr.value_span.is_some()
        && map.keys.iter().any(|key| match key {
            LiteralMapKey::Property(property) => property.key.trim().contains(char::is_whitespace),
            LiteralMapKey::Spread(_) => false,
        })
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<div [class.active]="isActive"></div>"#,
        r#"<div class="static-class"></div>"#,
        r#"<div class="static" [class.active]="isActive"></div>"#,
        r#"<div [class.active]="isActive" [class.disabled]="isDisabled"></div>"#,
        r#"<button [class.active]="isActive && !isDisabled">Click</button>"#,
        r#"<input type="text" [class.error]="hasError" [value]="inputValue">"#,
        r#"<div [ngClass]="{'btn btn-primary': isPrimary}"></div>"#,
        r#"<div [ngClass]="{'alert alert-danger': hasError, 'alert-success': !hasError}"></div>"#,
        r#"<button [ngClass]="{'btn btn-lg btn-block': isLarge}">Submit</button>"#,
        r#"<div [ngClass]="{'btn btn-primary': isPrimary, 'active': isActive}"></div>"#,
        r#"<div *ngIf="show" [ngClass]="{'btn btn-primary': isPrimary}"></div>"#,
    ];

    let fail = vec![
        r#"<div [ngClass]="className"></div>"#,
        r#"<div [ngClass]="getClasses()"></div>"#,
        r#"<button [ngClass]="isActive ? 'active' : 'inactive'"></button>"#,
        r#"<div class="static" [ngClass]="dynamicClass"></div>"#,
        r#"<input type="text" [ngClass]="inputClass" [value]="text">"#,
        r#"<div [ngClass]="['class1', 'class2']"></div>"#,
        r#"<div [ngClass]="{ active: true, disabled: false }"></div>"#,
        r#"<div [ngClass]="{ ...base, 'a': x }"></div>"#,
        // Reported once, on the element rather than on the structural directive's template.
        r#"<div *ngIf="show" [ngClass]="classes"></div>"#,
        r#"<ng-template [ngIf]="show" [ngClass]="classes"><span></span></ng-template>"#,
        r#"@if (show) { <span bind-ngClass="classes"></span> }"#,
    ];

    Tester::new(PreferClassBinding::NAME, PreferClassBinding::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<div [class.a]=\"b\"></div>' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<div [ngClass]=\"classes\"></div>' }) class A {}",
    ];
    Tester::new(PreferClassBinding::NAME, PreferClassBinding::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
