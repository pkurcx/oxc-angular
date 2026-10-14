use oxc_angular_compiler::ast::r3::{
    R3BoundAttribute, R3Element, R3Template, R3TemplateAttr, R3Visitor, visit_all,
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::AngularTemplate,
};

fn use_track_by_function_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Missing trackBy function in ngFor directive").with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
pub struct UseTrackByFunction(Box<UseTrackByFunctionConfig>);

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct UseTrackByFunctionConfig {
    /// Input names accepted in place of `ngForTrackBy`, such as `ngForTrackById` for a
    /// directive that adds a `trackById` key to `*ngFor`.
    alias: Vec<String>,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that `*ngFor` and `<ng-template ngFor [ngForOf]>` set a `trackBy` function.
    ///
    /// ### Why is this bad?
    ///
    /// Without `trackBy`, `ngFor` tracks items by identity, so replacing the array (sorting,
    /// filtering, fetching fresh data) destroys and recreates every row, losing component state,
    /// focus and animations.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <li *ngFor="let item of items">{{ item }}</li>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <li *ngFor="let item of items; trackBy: trackById">{{ item }}</li>
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - An element with both a structural directive and a `[ngForOf]` input is reported once, not
    ///   twice.
    UseTrackByFunction,
    angular_template,
    perf,
    config = UseTrackByFunction,
    version = "next",
    short_description = "Ensures trackBy function is used",
);

impl Rule for UseTrackByFunction {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { rule: self, ctx, template }, template.nodes);
        }
    }
}

impl UseTrackByFunction {
    fn is_track_by(&self, name: &str) -> bool {
        name == "ngForTrackBy" || self.0.alias.iter().any(|alias| alias == name)
    }
}

struct Visitor<'c, 'a> {
    rule: &'c UseTrackByFunction,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
}

impl Visitor<'_, '_> {
    fn check_inputs(&self, inputs: &[R3BoundAttribute]) {
        if inputs.iter().any(|input| self.rule.is_track_by(&input.name)) {
            return;
        }
        for input in inputs.iter().filter(|input| input.name.as_str() == "ngForOf") {
            let span = self.template.span(input.source_span);
            self.ctx.diagnostic(use_track_by_function_diagnostic(span));
        }
    }
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        self.check_inputs(&element.inputs);
        self.visit_element_children(element);
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        let attrs = &template.template_attrs;
        if let (Some(first), Some(last)) = (attrs.first(), attrs.last())
            && !attrs.iter().any(|attr| self.rule.is_track_by(attr_name(attr)))
        {
            // From the `*` before the first binding to the quote after the last one.
            let span = self.template.span(Span::new(attr_span(first).start, attr_span(last).end));
            #[expect(clippy::cast_possible_truncation)]
            let source_end = self.ctx.source_text().len() as u32;
            let span = Span::new(span.start.saturating_sub(1), (span.end + 1).min(source_end));
            let ng_for_of = |attr: &&R3TemplateAttr| matches!(attr, R3TemplateAttr::Bound(attr) if attr.name.as_str() == "ngForOf");
            for _ in attrs.iter().filter(ng_for_of) {
                self.ctx.diagnostic(use_track_by_function_diagnostic(span));
            }
        }
        // A structural directive's template holds copies of the wrapped element's inputs.
        if attrs.is_empty() {
            self.check_inputs(&template.inputs);
        }
        self.visit_template_children(template);
    }
}

fn attr_name<'n>(attr: &'n R3TemplateAttr) -> &'n str {
    match attr {
        R3TemplateAttr::Bound(attr) => attr.name.as_str(),
        R3TemplateAttr::Text(attr) => attr.name.as_str(),
    }
}

fn attr_span(attr: &R3TemplateAttr) -> Span {
    match attr {
        R3TemplateAttr::Bound(attr) => attr.source_span,
        R3TemplateAttr::Text(attr) => attr.source_span,
    }
}

#[test]
fn test() {
    use crate::tester::Tester;
    use serde_json::json;

    let pass = vec![
        (
            r#"
              <div *ngFor="let item of [1, 2, 3]; trackBy: trackByFn">
                {{ item }}
              </div>
            "#,
            None,
        ),
        (
            r#"
              <div *ngFor="let item of [1, 2, 3]; let i = index; trackBy: trackByFn">
                {{ item }}
              </div>
            "#,
            None,
        ),
        (
            r#"
              <div *ngFor="let item of [1, 2, 3]; trackBy : trackByFn">
                {{ item }}
              </div>
            "#,
            None,
        ),
        (
            "
              <div *ngFor='let item of [1, 2, 3]; let i = index; trackBy: trackByFn'>
                {{ item }}
              </div>
            ",
            None,
        ),
        (
            r#"
              <div *ngFor  =  "let item of [1, 2, 3]; let i = index; trackBy : trackByFn">
                {{ item }}
              </div>
            "#,
            None,
        ),
        (
            r#"
              <ng-template ngFor let-item [ngForOf]="[1, 2, 3]" let-i="index"
                [ngForTrackBy]="trackByFn">
                {{ item }}
              </ng-template>
            "#,
            None,
        ),
        (
            r#"
              <div *ngFor="let item of ['a', 'b', 'c']; index as i; trackBy: trackByFn">
                {{ item }}
              </div>

              <ng-template ngFor let-item [ngForOf]="[1, 2, 3]" let-i="index"
                [ngForTrackBy]="trackByFn">
                {{ item }}
              </ng-template>
            "#,
            None,
        ),
        (
            r#"
              <div *ngFor="
                let item of [1, 2, 3];
                let i = index;
                trackBy : trackByFn
              ">
            "#,
            None,
        ),
        (
            r#"
                <div *ngFor="
                  let item of [1, 2, 3];
                  let i = index;
                  trackByProperty: 'id'
                ">
              "#,
            Some(json!([{ "alias": ["ngForTrackByProperty"] }])),
        ),
        (
            r#"
              <div *ngFor="let photo of photos; trackById"></div>
            "#,
            Some(json!([{ "alias": ["ngForTrackById"] }])),
        ),
        (
            r#"
              <ng-template ngFor let-item [ngForOf]="[1, 2, 3]" let-i="index" [ngForTrackByProperty]="trackByFn">
                {{ item }}
              </ng-template>
            "#,
            Some(json!([{ "alias": ["ngForTrackByProperty"] }])),
        ),
        (r#"<div *ngIf="show"><li *ngFor="let x of xs; trackBy: id"></li></div>"#, None),
        (r#"<li *ngIf="show" [ngForOf]="xs" [ngForTrackBy]="id"></li>"#, None),
    ];

    let fail = vec![
        (
            r#"
              <ul>
                <li *ngFor="let item of [1, 2, 3];">
                  {{ item }}
                </li>
              </ul>
            "#,
            None,
        ),
        (
            r#"
              <ul>
                <li *ngFor="let item of [1, 2, 3];">
                  {{ item }}
                </li>
              </ul>
            "#,
            Some(json!([{ "alias": ["ngForTrackByProperty"] }])),
        ),
        (
            r#"
              <ng-template ngFor let-item [ngForOf]="[1, 2, 3]" let-i="index">
                {{ item }}
              </ng-template>
            "#,
            None,
        ),
        (
            r#"
              <ng-template ngFor let-item [ngForOf]="[1, 2, 3]" let-i="index">
                {{ item }}
              </ng-template>
            "#,
            Some(json!([{ "alias": ["ngForTrackByProperty"] }])),
        ),
        (
            r#"
              <div *ngFor="let item of [1, 2, 3]; trackBy: trackByFn">
                {{ item }}
              </div>
              <ul>
                <li *ngFor="let item of [1, 2, 3];">
                  {{ item }}
                </li>
              </ul>
            "#,
            None,
        ),
        (
            r#"
              <div *ngFor="let item of [1, 2, 3];">
                {{ item }}
              </div>
              <ng-template ngFor let-item [ngForOf]="[1, 2, 3]" let-i="index">
                {{ item }}
              </ng-template>
            "#,
            None,
        ),
        (
            r#"
              <div *ngFor="let item of [1, 2, 3]; trackBy: trackByFn">
                {{ item }}
              </div>
              <div *ngFor="let item of [1, 2, 3]; trackByProperty: trackByFn">
                {{ item }}
              </div>
              <ul>
                <li *ngFor="let item of [1, 2, 3];">
                  {{ item }}
                </li>
              </ul>
            "#,
            Some(json!([{ "alias": ["ngForTrackByProperty"] }])),
        ),
        // The wrapper template of `*ngIf` copies the element's inputs: reported once.
        (r#"<li *ngIf="show" [ngForOf]="xs"></li>"#, None),
        (r#"<div *ngIf="show"><li *ngFor="let x of xs"></li></div>"#, None),
    ];

    Tester::new(UseTrackByFunction::NAME, UseTrackByFunction::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<li *ngFor=\"let x of xs; trackBy: id\"></li>' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: `\n  <li *ngFor=\"let x of xs\"></li>\n` }) class A {}",
    ];
    Tester::new(UseTrackByFunction::NAME, UseTrackByFunction::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
