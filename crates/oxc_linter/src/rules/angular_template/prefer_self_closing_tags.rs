use cow_utils::CowUtils;
use oxc_angular_compiler::ast::r3::{
    R3Content, R3Element, R3Node, R3Template, R3Visitor, visit_all,
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{
        ANGULAR_DOM_ELEMENTS, AngularTemplate, visit_element_in_eslint_order,
        visit_template_in_eslint_order,
    },
};

fn prefer_self_closing_tags_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Use self-closing tags for elements with a closing tag but no content.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferSelfClosingTags;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that self-closing tags are used for components, `<ng-template>` and
    /// `<ng-content>` that have a closing tag but no content. `index.html` is skipped.
    ///
    /// ### Why is this bad?
    ///
    /// An empty closing tag is noise; Angular supports self-closing tags for these elements.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <app-avatar [user]="user"></app-avatar>
    /// <ng-content></ng-content>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <app-avatar [user]="user" />
    /// <ng-content />
    /// ```
    PreferSelfClosingTags,
    angular_template,
    style,
    fix,
    version = "next",
    short_description = "Ensures that self-closing tags are used for elements with a closing tag but no content.",
);

impl Rule for PreferSelfClosingTags {
    fn run_once(&self, ctx: &LintContext) {
        if ctx.file_path().file_name().is_some_and(|name| name == "index.html") {
            return;
        }
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { ctx, template }, template.nodes);
        }
    }
}

struct Visitor<'c, 'a> {
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
}

impl<'a> Visitor<'_, 'a> {
    fn check(&self, children: &[R3Node<'a>], start: Span, end: Option<Span>) {
        let Some(end) = end.filter(|end| *end != start) else { return };
        let start = self.template.span(start);
        let end = self.template.span(end);
        let has_content = |span: Span| !self.ctx.source_range(span).trim().is_empty();
        let children_have_content = children.iter().any(|child| match child {
            R3Node::Text(text) => has_content(self.template.span(text.source_span)),
            _ => true,
        });
        if children_have_content || has_content(Span::new(start.end, end.start)) {
            return;
        }
        let before_close = self.ctx.source_range(Span::new(start.start, start.end - 1));
        let separator = if before_close.ends_with(char::is_whitespace) { "" } else { " " };
        self.ctx.diagnostic_with_fix(prefer_self_closing_tags_diagnostic(end), |fixer| {
            fixer.replace(Span::new(start.end - 1, end.end), format!("{separator}/>"))
        });
    }
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        let name = element.name.cow_to_ascii_lowercase();
        let is_native = name.starts_with(":svg:")
            || name.starts_with(":math:")
            || ANGULAR_DOM_ELEMENTS.contains(name.as_ref());
        if !is_native {
            self.check(&element.children, element.start_source_span, element.end_source_span);
        }
        visit_element_in_eslint_order(self, element);
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        self.check(&template.children, template.start_source_span, template.end_source_span);
        visit_template_in_eslint_order(self, template);
    }

    fn visit_content(&mut self, content: &R3Content<'a>) {
        self.check(&content.children, content.start_source_span, content.end_source_span);
        visit_all(self, &content.children);
    }
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::{TestCase, Tester};

    let mut pass: Vec<TestCase> = vec![
        "<app-root />",
        "<app-root>content</app-root>",
        "<app-root><!-- note --></app-root>",
        "<app-root><span></span></app-root>",
        "<div></div>",
        "<svg><path></path></svg>",
        "<ng-content />",
        r#"<ng-content select="x">fallback</ng-content>"#,
        "<ng-template />",
        r#"<div *ngIf="x"></div>"#,
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    pass.push(("<app-root></app-root>", None, None, Some(PathBuf::from("index.html"))).into());

    let fail: Vec<TestCase> = vec![
        "<app-root></app-root>",
        "<app-root>\n  \n</app-root>",
        r#"<app-root [a]="b" (c)="d()"></app-root>"#,
        "<ng-content></ng-content>",
        r#"<ng-content select="x"></ng-content>"#,
        "<ng-template></ng-template>",
        r#"<app-item *ngIf="x"></app-item>"#,
        // Not in `@angular-eslint`'s list of native elements.
        "<search></search>",
    ]
    .into_iter()
    .map(Into::into)
    .collect();

    let fix = vec![
        ("<app-root></app-root>", "<app-root />"),
        ("<app-root ></app-root>", "<app-root />"),
        (r#"<app-root [a]="b"></app-root>"#, r#"<app-root [a]="b" />"#),
        ("<ng-content></ng-content>", "<ng-content />"),
        ("<ng-template>\n</ng-template>", "<ng-template />"),
    ];

    Tester::new(PreferSelfClosingTags::NAME, PreferSelfClosingTags::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
