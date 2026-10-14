use cow_utils::CowUtils;
use oxc_angular_compiler::ast::r3::{R3Element, R3Template, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{context::LintContext, rule::Rule, utils::AngularTemplate};

fn no_nested_tags_diagnostic(tag: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "<{tag}> elements must not be nested! This breaks angular incremental hydration as all browsers will convert \"<{tag}>1<{tag}>2</{tag}>3</{tag}>\" into \"<{tag}>1</{tag}><{tag}>2</{tag}>3\", creating a DOM mismatch between SSR and Angular"
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoNestedTags;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Denies nesting of `<p>` and `<a>` tags.
    ///
    /// ### Why is this bad?
    ///
    /// Nested `<p>` or `<a>` tags are invalid HTML. Browsers close the outer tag when they meet
    /// the inner one, turning `<p>1<p>2</p>3</p>` into `<p>1</p><p>2</p>3`, so the DOM no
    /// longer matches the server-rendered HTML Angular expects and hydration breaks. Tags inside
    /// an explicit `<ng-template>` are not rendered in place, so they are allowed.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <a><a></a></a>
    /// <p>@if (true) {<p></p>}</p>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <a></a><a></a>
    /// <p><ng-template><p>Text</p></ng-template></p>
    /// ```
    NoNestedTags,
    angular_template,
    correctness,
    version = "next",
    short_description = "Denies nesting of `<p>` and `<a>` tags.",
);

impl Rule for NoNestedTags {
    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { ctx, template, open_a: 0, open_p: 0 }, template.nodes);
        }
    }
}

struct Visitor<'c, 'a> {
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
    open_a: u32,
    open_p: u32,
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        let name = element.name.as_str();
        let (is_a, is_p) = (name.eq_ignore_ascii_case("a"), name.eq_ignore_ascii_case("p"));
        if (is_a && self.open_a > 0) || (is_p && self.open_p > 0) {
            let span = self.template.span(element.source_span);
            self.ctx.diagnostic(no_nested_tags_diagnostic(name, span));
        }
        self.open_a += u32::from(is_a);
        self.open_p += u32::from(is_p);
        self.visit_element_children(element);
        self.open_a -= u32::from(is_a);
        self.open_p -= u32::from(is_p);
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        let is_explicit = template.tag_name.is_some_and(|tag| {
            matches!(
                tag.as_str().cow_to_ascii_lowercase().as_ref(),
                "ng-template" | ":svg:ng-template"
            )
        });
        // An explicit `<ng-template>` is not rendered in place, so tags outside it are not
        // its ancestors in the DOM.
        let outer = (self.open_a, self.open_p);
        if is_explicit {
            (self.open_a, self.open_p) = (0, 0);
        }
        visit_all(self, &template.children);
        (self.open_a, self.open_p) = outer;
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "<a></a>",
        "<a></a><a></a>",
        "<A></A><A></A>",
        "<p></p>",
        "<p></p><p></p>",
        "
    <p>
      Text
      <ng-template #content>
        <p>Text</p>
      </ng-template>
    </p>
  ",
        "<a><ng-template><a></a></ng-template></a>",
        r#"<a><ng-template *ngIf="x"><a></a></ng-template></a>"#,
        r#"<a *ngIf="x"></a><a></a>"#,
        "<a><span></span></a><p><span></span></p>",
    ];

    let deep = format!("<a>{}\n  <a>fail</a>\n{}</a>", "<div>".repeat(20), "</div>".repeat(20));
    let fail = vec![
        "<a><a></a></a>",
        "<a><A></A></a>",
        "<p>@if(true) {<p></p>}</p>",
        r#"<a><a *ngFor="let item of items"></a></a>"#,
        "<p><ng-container><p></p></ng-container></p>",
        &deep,
        r#"<a *ngIf="x"><span><a></a></span></a>"#,
        "<ng-template><a><a></a></a></ng-template>",
        "<p>@defer {<p></p>} @placeholder {<p></p>}</p>",
    ];

    Tester::new(NoNestedTags::NAME, NoNestedTags::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<a></a><a></a>' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: `<a>\n  <a></a>\n</a>` }) class A {}",
    ];
    Tester::new(NoNestedTags::NAME, NoNestedTags::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
