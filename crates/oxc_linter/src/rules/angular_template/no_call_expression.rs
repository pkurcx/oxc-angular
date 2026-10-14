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

fn no_call_expression_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Avoid calling expressions in templates").with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NoCallExpression(Box<NoCallExpressionConfig>);

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
#[expect(clippy::struct_field_names)]
struct NoCallExpressionConfig {
    /// Names of functions and methods that may be called.
    allow_list: Vec<String>,
    /// Functions and methods whose name starts with this prefix may be called.
    allow_prefix: Option<String>,
    /// Functions and methods whose name ends with this suffix may be called.
    allow_suffix: Option<String>,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows calling functions and methods in templates, except in output handlers and
    /// `$any()`.
    ///
    /// ### Why is this bad?
    ///
    /// A call in a binding runs on every change detection cycle, which can happen many times a
    /// second and once per item of a list. A component property, a pure pipe or a computed signal
    /// is only recalculated when its inputs change.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div>{{ getInfo() }}</div>
    /// <a [href]="createUrl()">info</a>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div>{{ info }}</div>
    /// <button (click)="handleClick()">Click Here</button>
    /// ```
    NoCallExpression,
    angular_template,
    restriction,
    config = NoCallExpression,
    version = "next",
    short_description = "Disallows calling expressions in templates, except for output handlers",
);

impl Rule for NoCallExpression {
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
                if in_event {
                    return;
                }
                if let Some(name) = name_of(&call.receiver)
                    && (name == "$any"
                        || config.allow_list.iter().any(|allowed| allowed == name)
                        || config
                            .allow_prefix
                            .as_deref()
                            .is_some_and(|p| !p.is_empty() && name.starts_with(p))
                        || config
                            .allow_suffix
                            .as_deref()
                            .is_some_and(|s| !s.is_empty() && name.ends_with(s)))
                {
                    return;
                }
                ctx.diagnostic(no_call_expression_diagnostic(
                    template.expression_span(call.source_span),
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

fn name_of<'e>(expression: &'e AngularExpression) -> Option<&'e str> {
    let name = match expression {
        AngularExpression::PropertyRead(read) => read.name.as_str(),
        AngularExpression::SafePropertyRead(read) => read.name.as_str(),
        _ => return None,
    };
    (!name.is_empty()).then_some(name)
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        ("{{ info }}", None),
        (r#"<button type="button" (click)="handleClick()">Click Here</button>"#, None),
        ("{{ $any(info) }}", None),
        (r#"<input (change)="obj?.changeHandler()">"#, None),
        (r#"<form [formGroup]="form" (ngSubmit)="form.valid || save()"></form>"#, None),
        (r#"<form [formGroup]="form" (ngSubmit)="form.valid && save()"></form>"#, None),
        (r#"<form [formGroup]="form" (ngSubmit)="id ? save() : edit()"></form>"#, None),
        (
            r#"
      {{ obj?.nested() }} {{ obj!.nested() }}
      <a [href]="getHref()">info</a>
      {{ $validWithPrefix() }} {{ validWithSuffix$() }}
    "#,
            Some(
                json!([{ "allowList": ["nested", "getHref"], "allowPrefix": "$", "allowSuffix": "$" }]),
            ),
        ),
        (
            "
  @if (condition) {
    <div></div>
  } @else if (otherCondition) {
    <div></div>
  } @else {
    <div></div>
  }",
            None,
        ),
        (
            "
  @switch (condition) {
    @case(value) {
      <div></div>
    }
    @default {
      <div></div>
    }
  }",
            None,
        ),
        (
            "
  @for (item of list; track item.id) {
    <div></div>
  } @empty {
    <div></div>
  }",
            None,
        ),
        (
            "
  @defer (on viewport(ref); prefetch on viewport(ref)) {
    <div></div>
  } @error {
    <div></div>
  } @loading {
    <div></div>
  } @placeholder {
    <div></div>
  }",
            None,
        ),
        // Output handlers on a structural directive's element and on `<ng-template>`.
        (r#"<button *ngIf="show" (click)="save()">Save</button>"#, None),
        (r#"<ng-template (click)="save()"></ng-template>"#, None),
        (r#"@defer {} @placeholder { <b (click)="save()"></b> }"#, None),
        // A safe call is not a call expression upstream.
        ("{{ fn?.() }}", None),
    ];

    let fail = vec![
        ("<div>{{ getInfo() }}</div>", None),
        (r#"<a href="{{ getUrls().user }}"></a>"#, None),
        (r#"<p [test]="test?.getInfo()"></p>"#, None),
        (
            r#"
        <a [href]="id && createUrl() && test($any)">info</a>
        {{ id || obj?.nested1() }}
      "#,
            None,
        ),
        (
            r#"
        <a [href]="id ? a?.createUrl() : editUrl(3)">info</a>
        {{ 1 === 2 ? 3 : obj?.nested1() }}
      "#,
            None,
        ),
        (
            r#"
        {{ obj?.nested1() }} {{ obj!.nested1() }}
        <button [type]="obj!.$any(b)!.getType()">info</button>
        <a [href]="obj.propertyA?.href()">info</a>
      "#,
            None,
        ),
        (
            r#"
    @if (foo()) {
      <div [id]="foo()"></div>
    } @else if (foo()) {
      <div [id]="foo()"></div>
    } @else {
      <div [id]="foo()"></div>
    }"#,
            None,
        ),
        (
            r#"
    @switch (foo()) {
      @case(foo()) {
        <div [id]="foo()"></div>
      }
      @default {
        <div [id]="foo()"></div>
      }
    }"#,
            None,
        ),
        (
            r#"
    @for (item of getFooList(); track item.getId()) {
      <div [id]="foo()"></div>
    } @empty {
      <div [id]="foo()"></div>
    }"#,
            None,
        ),
        (
            r#"
    @defer (when foo(); prefetch when foo()) {
      <div [id]="foo()"></div>
    } @error {
      <div [id]="foo()"></div>
    } @loading {
      <div [id]="foo()"></div>
    } @placeholder {
      <div [id]="foo()"></div>
    }"#,
            None,
        ),
        (
            "
    @defer (on idle; hydrate when foo()) {
      <div></div>
    }",
            None,
        ),
        ("{{ foo(bar(), baz()) }}", None),
        ("{{ foo()() }}", None),
        ("{{ foo().bar() }}", None),
        ("{{ notOk().ok() }}", Some(json!([{ "allowList": ["ok"] }]))),
        // Empty affixes allow nothing, as upstream treats them as unset.
        ("{{ foo() }}", Some(json!([{ "allowPrefix": "", "allowSuffix": "" }]))),
        // Reported once, though the structural template holds a copy of the input.
        (r#"<div *ngIf="isVisible()" [title]="getTitle()"></div>"#, None),
        (r#"<ng-template [ngIf]="isVisible()"><p>x</p></ng-template>"#, None),
        // Only the event handler is exempt, not the element's other bindings.
        (r#"<button [disabled]="isBusy()" (click)="save()"></button>"#, None),
        ("@let total = sum(items);", None),
    ];

    Tester::new(NoCallExpression::NAME, NoCallExpression::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<button (click)=\"save()\"></button>' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '{{ getInfo() }}' }) class A {}",
    ];
    Tester::new(NoCallExpression::NAME, NoCallExpression::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
