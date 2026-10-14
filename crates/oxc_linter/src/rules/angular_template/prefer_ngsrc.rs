use oxc_angular_compiler::ast::{
    expression::{AngularExpression, BinaryOperator, LiteralValue},
    r3::{R3BoundAttribute, R3Element, R3TextAttribute},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{AngularTemplate, angular_elements},
};

fn missing_attribute_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The attribute [ngSrc] should be used for img elements instead of [src].")
        .with_label(span)
}

fn invalid_double_source_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Only [ngSrc] should exist on an img element. Delete the [src] attribute.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferNgsrc;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures `ngSrc` is used instead of `src` for `<img>` elements.
    ///
    /// ### Why is this bad?
    ///
    /// Angular's `NgOptimizedImage` directive (`ngSrc`) lazy loads images, generates `srcset`,
    /// sets loading priority hints and prevents layout shift, which improves Largest Contentful
    /// Paint. A plain `src` gets none of this. `data:` URLs are allowed with `src`, since inline
    /// images do not benefit from `ngSrc`.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <img src="http://localhost">
    /// <img [src]="value">
    /// <img ngSrc="http://localhost" src="http://localhost">
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <img ngSrc="http://localhost">
    /// <img [ngSrc]="value">
    /// <img src="data:image/jpeg;base64">
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - `bind-src`, `[(src)]`, `[class.src]` and `[@src]` are reported without a suggestion, where
    ///   `@angular-eslint` suggests a no-op.
    PreferNgsrc,
    angular_template,
    style,
    suggestion,
    version = "next",
    short_description = "Ensures ngSrc is used instead of src for img elements",
);

#[derive(Clone, Copy)]
enum SrcAttribute<'e, 'a> {
    Bound(&'e R3BoundAttribute<'a>),
    Text(&'e R3TextAttribute<'a>),
}

impl SrcAttribute<'_, '_> {
    fn source_span(self) -> Span {
        match self {
            Self::Bound(attr) => attr.source_span,
            Self::Text(attr) => attr.source_span,
        }
    }

    // Angular rejects `ngSrc` with a data URL, so such a `src` is left alone.
    fn is_base64_image(self) -> bool {
        match self {
            Self::Text(attr) => attr.value.trim_start().starts_with("data:"),
            Self::Bound(attr) => {
                is_data_string(&attr.value)
                    || matches!(&attr.value, AngularExpression::Binary(binary)
                        if binary.operation == BinaryOperator::Add && is_data_string(&binary.left))
            }
        }
    }
}

fn is_data_string(expression: &AngularExpression) -> bool {
    matches!(expression, AngularExpression::LiteralPrimitive(literal)
        if matches!(&literal.value, LiteralValue::String(value) if value.trim_start().starts_with("data:")))
}

// Inputs first, as upstream searches `[...inputs, ...attributes]`. Angular drops the legacy
// animation `@` from an input's name; oxc keeps it for `[@x]`.
fn find_attribute<'e, 'a>(element: &'e R3Element<'a>, name: &str) -> Option<SrcAttribute<'e, 'a>> {
    element
        .inputs
        .iter()
        .find(|attr| attr.name.strip_prefix('@').unwrap_or(&attr.name) == name)
        .map(SrcAttribute::Bound)
        .or_else(|| {
            element.attributes.iter().find(|attr| attr.name == name).map(SrcAttribute::Text)
        })
}

impl Rule for PreferNgsrc {
    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            for element in angular_elements(template.nodes) {
                if element.name.eq_ignore_ascii_case("img") {
                    check_img(ctx, template, element);
                }
            }
        }
    }
}

fn check_img<'a>(ctx: &LintContext<'a>, template: &AngularTemplate<'a>, element: &R3Element<'a>) {
    let Some(src) = find_attribute(element, "src") else { return };
    let has_ng_src = find_attribute(element, "ngSrc").is_some();
    if !has_ng_src && src.is_base64_image() {
        return;
    }
    let span = template.span(src.source_span());
    if span.start > span.end {
        return;
    }
    if has_ng_src {
        let start = ctx.source_range(Span::new(0, span.start)).trim_end().len();
        #[expect(clippy::cast_possible_truncation)]
        let removed = Span::new(start as u32, span.end);
        ctx.diagnostic_with_suggestion(invalid_double_source_diagnostic(span), |fixer| {
            fixer.delete_range(removed).with_message("Remove the [src] attribute")
        });
        return;
    }
    let text = ctx.source_range(span);
    let renamed = ["[attr.src]", "[src]"]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix).map(|rest| format!("[ngSrc]{rest}")))
        .or_else(|| text.strip_prefix("src").map(|rest| format!("ngSrc{rest}")));
    match renamed {
        Some(renamed) => {
            ctx.diagnostic_with_suggestion(missing_attribute_diagnostic(span), |fixer| {
                fixer.replace(span, renamed).with_message("Replace [src] with [ngSrc]")
            });
        }
        // `bind-src`, `[(src)]`, `[@src]`: upstream's suggestion leaves these unchanged.
        None => ctx.diagnostic(missing_attribute_diagnostic(span)),
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<img alt="nothing">"#,
        r#"<img ngSrc="http://localhost">"#,
        r#"<img [ngSrc]="'http://localhost'">"#,
        r#"<img [ngSrc]="value">"#,
        r#"<IMG [ngSrc]="value">"#,
        r#"<img src="data:image/jpeg;base64">"#,
        r#"<img [src]="'data:image/jpeg;base64'">"#,
        r#"<img [src]="'data:' + value">"#,
        r#"<img [attr.src]="'data:image/jpeg;base64'">"#,
        r#"<img [attr.src]="'data:' + value">"#,
        r#"<app-img src="http://localhost"></app-img>"#,
        r#"<img *ngIf="show" [ngSrc]="value">"#,
    ];

    let fail = vec![
        r#"<img src="http://localhost">"#,
        r#"<img src="http://src.com">"#,
        r#"<img [src]="'http://localhost'">"#,
        r#"<img [src]="value">"#,
        r#"<img [attr.src]="value">"#,
        r#"<img [attr.src]="'http://localhost'">"#,
        r#"<img [src]="'http://' + value">"#,
        r#"<IMG [src]="value">"#,
        r#"<img ngSrc="http://localhost" src="http://localhost">"#,
        r#"<img ngSrc="http://src.com" src="http://src.com">"#,
        r#"<img ngSrc="http://localhost" [src]="'http://localhost'">"#,
        r#"<img ngSrc="http://localhost" [src]="value">"#,
        r#"<img [ngSrc]="otherValue" src="http://localhost">"#,
        r#"<img [ngSrc]="otherValue" [src]="'http://localhost'">"#,
        r#"<img [ngSrc]="otherValue" [src]="value">"#,
        r#"<img [src]="otherValue" [ngSrc]="value">"#,
        r#"<img src="data:image/png;base64" [ngSrc]="otherValue">"#,
        r#"<img [attr.src]="'data:image/png;base64'" [ngSrc]="otherValue">"#,
        r#"<img [src]="'data:image/png;base64'" [ngSrc]="otherValue">"#,
        r#"<img [src]="'data:' + value" [ngSrc]="otherValue">"#,
        "<img\n  src=\"http://localhost\">",
        "<img\n  [src]=\"value\"\n  alt=\"test\">",
        "<img\n  [attr.src]=\"value\"\n  width=\"100\">",
        "<img\n  ngSrc=\"http://localhost\"\n  src=\"http://localhost\">",
        "<img\n  [ngSrc]=\"otherValue\"\n  [src]=\"value\"\n  alt=\"test\">",
        "<img alt=\"test\"\n  [src]=\"value\"\n>",
        r#"<img *ngIf="show" src="http://localhost">"#,
        r#"<ng-template><img [src]="value"></ng-template>"#,
        r#"<img bind-src="value">"#,
        r#"<img [src]="('data:x')">"#,
        r#"<img [@src]="value">"#,
    ];

    let fix = vec![
        (r#"<img src="http://localhost">"#, r#"<img ngSrc="http://localhost">"#),
        (r#"<img src="http://src.com">"#, r#"<img ngSrc="http://src.com">"#),
        (r#"<img [src]="'http://localhost'">"#, r#"<img [ngSrc]="'http://localhost'">"#),
        (r#"<img [src]="value">"#, r#"<img [ngSrc]="value">"#),
        (r#"<img [attr.src]="value">"#, r#"<img [ngSrc]="value">"#),
        (r#"<img [attr.src]="'http://localhost'">"#, r#"<img [ngSrc]="'http://localhost'">"#),
        (r#"<img [src]="'http://' + value">"#, r#"<img [ngSrc]="'http://' + value">"#),
        (r#"<IMG [src]="value">"#, r#"<IMG [ngSrc]="value">"#),
        (
            r#"<img ngSrc="http://localhost" src="http://localhost">"#,
            r#"<img ngSrc="http://localhost">"#,
        ),
        (r#"<img ngSrc="http://src.com" src="http://src.com">"#, r#"<img ngSrc="http://src.com">"#),
        (
            r#"<img ngSrc="http://localhost" [src]="'http://localhost'">"#,
            r#"<img ngSrc="http://localhost">"#,
        ),
        (r#"<img ngSrc="http://localhost" [src]="value">"#, r#"<img ngSrc="http://localhost">"#),
        (r#"<img [ngSrc]="otherValue" src="http://localhost">"#, r#"<img [ngSrc]="otherValue">"#),
        (
            r#"<img [ngSrc]="otherValue" [src]="'http://localhost'">"#,
            r#"<img [ngSrc]="otherValue">"#,
        ),
        (r#"<img [ngSrc]="otherValue" [src]="value">"#, r#"<img [ngSrc]="otherValue">"#),
        (r#"<img [src]="otherValue" [ngSrc]="value">"#, r#"<img [ngSrc]="value">"#),
        (
            r#"<img src="data:image/png;base64" [ngSrc]="otherValue">"#,
            r#"<img [ngSrc]="otherValue">"#,
        ),
        (
            r#"<img [attr.src]="'data:image/png;base64'" [ngSrc]="otherValue">"#,
            r#"<img [ngSrc]="otherValue">"#,
        ),
        (
            r#"<img [src]="'data:image/png;base64'" [ngSrc]="otherValue">"#,
            r#"<img [ngSrc]="otherValue">"#,
        ),
        (r#"<img [src]="'data:' + value" [ngSrc]="otherValue">"#, r#"<img [ngSrc]="otherValue">"#),
        ("<img\n  src=\"http://localhost\">", "<img\n  ngSrc=\"http://localhost\">"),
        ("<img\n  [src]=\"value\"\n  alt=\"test\">", "<img\n  [ngSrc]=\"value\"\n  alt=\"test\">"),
        (
            "<img\n  [attr.src]=\"value\"\n  width=\"100\">",
            "<img\n  [ngSrc]=\"value\"\n  width=\"100\">",
        ),
        (
            "<img\n  ngSrc=\"http://localhost\"\n  src=\"http://localhost\">",
            "<img\n  ngSrc=\"http://localhost\">",
        ),
        (
            "<img\n  [ngSrc]=\"otherValue\"\n  [src]=\"value\"\n  alt=\"test\">",
            "<img\n  [ngSrc]=\"otherValue\"\n  alt=\"test\">",
        ),
        ("<img alt=\"test\"\n  [src]=\"value\"\n>", "<img alt=\"test\"\n  [ngSrc]=\"value\"\n>"),
        (
            r#"<img *ngIf="show" src="http://localhost">"#,
            r#"<img *ngIf="show" ngSrc="http://localhost">"#,
        ),
    ];

    Tester::new(PreferNgsrc::NAME, PreferNgsrc::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();

    let inline_pass = vec![
        r#"import { Component } from '@angular/core'; @Component({ template: '<img [ngSrc]="value">' }) class A {}"#,
    ];
    let inline_fail = vec![
        r#"import { Component } from '@angular/core'; @Component({ template: '<img [src]="value">' }) class A {}"#,
    ];
    let inline_fix = vec![
        (
            r#"import { Component } from '@angular/core'; @Component({ template: '<img [src]="value">' }) class A {}"#,
            r#"import { Component } from '@angular/core'; @Component({ template: '<img [ngSrc]="value">' }) class A {}"#,
        ),
        (
            r#"import { Component } from '@angular/core'; @Component({ template: `<img ngSrc="a"  src="b">` }) class A {}"#,
            r#"import { Component } from '@angular/core'; @Component({ template: `<img ngSrc="a">` }) class A {}"#,
        ),
    ];
    Tester::new(PreferNgsrc::NAME, PreferNgsrc::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .expect_fix(inline_fix)
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
