use cow_utils::CowUtils;
use oxc_angular_compiler::ast::{
    expression::LiteralValue,
    r3::{R3Element, R3Visitor, visit_all},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;

use super::click_events_have_key_events::{
    is_inherently_interactive_element, is_presentation_role, role_matches,
};
use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{
        ANGULAR_DOM_ELEMENTS, AngularAttributeValue, AngularTemplate, angular_attribute_value,
        is_angular_element_hidden,
    },
};

fn interactive_supports_focus_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Elements with interaction handlers must be focusable.").with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct InteractiveSupportsFocus(Box<InteractiveSupportsFocusConfig>);

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[schemars(rename = "AngularTemplateInteractiveSupportsFocusConfig")]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct InteractiveSupportsFocusConfig {
    /// Elements that are not checked, like `form`, where click and key events of the controls
    /// inside bubble up.
    allow_list: Vec<String>,
}

impl Default for InteractiveSupportsFocusConfig {
    fn default() -> Self {
        Self { allow_list: vec!["form".to_string()] }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that elements with `(click)`, `(keyup)`, `(keydown)` or `(keypress)` handlers
    /// are focusable: interactive by nature, editable, or with a `tabindex`.
    ///
    /// ### Why is this bad?
    ///
    /// Keyboard users can only trigger the handlers of elements they can focus.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <span (click)="onClick()">Submit</span>
    /// <div role="button" (click)="onClick()"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <span (click)="onClick()" tabindex="0">Submit</span>
    /// <button (click)="onClick()">Submit</button>
    /// ```
    InteractiveSupportsFocus,
    angular_template,
    correctness,
    config = InteractiveSupportsFocus,
    version = "next",
    short_description = "Ensures that elements with interactive handlers like `(click)` are focusable.",
);

impl Rule for InteractiveSupportsFocus {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
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
    rule: &'c InteractiveSupportsFocusConfig,
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
        let (ctx, template) = (self.ctx, self.template);
        let name = element.name.cow_to_ascii_lowercase();
        if !ANGULAR_DOM_ELEMENTS.contains(name.as_ref())
            || self.rule.allow_list.iter().any(|allowed| *allowed == name)
        {
            return;
        }
        let has_interactive_output = element.outputs.iter().any(|output| {
            output.name == "click"
                || ["keyup", "keydown", "keypress"].iter().any(|key| output.name.starts_with(key))
        });
        if !has_interactive_output
            || is_disabled_element(ctx, template, element)
            || is_presentation_role(ctx, template, element)
        {
            return;
        }
        let has_tabindex = element.attributes.iter().any(|attr| attr.name == "tabindex")
            || element.inputs.iter().any(|input| input.name == "tabindex");
        if has_tabindex
            || is_inherently_interactive_element(element)
            || role_matches(ctx, template, element, |role| NON_INTERACTIVE_ROLES.contains(role))
            || is_content_editable(ctx, template, element)
        {
            return;
        }
        ctx.diagnostic(interactive_supports_focus_diagnostic(template.span(element.source_span)));
    }
}

fn is_disabled_element<'a>(
    ctx: &LintContext<'a>,
    template: &AngularTemplate,
    element: &R3Element<'a>,
) -> bool {
    let value = |name: &str| angular_attribute_value(ctx, template, element, name);
    // Any value but a literal `undefined`, so `[disabled]="false"` counts too.
    !matches!(
        value("disabled"),
        None | Some(AngularAttributeValue::Literal(LiteralValue::Undefined))
    ) || value("aria-disabled")
        .and_then(|value| value.as_string())
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn is_content_editable<'a>(
    ctx: &LintContext<'a>,
    template: &AngularTemplate,
    element: &R3Element<'a>,
) -> bool {
    angular_attribute_value(ctx, template, element, "contenteditable")
        .and_then(|value| value.as_string())
        .is_some_and(|value| value.is_empty() || value.eq_ignore_ascii_case("true"))
}

/// Roles that are not widgets in `aria-query` 5.3.2, except `toolbar`, plus `progressbar`.
const NON_INTERACTIVE_ROLES: phf::Set<&'static str> = phf::phf_set![
    "alert",
    "alertdialog",
    "application",
    "article",
    "banner",
    "blockquote",
    "caption",
    "cell",
    "code",
    "complementary",
    "contentinfo",
    "definition",
    "deletion",
    "dialog",
    "directory",
    "doc-abstract",
    "doc-acknowledgments",
    "doc-afterword",
    "doc-appendix",
    "doc-biblioentry",
    "doc-bibliography",
    "doc-chapter",
    "doc-colophon",
    "doc-conclusion",
    "doc-cover",
    "doc-credit",
    "doc-credits",
    "doc-dedication",
    "doc-endnote",
    "doc-endnotes",
    "doc-epigraph",
    "doc-epilogue",
    "doc-errata",
    "doc-example",
    "doc-footnote",
    "doc-foreword",
    "doc-glossary",
    "doc-index",
    "doc-introduction",
    "doc-notice",
    "doc-pagebreak",
    "doc-pagefooter",
    "doc-pageheader",
    "doc-pagelist",
    "doc-part",
    "doc-preface",
    "doc-prologue",
    "doc-pullquote",
    "doc-qna",
    "doc-subtitle",
    "doc-tip",
    "doc-toc",
    "document",
    "emphasis",
    "feed",
    "figure",
    "form",
    "generic",
    "graphics-document",
    "graphics-object",
    "graphics-symbol",
    "group",
    "heading",
    "img",
    "insertion",
    "list",
    "listitem",
    "log",
    "main",
    "mark",
    "marquee",
    "math",
    "meter",
    "navigation",
    "none",
    "note",
    "paragraph",
    "presentation",
    "progressbar",
    "region",
    "rowgroup",
    "search",
    "separator",
    "status",
    "strong",
    "subscript",
    "superscript",
    "table",
    "tabpanel",
    "term",
    "time",
    "timer",
    "tooltip",
];

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        ("<div></div>", None),
        (r#"<div aria-hidden (click)="onClick()"></div>"#, None),
        (r#"<div aria-hidden="true" (click)="onClick()"></div>"#, None),
        (r#"<div [attr.aria-hidden]="true" (click)="onClick()"></div>"#, None),
        (r#"<div role="button" aria-disabled="true" (click)="onClick()"></div>"#, None),
        (r#"<div role="button" [attr.aria-disabled]="true" (click)="onClick()"></div>"#, None),
        (r#"<div role="presentation" (click)="onClick()"></div>"#, None),
        (r#"<div role="none" (click)="onClick()"></div>"#, None),
        (r#"<div role="progressbar" (click)="onClick()"></div>"#, None),
        (r#"<div role="region" (click)="onClick()"></div>"#, None),
        (r#"<input type="hidden" (click)="onClick()">"#, None),
        (r#"<input type="hidden" (click)="onClick()" tabindex="-1">"#, None),
        (r#"<input type="hidden" (click)="onClick()" [attr.tabindex]="-1">"#, None),
        (r#"<input type="text" (keyup)="onKeyUp()">"#, None),
        (r#"<input (keydown)="onKeydown()">"#, None),
        (r#"<input (click)="onClick()" role="combobox">"#, None),
        (r#"<button (click)="onClick()" class="foo">Foo</button>"#, None),
        (r#"<option (click)="onClick()" class="foo">Food</option>"#, None),
        (r#"<select (click)="onClick()" class="foo"></select>"#, None),
        (r#"<summary (click)="onClick()">Foo</summary>"#, None),
        (r#"<textarea (keypress)="onKeypress()" class="foo"></textarea>"#, None),
        (r#"<input disabled type="text" (click)="onClick()">"#, None),
        (r#"<button disabled (click)="onClick()" class="foo">Foo</button>"#, None),
        (r#"<select disabled (click)="onClick()" class="foo"></select>"#, None),
        (r##"<area href="#" (click)="onClick()" class="foo"/>"##, None),
        (r#"<area (click)="onClick()" tabindex=0 class="foo"/>"#, None),
        (r#"<a (click)="onClick()" tabindex="0">Click me</a>"#, None),
        ("<a (click)=\"onClick()\" tabindex={{0}}>Click me</a>", None),
        (r#"<a (click)="onClick()" [attr.tabindex]="0">Click me</a>"#, None),
        (r#"<a (click)="onClick()" tabindex="bad">Click me</a>"#, None),
        (r#"<a (click)="onClick()" [attr.tabindex]="undefined"}>Click me</a>"#, None),
        (r#"<a (click)="onClick()" [attr.tabindex]="dynamicTabindex">Click me</a>"#, None),
        (r##"<a role="button" (click)="onClick()" href="#">hash</a>"##, None),
        (r#"<a (click)="onClick()" href="http://x.y.z">x.y.z</a>"#, None),
        (r#"<a role="link" (click)="onClick()" href="http://x.y.z">x.y.z</a>"#, None),
        (r#"<a (click)="onClick()" href="javascript:void(0);">Click ALL the things!</a>"#, None),
        (r#"<a routerLink="route" (click)="onClick()"></a>"#, None),
        (r#"<a [routerLink]="route" (click)="onClick()"></a>"#, None),
        (r#"<div (click)="onClick()" tabindex="invalid"></div>"#, None),
        (r#"<div (click)="onClick()" [attr.tabindex]="undefined"></div>"#, None),
        // Upstream also has `[attr.tabindex]=="false"`, which is a template parse error here.
        (r#"<span (click)="onClick()" [attr.tabindex]="false">Submit</span>"#, None),
        (r#"<span (click)="onClick()" [attr.tabindex]="null">Submit</span>"#, None),
        (r#"<span (click)="onClick()" tabindex="0">Click me!</span>"#, None),
        (r#"<span (click)="onClick()" [attr.tabindex]="0">Click me!</span>"#, None),
        (r#"<span (click)="onClick()" tabindex="-1">Click me!</span>"#, None),
        (r#"<span (click)="onClick()" [attr.tabindex]="-1">Click me!</span>"#, None),
        (r#"<div role="button" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div role="checkbox" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div role="link" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div role="menuitem" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div role="option" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div role="radio" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div role="spinbutton" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div role="switch" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div role="tablist" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div role="tab" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div role="textbox" tabindex="0" (click)="onClick()"></div>"#, None),
        (r#"<div contenteditable="true" (keyup)="onKeyUp()">Edit this text</div>"#, None),
        (
            r#"<div [attr.contenteditable]="true" (keydown)="onKeyDown()">Edit this text</div>"#,
            None,
        ),
        (r#"<div contenteditable (keypress)="onKeyPress()">Edit this too!</div>"#, None),
        (r#"<test-component (keydown)="onKeyDown()"></test-component>"#, None),
        (r#"<form (keydown)="onKeyDown()"></form>"#, None),
        (
            r#"<form (keydown)="onKeyDown()"></form><section (keydown)="onKeyDown()"></section>"#,
            Some(json!([{ "allowList": ["form", "section"] }])),
        ),
        (r#"<div hidden><span (click)="onClick()"></span></div>"#, None),
        (r#"<div *ngIf="show" (click)="onClick()" tabindex="0"></div>"#, None),
        (r#"<ng-template (click)="onClick()"></ng-template>"#, None),
    ];

    let fail = vec![
        (r#"<div aria-hidden="false" (click)="onClick()"></div>"#, None),
        (r#"<div [attr.aria-hidden]="false" (click)="onClick()"></div>"#, None),
        (r#"<div role="button" aria-disabled="false" (click)="onClick()"></div>"#, None),
        (r#"<div [attr.aria-disabled]="false" (click)="onClick()"></div>"#, None),
        (r#"<div role="button" (click)="onClick()"></div>"#, None),
        (r#"<span (click)="onClick()">Submit</span>"#, None),
        (r#"<SPAN (click)="onClick()">Submit</SPAN>"#, None),
        (r#"<div (click)="onClick()" [attr.aria-label]="clickableThing"></div>"#, None),
        (r#"<div (click)="onClick()" role="invalid"></div>"#, None),
        (r#"<area (click)="onClick()" class="foo">"#, None),
        (r#"<a (click)="onClick()">Click me</a>"#, None),
        (
            r#"<div (keyup)="onKeyUp()" (keydown)="onKeyDown()" (keypress)="onKeyPress()">Cannot be focused</div>"#,
            None,
        ),
        (
            r#"<div [attr.contenteditable]="false" (keyup)="onKeyUp()">Cannot be focused</div>"#,
            None,
        ),
        (r#"<form (keydown)="onKeyDown()"></form>"#, Some(json!([{ "allowList": [] }]))),
        // `row` is a widget role.
        (r#"<div role="row" (click)="onClick()"></div>"#, None),
        (r#"<div [tabIndex]="0" (click)="onClick()"></div>"#, None),
        (r#"<div *ngIf="show" (click)="onClick()"></div>"#, None),
        (r#"<ng-template><span (click)="onClick()"></span></ng-template>"#, None),
        (r#"@for (item of items; track item) { <li (click)="select(item)"></li> }"#, None),
    ];

    Tester::new(InteractiveSupportsFocus::NAME, InteractiveSupportsFocus::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<span (click)=\"a()\" tabindex=\"0\"></span>' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<span (click)=\"a()\"></span>' }) class A {}",
    ];
    Tester::new(
        InteractiveSupportsFocus::NAME,
        InteractiveSupportsFocus::PLUGIN,
        inline_pass,
        inline_fail,
    )
    .change_rule_path_extension("ts")
    .with_snapshot_suffix("inline")
    .test_and_snapshot();
}
