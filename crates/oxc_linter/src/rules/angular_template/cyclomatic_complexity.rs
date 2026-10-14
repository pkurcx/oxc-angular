use oxc_angular_compiler::ast::r3::{
    R3BoundAttribute, R3Element, R3ForLoopBlock, R3IfBlock, R3SwitchBlock, R3SwitchBlockCase,
    R3Template, R3TextAttribute, R3Visitor, visit_all,
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{AngularTemplate, visit_element_in_eslint_order, visit_template_in_eslint_order},
};

fn cyclomatic_complexity_diagnostic(total: u32, max: u32, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The cyclomatic complexity {total} exceeds the defined limit {max}"
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum CyclomaticComplexityVariant {
    /// Counts `@if`, `@for`, each `@case` and `@default`, and `ngIf`, `ngForOf`,
    /// `ngSwitchCase` and `ngSwitchDefault`.
    #[default]
    Classic,
    /// Counts each `@switch` and `ngSwitch` once instead of each of its cases.
    Modified,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct CyclomaticComplexity {
    /// The maximum number of branches in a template.
    max_complexity: u32,
    /// How switches are counted.
    variant: CyclomaticComplexityVariant,
}

impl Default for CyclomaticComplexity {
    fn default() -> Self {
        Self { max_complexity: 5, variant: CyclomaticComplexityVariant::Classic }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Limits the number of branches (control flow blocks and structural directives) in a
    /// template. Every branch beyond the limit is reported.
    ///
    /// ### Why is this bad?
    ///
    /// A template with many paths is hard to follow and test; split it into components.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule, with `{ "maxComplexity": 1 }`:
    /// ```html
    /// @if (a) { <p>a</p> }
    /// @for (item of items; track item) { <p>{{ item }}</p> }
    /// ```
    ///
    /// Examples of **correct** code for this rule, with `{ "maxComplexity": 1 }`:
    /// ```html
    /// @if (a) { <p>a</p> }
    /// ```
    CyclomaticComplexity,
    angular_template,
    pedantic,
    config = CyclomaticComplexity,
    version = "next",
    short_description = "Checks cyclomatic complexity against a specified limit.",
);

impl Rule for CyclomaticComplexity {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        // Each inline template is counted on its own, as `@angular-eslint` lints it as a file.
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { rule: self, ctx, template, total: 0 }, template.nodes);
        }
    }
}

struct Visitor<'c, 'a> {
    rule: &'c CyclomaticComplexity,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
    total: u32,
}

impl Visitor<'_, '_> {
    fn increment(&mut self, span: Span) {
        self.total += 1;
        let max = self.rule.max_complexity;
        if self.total > max {
            let span = self.template.span(span);
            self.ctx.diagnostic(cyclomatic_complexity_diagnostic(self.total, max, span));
        }
    }

    fn is_classic(&self) -> bool {
        self.rule.variant == CyclomaticComplexityVariant::Classic
    }
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        visit_element_in_eslint_order(self, element);
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        visit_template_in_eslint_order(self, template);
    }

    fn visit_bound_attribute(&mut self, attr: &R3BoundAttribute<'a>) {
        let switch = if self.is_classic() { "ngSwitchCase" } else { "ngSwitch" };
        if matches!(attr.name.as_str(), "ngForOf" | "ngIf") || attr.name == switch {
            self.increment(attr.source_span);
        }
    }

    fn visit_text_attribute(&mut self, attr: &R3TextAttribute<'a>) {
        if self.is_classic() && attr.name == "ngSwitchDefault" {
            self.increment(attr.source_span);
        }
    }

    fn visit_if_block(&mut self, block: &R3IfBlock<'a>) {
        self.increment(block.source_span);
        for branch in &block.branches {
            self.visit_if_block_branch(branch);
        }
    }

    fn visit_for_loop_block(&mut self, block: &R3ForLoopBlock<'a>) {
        self.increment(block.source_span);
        visit_all(self, &block.children);
        if let Some(empty) = &block.empty {
            self.visit_for_loop_block_empty(empty);
        }
    }

    fn visit_switch_block(&mut self, block: &R3SwitchBlock<'a>) {
        if !self.is_classic() {
            self.increment(block.source_span);
        }
        for group in &block.groups {
            self.visit_switch_block_case_group(group);
        }
    }

    fn visit_switch_block_case(&mut self, case: &R3SwitchBlockCase<'a>) {
        if self.is_classic() {
            self.increment(case.source_span);
        }
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        (r#"<div *ngIf="a"><p *ngFor="let x of xs">{{ x }}</p></div>"#, None),
        (
            r#"<div [ngSwitch]="m"><p *ngSwitchCase="1"></p><p *ngSwitchCase="2"></p><p *ngSwitchDefault></p></div>"#,
            Some(json!([{ "maxComplexity": 3 }])),
        ),
        (
            "@switch (m) { @case (1) { a } @case (2) { b } @default { c } }",
            Some(json!([{ "maxComplexity": 1, "variant": "modified" }])),
        ),
        ("@if (a) { x } @else if (b) { y } @else { z }", Some(json!([{ "maxComplexity": 1 }]))),
    ];

    let fail = vec![
        (
            r#"<div *ngIf="a"><p *ngFor="let x of xs"><span *ngIf="x"></span></p></div>"#,
            Some(json!([{ "maxComplexity": 2 }])),
        ),
        (
            r#"<div [ngSwitch]="m"><p *ngSwitchCase="1"></p><p *ngSwitchCase="2"></p><p *ngSwitchDefault></p></div>"#,
            Some(json!([{ "maxComplexity": 2 }])),
        ),
        (
            "@switch (m) { @case (1) { a } @case (2) { b } @default { c } }",
            Some(json!([{ "maxComplexity": 2 }])),
        ),
        (
            r#"<div [ngSwitch]="m"></div> @switch (m) { @case (1) { a } }"#,
            Some(json!([{ "maxComplexity": 1, "variant": "modified" }])),
        ),
        (
            "@if (a) { @for (x of xs; track x) { @if (x) { y } } @empty { } }",
            Some(json!([{ "maxComplexity": 1 }])),
        ),
    ];

    Tester::new(CyclomaticComplexity::NAME, CyclomaticComplexity::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    // Each inline template is counted on its own.
    let two_components = "import { Component } from '@angular/core';
@Component({ template: '@if (a) { x }' }) class A {}
@Component({ template: '@if (b) { y }' }) class B {}";
    Tester::new(
        CyclomaticComplexity::NAME,
        CyclomaticComplexity::PLUGIN,
        vec![(two_components, Some(json!([{ "maxComplexity": 1 }])))],
        vec![],
    )
    .change_rule_path_extension("ts")
    .test();
}
