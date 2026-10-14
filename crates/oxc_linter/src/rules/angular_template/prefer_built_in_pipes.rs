use std::ptr;

use oxc_angular_compiler::ast::{
    expression::AngularExpression,
    r3::{R3BoundEvent, R3Node, R3Visitor, visit_all},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::for_each_expression,
};

fn prefer_built_in_pipes_diagnostic(method: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Avoid using method \"{method}\" in templates. Prefer the Angular built-in pipes instead (e.g. lowercase, uppercase, titlecase)."
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PreferBuiltInPipes(Box<PreferBuiltInPipesConfig>);

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct PreferBuiltInPipesConfig {
    /// Method names to disallow. Replaces the default list: `toLowerCase`, `toUpperCase`,
    /// `toLocaleLowerCase` and `toLocaleUpperCase`.
    disallow_list: Vec<String>,
    /// Whether to allow these method calls inside output event handlers.
    allow_in_output_handlers: bool,
}

impl Default for PreferBuiltInPipesConfig {
    fn default() -> Self {
        Self {
            disallow_list: ["toLowerCase", "toUpperCase", "toLocaleLowerCase", "toLocaleUpperCase"]
                .map(String::from)
                .to_vec(),
            allow_in_output_handlers: true,
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Encourages the use of Angular built-in pipes (e.g. `lowercase`, `uppercase`, `titlecase`)
    /// instead of certain JavaScript methods in templates.
    ///
    /// ### Why is this bad?
    ///
    /// A method call in a binding runs on every change detection cycle, while a pure pipe only
    /// transforms its input when it changes. Pipes also keep templates declarative.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// {{ name.toLowerCase() }}
    /// <div [title]="title.toUpperCase()"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// {{ name | lowercase }}
    /// <div [title]="title | uppercase"></div>
    /// ```
    PreferBuiltInPipes,
    angular_template,
    style,
    config = PreferBuiltInPipes,
    version = "next",
    short_description = "Encourages the use of Angular built-in pipes (e.g. lowercase, uppercase, titlecase) instead of certain JavaScript methods in Angular templates.",
);

impl Rule for PreferBuiltInPipes {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        let config = &self.0;
        for template in ctx.angular_templates() {
            let handlers = event_handlers(template.nodes);
            let mut in_event = false;
            for_each_expression(template.nodes, &mut |expression, parent| {
                if parent.is_none() {
                    in_event = handlers.iter().any(|handler| ptr::eq(*handler, expression));
                }
                let AngularExpression::Call(call) = expression else { return };
                let AngularExpression::PropertyRead(method) = &call.receiver else { return };
                if !config.disallow_list.iter().any(|name| name == method.name.as_str())
                    || (config.allow_in_output_handlers && in_event)
                {
                    return;
                }
                ctx.diagnostic(prefer_built_in_pipes_diagnostic(
                    method.name.as_str(),
                    template.expression_span(method.source_span),
                ));
            });
        }
    }
}

/// The root expressions of bound events, which `for_each_expression` walks with no parent.
fn event_handlers<'a>(nodes: &[R3Node<'a>]) -> Vec<*const AngularExpression<'a>> {
    struct Collector<'a>(Vec<*const AngularExpression<'a>>);
    impl<'a> R3Visitor<'a> for Collector<'a> {
        fn visit_bound_event(&mut self, event: &R3BoundEvent<'a>) {
            self.0.push(&raw const event.handler);
        }
    }
    let mut collector = Collector(vec![]);
    visit_all(&mut collector, nodes);
    collector.0
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        ("{{ name }}", None),
        ("{{ user.name }}", None),
        (r#"<button (click)="handleClick(name.toLowerCase())">Click</button>"#, None),
        (r#"<button (change)="onChange(value.toUpperCase())">Change</button>"#, None),
        ("{{ name | lowercase }}", None),
        ("{{ name | uppercase }}", None),
        ("{{ title | titlecase }}", None),
        ("{{ getValue() }}", None),
        ("{{ items.push(item) }}", None),
        (r#"{{ text.includes("test") }}"#, None),
        ("{{ name.toLowerCase() }}", Some(json!([{ "disallowList": ["toUpperCase"] }]))),
        (
            r#"<button (click)="handleClick()">Click</button>"#,
            Some(json!([{ "allowInOutputHandlers": false }])),
        ),
        // Only a plain property read is matched, as upstream.
        ("{{ name?.toLowerCase() }}", None),
        (r#"<button *ngIf="show" (click)="go(name.toLowerCase())">Go</button>"#, None),
        (r#"@for (x of xs; track x) {} @empty { <b (click)="go(x.toLowerCase())"></b> }"#, None),
    ];

    let fail = vec![
        ("{{ name.toLowerCase() }}", None),
        ("{{ title.toUpperCase() }}", None),
        ("{{ text.toLocaleLowerCase() }}", None),
        ("{{ text.toLocaleUpperCase() }}", None),
        (r#"<div [title]="name.toLowerCase()">Content</div>"#, None),
        (r#"<div title="{{ name.toUpperCase() }}">Content</div>"#, None),
        (
            r#"<button (click)="handleClick(name.toLowerCase())">Click</button>"#,
            Some(json!([{ "allowInOutputHandlers": false }])),
        ),
        (
            "{{ name.toUpperCase() }}",
            Some(json!([{ "disallowList": ["toUpperCase", "toLowerCase"] }])),
        ),
        ("{{ date.getFullYear() }}", Some(json!([{ "disallowList": ["getFullYear"] }]))),
        // Reported once, though the structural template holds a copy of the input.
        (r#"<div *ngIf="show" [title]="name.toLowerCase()"></div>"#, None),
        (r#"<ng-template [ngIf]="name.toLowerCase()"><p>x</p></ng-template>"#, None),
        // The default list stays when only `allowInOutputHandlers` is set.
        ("@if (a.toUpperCase()) { x }", Some(json!([{ "allowInOutputHandlers": false }]))),
    ];

    Tester::new(PreferBuiltInPipes::NAME, PreferBuiltInPipes::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '{{ name | lowercase }}' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '{{ name.toLowerCase() }}' }) class A {}",
    ];
    Tester::new(PreferBuiltInPipes::NAME, PreferBuiltInPipes::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
