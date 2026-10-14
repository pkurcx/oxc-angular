use oxc_angular_compiler::ast::r3::{R3SwitchBlock, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{context::LintContext, rule::Rule, utils::AngularTemplate};

fn require_switch_default_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Switch should have a default block. Add `@default { ... }`, or `@default never` to enforce a compile-time exhaustive switch.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireSwitchDefault;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires a `@default` block on every `@switch`, or the `@default never;` exhaustive check.
    ///
    /// ### Why is this bad?
    ///
    /// A `@switch` without a `@default` silently renders nothing when the value matches none of
    /// the `@case` branches, which is rarely intended and easy to miss. The linter cannot tell
    /// whether the cases cover a union or enum; `@default never;` makes the template type-check
    /// fail when a case is missing.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// @switch (value) {
    ///   @case (1) { one }
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// @switch (value) {
    ///   @case (1) { one }
    ///   @default { other }
    /// }
    /// @switch (value) {
    ///   @case (1) { one }
    ///   @default never;
    /// }
    /// ```
    RequireSwitchDefault,
    angular_template,
    restriction,
    version = "next",
    short_description = "Requires a `@default` block on every `@switch` to ensure all cases are handled.",
);

impl Rule for RequireSwitchDefault {
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
    fn visit_switch_block(&mut self, block: &R3SwitchBlock<'a>) {
        let has_default =
            block.groups.iter().flat_map(|g| &g.cases).any(|c| c.expression.is_none());
        if block.exhaustive_check.is_none() && !has_default {
            let span = self.template.span(block.name_span);
            self.ctx.diagnostic(require_switch_default_diagnostic(span));
        }
        for group in &block.groups {
            self.visit_switch_block_case_group(group);
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "@switch (value) {
            @case (1) {
              one
            }
            @default {
              other
            }
          }",
        "@switch (value) {
            @default {
              other
            }
          }",
        "@switch (value) {
            @case (1) {
              one
            }
            @default never;
          }",
        "@switch (item.kind) {
            @case ('a') {
              a
            }
            @default never(item);
          }",
        "@switch (value) {
            @case (1) {}
            @case (2) {
              one or two
            }
            @default {
              other
            }
          }",
        "<div>{{ value }}</div>",
        "@switch (value) { @case (1) @case (2) { one or two } @default { other } }",
        r#"<div *ngIf="x">@switch (value) { @default { other } }</div>"#,
        "<ng-template>@switch (value) { @default { other } }</ng-template>",
    ];

    let fail = vec![
        "
          @switch (value) {
            @case (1) {
              one
            }
          }
        ",
        "
          @switch (value) {
            @case (1) {
              one
            }
            @case (2) {
              two
            }
          }
        ",
        "
          @switch (outer) {
            @default {
              @switch (inner) {
                @case (1) {
                  one
                }
              }
            }
          }
        ",
        r#"<div *ngIf="x">@switch (value) { @case (1) { one } }</div>"#,
        "<ng-template>@switch (value) { @case (1) { one } }</ng-template>",
        "@defer (when ready) { @switch (value) { @case (1) { one } } } @placeholder { x }",
    ];

    Tester::new(RequireSwitchDefault::NAME, RequireSwitchDefault::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '@switch (a) { @default { x } }' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: `\n  @switch (a) { @case (1) { x } }\n` }) class A {}",
    ];
    Tester::new(RequireSwitchDefault::NAME, RequireSwitchDefault::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
