use oxc_angular_compiler::ast::r3::{
    R3ForLoopBlock, R3ForLoopBlockEmpty, R3IfBlockBranch, R3Node, R3SwitchBlock, R3Visitor,
    visit_all,
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{context::LintContext, rule::Rule, utils::AngularTemplate};

fn no_empty_control_flow_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Unexpected empty control flow block.").with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoEmptyControlFlow;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that control flow blocks (`@if`, `@else`, `@for`, `@empty`, `@switch`, `@case`,
    /// `@default`) are not empty or whitespace only.
    ///
    /// ### Why is this bad?
    ///
    /// Empty control flow blocks usually come from a refactoring that was not completed. They
    /// add nothing and leave the reader wondering whether the empty branch is intentional.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// @if (a) {} @else { b }
    /// @for (item of items; track item.id) { {{ item }} } @empty {}
    /// @switch (value) {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// @if (a) { a } @else { b }
    /// @for (item of items; track item.id) { {{ item }} } @empty { None }
    /// ```
    NoEmptyControlFlow,
    angular_template,
    suspicious,
    version = "next",
    short_description = "Ensures that control flow blocks are not empty.",
);

impl Rule for NoEmptyControlFlow {
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

impl Visitor<'_, '_> {
    /// Whether the block content between `start` and `end` (template offsets) is blank. Comments
    /// are not template nodes, so they make `children` empty without making the content blank.
    fn is_empty(&self, children: &[R3Node], start: u32, end: Option<u32>) -> bool {
        children.is_empty()
            || end.is_some_and(|end| {
                let content = self.template.span(Span::new(start, end));
                self.ctx
                    .source_text()
                    .get(content.start as usize..content.end as usize)
                    .is_some_and(|text| text.trim().is_empty())
            })
    }

    fn report(&self, name_span: Span) {
        let span = self.template.span(name_span);
        self.ctx.diagnostic(no_empty_control_flow_diagnostic(span));
    }
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_if_block_branch(&mut self, branch: &R3IfBlockBranch<'a>) {
        let end = branch.end_source_span.map(|span| span.start);
        if self.is_empty(&branch.children, branch.start_source_span.end, end) {
            self.report(branch.name_span);
        }
        visit_all(self, &branch.children);
    }

    fn visit_for_loop_block(&mut self, block: &R3ForLoopBlock<'a>) {
        // `main_block_span` ends after the `}` closing the loop body, before any `@empty`.
        let end = block.main_block_span.end.checked_sub(1);
        if self.is_empty(&block.children, block.start_source_span.end, end) {
            self.report(block.name_span);
        }
        visit_all(self, &block.children);
        if let Some(empty) = &block.empty {
            self.visit_for_loop_block_empty(empty);
        }
    }

    fn visit_for_loop_block_empty(&mut self, block: &R3ForLoopBlockEmpty<'a>) {
        let end = block.end_source_span.map(|span| span.start);
        if self.is_empty(&block.children, block.start_source_span.end, end) {
            self.report(block.name_span);
        }
        visit_all(self, &block.children);
    }

    fn visit_switch_block(&mut self, block: &R3SwitchBlock<'a>) {
        if block.groups.is_empty() {
            self.report(block.name_span);
            return;
        }
        for group in &block.groups {
            let end = group.end_source_span.map(|span| span.start);
            if self.is_empty(&group.children, group.start_source_span.end, end)
                && let Some(case) = group.cases.first()
            {
                self.report(case.name_span);
            }
            visit_all(self, &group.children);
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "@if (a) {
            1
          } @else if (b) {
            2
          } @else {
            3
          }",
        "@if (a) {
            <!-- a -->
          } @else if (b) {
            <!-- b -->
          } @else {
            <!-- c -->
          }",
        "@if (a) {
             &nbsp;
          } @else if (b) {
             &nbsp;
          } @else {
             &nbsp;
          }",
        "@for (item of items; track item.id) {
            1
          } @empty {
            2
          }",
        "@for (item of items; track item.id) {
            <!-- a -->
          } @empty {
            <!-- b -->
          }",
        "@for (item of items; track item.id) {
            &nbsp;
          } @empty {
            &nbsp;
          }",
        "@switch (condition) {
            @case (a) {
              1
            }
            @default {
              2
            }
          }",
        "@switch (condition) {
            @case (a) {
              <!-- a -->
            }
            @default {
              <!-- b -->
            }
          }",
        "@switch (condition) {
            @case (a) {
              &nbsp;
            }
            @default {
              &nbsp;
            }
          }",
        "@switch (condition) { @case (a) @case (b) { 1 } @default { 2 } }",
        r#"<div *ngIf="x">@if (a) { 1 }</div>"#,
        "<ng-template>@if (a) { 1 }</ng-template>",
    ];

    let fail = vec![
        // @if, @else.
        "
          @if (a) {} @else if (b) {
            2
          } @else {
            3
          }",
        "
          @if (a) {
          } @else if (b) {
            2
          } @else {
            3
          }",
        "
          @if (a) {
            1
          } @else if (b) {} @else {
            3
          }",
        "
          @if (a) {
            1
          } @else if (b) {
          } @else {
            3
          }",
        "
          @if (a) {
            1
          } @else if (b) {
            2
          } @else {}
          ",
        "
          @if (a) {
            1
          } @else if (b) {
            2
          } @else {
          }
          ",
        // @for, @empty.
        "
          @for (item of items; track item.id) {} @empty {
            2
          }",
        "
          @for (item of items; track item.id) {
          } @empty {
            2
          }",
        "
          @for (item of items; track item.id) {
            1
          } @empty {}
          ",
        "
          @for (item of items; track item.id) {
            1
          } @empty {
          }",
        // @switch, @case.
        "
          @switch (condition) {}
          ",
        "
          @switch (condition) {
          }",
        "
          @switch (condition) {
            @case (a) {}
            @default {
              2
            }
          }",
        "
          @switch (condition) {
            @case (a) {
            }
            @default {
              2
            }
          }",
        "
          @switch (condition) {
            @case (a) {
              1
            }
            @default {}
          }",
        "
          @switch (condition) {
            @case (a) {
              1
            }
            @default {
            }
          }",
        "@switch (condition) { @case (a) @case (b) { } @default { 2 } }",
        "@if (a) {<!-- only a comment -->}",
        r#"<div *ngIf="x">@if (a) {}</div>"#,
        "<ng-template>@for (item of items; track item) { }</ng-template>",
        "@defer { @if (a) { } }",
    ];

    Tester::new(NoEmptyControlFlow::NAME, NoEmptyControlFlow::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '@if (a) { a }' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: `\n  @if (a) {\n  } @else { b }\n` }) class A {}",
    ];
    Tester::new(NoEmptyControlFlow::NAME, NoEmptyControlFlow::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
