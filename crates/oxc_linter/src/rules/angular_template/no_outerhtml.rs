use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{angular_binding_name, angular_elements, is_angular_dom_element},
};

fn no_outerhtml_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Do not use `outerHTML`. Binding to `[outerHTML]` throws on update because setting `outerHTML` replaces the host element itself, so Angular loses its reference to the node (\"This element has no parent node\"); a static `outerHTML` attribute is not a real DOM attribute and has no effect. Use `[innerHTML]` (being mindful of sanitization) or restructure the template instead.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoOuterhtml;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows using `outerHTML` on native elements, as a binding (`[outerHTML]`,
    /// `bind-outerHTML`, `[attr.outerHTML]`) or a static attribute.
    ///
    /// ### Why is this bad?
    ///
    /// Setting `outerHTML` replaces the element Angular is bound to, so the next update throws
    /// `This element has no parent node`. A static `outerHTML` attribute is not a DOM attribute
    /// and has no effect. Bindings to an `outerHTML` input of a component are allowed.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div [outerHTML]="html"></div>
    /// <div outerHTML="static"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div [innerHTML]="html"></div>
    /// <my-component [outerHTML]="html"></my-component>
    /// ```
    NoOuterhtml,
    angular_template,
    correctness,
    version = "next",
    short_description = "Disallows using `outerHTML` on elements, which breaks change detection and detaches the element from the Angular view",
);

impl Rule for NoOuterhtml {
    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            for element in angular_elements(template.nodes) {
                if !is_angular_dom_element(&element.name, false) {
                    continue;
                }
                let is_outer_html = |name: &str| name.eq_ignore_ascii_case("outerhtml");
                // Angular drops the legacy animation `@` from the name; oxc keeps it for `[@x]`.
                let inputs = element
                    .inputs
                    .iter()
                    .filter(|a| is_outer_html(a.name.strip_prefix('@').unwrap_or(&a.name)));
                for input in inputs {
                    // Upstream's key span leaves out the binding syntax and the legacy animation
                    // prefix: `outerHTML` in `[outerHTML]`, `bind-outerHTML` and `[@outerHTML]`.
                    let key = ctx.source_range(template.span(input.key_span));
                    let name = angular_binding_name(ctx, template, input.key_span);
                    let name = name
                        .strip_prefix('@')
                        .or_else(|| name.strip_prefix("animate-"))
                        .unwrap_or(name);
                    #[expect(clippy::cast_possible_truncation)]
                    let span = Span::sized(
                        input.key_span.start
                            + (name.as_ptr() as usize - key.as_ptr() as usize) as u32,
                        name.len() as u32,
                    );
                    ctx.diagnostic(no_outerhtml_diagnostic(template.span(span)));
                }
                for attr in element.attributes.iter().filter(|a| is_outer_html(&a.name)) {
                    let span = attr.key_span.unwrap_or(attr.source_span);
                    ctx.diagnostic(no_outerhtml_diagnostic(template.span(span)));
                }
            }
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<div [innerHTML]="html"></div>"#,
        r#"<my-component [outerHTML]="html"></my-component>"#,
        r#"<img [src]="url" />"#,
        r#"<div [class.active]="isActive"></div>"#,
        r#"<ng-template [outerHTML]="html"></ng-template>"#,
        r#"<div (outerHTML)="handle()"></div>"#,
    ];

    let fail = vec![
        r#"<div [outerHTML]="html"></div>"#,
        r#"<p bind-outerHTML="html">lib works!</p>"#,
        r#"<div outerHTML="static"></div>"#,
        "@if (condition) {\n  <span [outerHTML]=\"html\"></span>\n}",
        r#"<div [attr.outerHTML]="html"></div>"#,
        r#"<div *ngIf="show" [outerHTML]="html"></div>"#,
        r#"<ng-template><div [outerHTML]="html"></div></ng-template>"#,
        r#"<div [(outerHTML)]="html"></div>"#,
        r#"<div bindon-outerHTML="html"></div>"#,
        r#"<div [class.outerHTML]="on"></div>"#,
        r#"<div [@outerHTML]="x"></div>"#,
        r#"<div bind-animate-outerHTML="x"></div>"#,
        r#"<div outerHTML="{{x}}"></div>"#,
    ];

    Tester::new(NoOuterhtml::NAME, NoOuterhtml::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        r#"import { Component } from '@angular/core'; @Component({ template: '<div [innerHTML]="html"></div>' }) class A {}"#,
    ];
    let inline_fail = vec![
        r#"import { Component } from '@angular/core'; @Component({ template: '<div [outerHTML]="html"></div>' }) class A {}"#,
    ];
    Tester::new(NoOuterhtml::NAME, NoOuterhtml::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
