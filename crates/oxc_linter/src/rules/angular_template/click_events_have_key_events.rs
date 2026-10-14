use cow_utils::CowUtils;
use oxc_angular_compiler::ast::r3::{R3BoundEvent, R3Element, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;

use super::button_has_type::IgnoredDirectives;
use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{
        ANGULAR_DOM_ELEMENTS, AngularAttributeValue, AngularTemplate, angular_attribute_value,
        is_angular_element_hidden,
    },
};

fn click_events_have_key_events_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "click must be accompanied by either keyup, keydown or keypress event for accessibility.",
    )
    .with_label(span)
}

fn click_events_have_key_code_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "click must be accompanied by a keyup, keydown or keypress event that specifies a key (e.g. `(keydown.enter)`) for accessibility.",
    )
    .with_label(span)
}

fn click_events_have_allowed_key_code_diagnostic(allowed: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "click must be accompanied by a keyup, keydown or keypress event for one of the allowed keys ({allowed}) for accessibility."
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct ClickEventsHaveKeyEvents(Box<ClickEventsHaveKeyEventsConfig>);

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct ClickEventsHaveKeyEventsConfig {
    /// Directive names that, when present on the element, cause it to be ignored. Entries
    /// wrapped in slashes, e.g. `/^tui/`, are treated as regular expressions.
    ignore_with_directives: IgnoredDirectives,
    /// Require key events to specify a key via a pseudo-event, e.g. `(keydown.enter)`. A bare
    /// `(keydown)` will not satisfy the rule.
    require_key_code: bool,
    /// Only key events whose key (the last segment of the pseudo-event, e.g. `enter` in
    /// `(keydown.shift.enter)`) is in this list satisfy the rule. Implies `requireKeyCode`.
    allowed_key_codes: Vec<String>,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that an element with a `(click)` handler also has a `(keyup)`, `(keydown)` or
    /// `(keypress)` handler, unless it is interactive by nature (like `<button>`), has
    /// `role="button"`, a presentation role, or is hidden from screen readers.
    ///
    /// ### Why is this bad?
    ///
    /// Keyboard users cannot click: a non-interactive element that reacts to clicks must also
    /// react to the keyboard.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div (click)="onClick()"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div (click)="onClick()" (keyup)="onKeyup()"></div>
    /// <button (click)="onClick()"></button>
    /// ```
    ClickEventsHaveKeyEvents,
    angular_template,
    correctness,
    config = ClickEventsHaveKeyEvents,
    version = "next",
    short_description = "Ensures that the click event is accompanied with at least one key event keyup, keydown or keypress.",
);

impl Rule for ClickEventsHaveKeyEvents {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        let mut rule =
            DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)?;
        for key in &mut rule.0.allowed_key_codes {
            *key = key.cow_to_lowercase().into_owned();
        }
        Ok(rule)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(
                &mut Visitor { rule: &self.0, ctx, template, hidden_ancestors: 0 },
                template.nodes,
            );
        }
    }
}

struct Visitor<'c, 'a> {
    rule: &'c ClickEventsHaveKeyEventsConfig,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
    hidden_ancestors: u32,
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        let hidden = is_angular_element_hidden(self.ctx, self.template, element);
        if !hidden && self.hidden_ancestors == 0 {
            self.check(element);
        }
        self.hidden_ancestors += u32::from(hidden);
        self.visit_element_children(element);
        self.hidden_ancestors -= u32::from(hidden);
    }
}

impl<'a> Visitor<'_, 'a> {
    fn check(&self, element: &R3Element<'a>) {
        if !ANGULAR_DOM_ELEMENTS.contains(element.name.cow_to_ascii_lowercase().as_ref())
            || self.rule.ignore_with_directives.matches(element)
            || is_presentation_role(self.ctx, self.template, element)
            || is_inherently_interactive_element(element)
            || role_matches(self.ctx, self.template, element, |role| role == "button")
        {
            return;
        }

        let allowed = &self.rule.allowed_key_codes;
        let (mut has_click, mut has_key_event, mut satisfied) = (false, false, false);
        for output in &element.outputs {
            has_click |= output.name == "click";
            if !is_key_event(output) {
                continue;
            }
            has_key_event = true;
            let key = key_code(output);
            satisfied |= if !allowed.is_empty() {
                key.is_some_and(|key| allowed.contains(&key))
            } else if self.rule.require_key_code {
                key.is_some()
            } else {
                true
            };
        }
        if !has_click || satisfied {
            return;
        }

        let span = self.template.span(element.source_span);
        self.ctx.diagnostic(if !has_key_event {
            click_events_have_key_events_diagnostic(span)
        } else if !allowed.is_empty() {
            click_events_have_allowed_key_code_diagnostic(&allowed.join(", "), span)
        } else {
            click_events_have_key_code_diagnostic(span)
        });
    }
}

fn is_key_event(event: &R3BoundEvent) -> bool {
    ["keyup", "keydown", "keypress"].iter().any(|key_event| {
        event
            .name
            .strip_prefix(key_event)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    })
}

/// The key of a key pseudo-event, like `enter` for `(keydown.shift.enter)`.
fn key_code(event: &R3BoundEvent) -> Option<String> {
    let (_, key) = event.name.rsplit_once('.')?;
    (!key.is_empty()).then(|| key.cow_to_lowercase().into_owned())
}

/// Whether the element's `role` is a string accepted by `f`.
pub(super) fn role_matches<'a>(
    ctx: &LintContext<'a>,
    template: &AngularTemplate,
    element: &R3Element<'a>,
    f: impl FnOnce(&str) -> bool,
) -> bool {
    angular_attribute_value(ctx, template, element, "role")
        .as_ref()
        .and_then(AngularAttributeValue::as_str)
        .is_some_and(f)
}

pub(super) fn is_presentation_role<'a>(
    ctx: &LintContext<'a>,
    template: &AngularTemplate,
    element: &R3Element<'a>,
) -> bool {
    role_matches(ctx, template, element, |role| matches!(role, "none" | "presentation"))
}

/// `@angular-eslint`'s `isInherentlyInteractiveElement`, reduced from the `aria-query` 5.3.2
/// and `axobject-query` 4.1.0 tables it filters: these elements match an interactive schema
/// whatever their attributes, except `a` and `area`, which need `href`.
pub(super) fn is_inherently_interactive_element(element: &R3Element) -> bool {
    match element.name.cow_to_ascii_lowercase().as_ref() {
        "audio" | "button" | "canvas" | "datalist" | "embed" | "input" | "menuitem" | "option"
        | "select" | "summary" | "td" | "textarea" | "th" | "tr" | "video" => true,
        "a" | "area" => {
            let names = element.attributes.iter().map(|attr| attr.name.as_str());
            let mut names = names.chain(element.inputs.iter().map(|input| input.name.as_str()));
            // Any `routerLink` satisfies the schema's attributes, on a lower case `a` only.
            names.any(|name| name == "href" || (element.name == "a" && name == "routerLink"))
        }
        _ => false,
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        (r#"<div (click)="onClick()" (keyup)="onKeyup()"></div>"#, None),
        (r#"<div (keyup)="onKeyup()" (click)="onClick()"></div>"#, None),
        (
            r#"<div (click)="onClick()" (keyup)="onKeyup()" (handleSomething)="handleSomething()"></div>"#,
            None,
        ),
        (r#"<div (click)="onClick()" (keyup.enter)="onKeyup()"></div>"#, None),
        (r#"<cui-button (click)="onClick()"></cui-button>"#, None),
        (r#"<div (click)="onClick()" aria-hidden></div>"#, None),
        (r#"<div (click)="onClick()" aria-hidden="true"></div>"#, None),
        (r#"<div (click)="onClick()" [attr.aria-hidden]="true"></div>"#, None),
        (r#"<div (click)="onClick()" role="presentation"></div>"#, None),
        (r#"<div (click)="onClick()" [attr.role]="'none'"></div>"#, None),
        (r#"<input (click)="onClick()">"#, None),
        (r#"<button (click)="onClick()"></button>"#, None),
        (r#"<textarea (click)="onClick()"></textarea>"#, None),
        (r#"<select (click)="onClick()"><option (click)="onClick()"></option></select>"#, None),
        (r##"<a href="#" (click)="onClick()"></a>"##, None),
        (r#"<a [attr.href]="href" class="anchor" (click)="onClick()"></a>"#, None),
        (r#"<a [routerLink]="'route'" (click)="onClick()"></a>"#, None),
        (r#"<div role="button" (click)="doSomething()"></div>"#, None),
        (r#"<span role="button" (click)="doSomething()"></span>"#, None),
        (r#"<p role="button" (click)="doSomething()"></p>"#, None),
        (
            r#"<div myDirective (click)="onClick()"></div>"#,
            Some(json!([{ "ignoreWithDirectives": ["myDirective"] }])),
        ),
        (
            r#"<div [myDirective] (click)="onClick()"></div>"#,
            Some(json!([{ "ignoreWithDirectives": ["myDirective"] }])),
        ),
        (
            r#"<div tuiOption (click)="onClick()"></div>"#,
            Some(json!([{ "ignoreWithDirectives": ["/^tui/"] }])),
        ),
        (
            r#"<div (click)="onClick()" (keydown.enter)="onKeydown()"></div>"#,
            Some(json!([{ "requireKeyCode": true }])),
        ),
        (
            r#"<div (click)="onClick()" (keyup.shift.space)="onKeyup()"></div>"#,
            Some(json!([{ "requireKeyCode": true }])),
        ),
        (
            r#"<div (click)="onClick()" (keydown)="onKeydown()" (keydown.enter)="onKeydown()"></div>"#,
            Some(json!([{ "requireKeyCode": true }])),
        ),
        (
            r#"<div (click)="onClick()" (keydown.enter)="onKeydown()"></div>"#,
            Some(json!([{ "allowedKeyCodes": ["Enter", "ArrowLeft"] }])),
        ),
        (
            r#"<div (click)="onClick()" (keyup.Enter)="onKeyup()"></div>"#,
            Some(json!([{ "allowedKeyCodes": ["Enter", "ArrowLeft"] }])),
        ),
        (
            r#"<div (click)="onClick()" (keydown.shift.arrowleft)="onKeydown()"></div>"#,
            Some(json!([{ "allowedKeyCodes": ["Enter", "ArrowLeft"] }])),
        ),
        (
            r#"<div (click)="onClick()" (keydown.space)="onKeydown()" (keydown.enter)="onKeydown()"></div>"#,
            Some(json!([{ "allowedKeyCodes": ["Enter", "ArrowLeft"] }])),
        ),
        // Hidden by an ancestor, a custom element included.
        (r#"<div hidden><span (click)="onClick()"></span></div>"#, None),
        (r#"<app-panel aria-hidden="true"><span (click)="onClick()"></span></app-panel>"#, None),
        (r#"<div *ngIf="show" (click)="onClick()" (keyup)="onKeyup()"></div>"#, None),
        (r#"<ng-template (click)="onClick()"></ng-template>"#, None),
        (r#"<td (click)="onClick()"></td>"#, None),
        (r#"<summary (click)="onClick()"></summary>"#, None),
    ];

    let fail = vec![
        (r#"<div (click)="onClick()"></div>"#, None),
        (r#"<div (click)="onClick()" (handleSomething)="handleSomething()"></div>"#, None),
        (r#"<header (click)="onClick()"></header>"#, None),
        (r#"<a (click)="onClick()"></a>"#, None),
        (r#"<A (click)="onClick()"></A>"#, None),
        (r#"<div (click)="onClick()" aria-hidden="false"></div>"#, None),
        (r#"<div (click)="onClick()" [attr.aria-hidden]="'false'"></div>"#, None),
        (r#"<div (click)="onClick()" role="header"></div>"#, None),
        (r#"<div (click)="onClick()" role="aside"></div>"#, None),
        (r#"<div (click)="onClick()" [attr.role]="'header'"></div>"#, None),
        (r#"<div (click)="onClick()" [attr.aria-hidden]="ariaHidden"></div>"#, None),
        (r#"<div (click)="onClick()" [attr.hidden]="hidden"></div>"#, None),
        (
            r#"<div myDirective (click)="onClick()"></div>"#,
            Some(json!([{ "ignoreWithDirectives": ["testDirective", "otherDirective"] }])),
        ),
        (
            r#"<div myDirective (click)="onClick()"></div>"#,
            Some(json!([{ "ignoreWithDirectives": ["/^tui/"] }])),
        ),
        (
            r#"<div (click)="onClick()" (keydown)="onKeydown()"></div>"#,
            Some(json!([{ "requireKeyCode": true }])),
        ),
        (r#"<div (click)="onClick()"></div>"#, Some(json!([{ "requireKeyCode": true }]))),
        (
            r#"<div (click)="onClick()" (keydown.space)="onKeydown()"></div>"#,
            Some(json!([{ "allowedKeyCodes": ["Enter", "ArrowLeft", "ArrowRight"] }])),
        ),
        (
            r#"<div (click)="onClick()" (keydown)="onKeydown()"></div>"#,
            Some(json!([{ "allowedKeyCodes": ["Enter"] }])),
        ),
        // Mixed case names are native elements too.
        (r#"<Div (click)="onClick()"></Div>"#, None),
        (r#"<A routerLink="x" (click)="onClick()"></A>"#, None),
        (r#"<div *ngIf="show" (click)="onClick()"></div>"#, None),
        (r#"<ng-template><span (click)="onClick()"></span></ng-template>"#, None),
        (r#"@defer { <span (click)="onClick()"></span> }"#, None),
    ];

    Tester::new(ClickEventsHaveKeyEvents::NAME, ClickEventsHaveKeyEvents::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<div (click)=\"a()\" (keyup)=\"a()\"></div>' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<div (click)=\"a()\"></div>' }) class A {}",
    ];
    Tester::new(
        ClickEventsHaveKeyEvents::NAME,
        ClickEventsHaveKeyEvents::PLUGIN,
        inline_pass,
        inline_fail,
    )
    .change_rule_path_extension("ts")
    .with_snapshot_suffix("inline")
    .test_and_snapshot();
}
