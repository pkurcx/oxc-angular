use lazy_regex::{Regex, regex_is_match};
use oxc_angular_compiler::ast::{
    expression::AngularExpression,
    r3::{I18nMeta, R3Node, R3Template, R3TextAttribute},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{AngularTemplate, deserialize_regex_option},
};

const GUIDE_COMMON_PREPARE: &str = "https://angular.dev/guide/i18n-common-prepare";
const GUIDE_CUSTOM_IDS: &str = "https://angular.dev/guide/i18n-optional-manage-marked-text";

fn attribute_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Attribute \"{name}\" has no corresponding i18n attribute. See more at {GUIDE_COMMON_PREPARE}#mark-element-attributes-for-translations"
    ))
    .with_label(span)
}

fn icu_or_text_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Each element containing text node should have an i18n attribute. See more at {GUIDE_COMMON_PREPARE}#icu-expressions and {GUIDE_COMMON_PREPARE}#mark-text-in-component-template"
    ))
    .with_label(span)
}

fn custom_id_on_attribute_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Missing custom ID on attribute \"i18n-{name}\". See more at {GUIDE_CUSTOM_IDS}"
    ))
    .with_label(span)
}

fn custom_id_on_element_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Missing custom ID on element. See more at {GUIDE_CUSTOM_IDS}"))
        .with_label(span)
}

fn duplicate_custom_id_diagnostic(id: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Duplicate custom ID \"@@{id}\". See more at {GUIDE_CUSTOM_IDS}#define-unique-custom-ids"
    ))
    .with_label(span)
}

fn missing_description_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Missing i18n description on element. See more at {GUIDE_COMMON_PREPARE}#i18n-metadata-for-translation"
    ))
    .with_label(span)
}

fn missing_meaning_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Missing i18n meaning on element. See more at {GUIDE_COMMON_PREPARE}#i18n-metadata-for-translation"
    ))
    .with_label(span)
}

fn markup_in_content_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Avoid HTML markup in an element with an i18n attribute.").with_label(span)
}

const DEFAULT_IGNORE_ATTRIBUTES: [&str; 34] = [
    "autocomplete",
    "charset",
    "class",
    "color",
    "colspan",
    "dir",
    "fill",
    "for",
    "formArrayName",
    "formControlName",
    "formGroupName",
    "height",
    "href",
    "id",
    "lang",
    "list",
    "name",
    "ngClass",
    "ngProjectAs",
    "role",
    "routerLink",
    "routerLinkActive",
    "src",
    "stroke",
    "stroke-width",
    "style",
    "svgIcon",
    "tabindex",
    "target",
    "type",
    "value",
    "viewBox",
    "width",
    "xmlns",
];

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct I18n(Box<I18nConfig>);

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct I18nConfig {
    /// Allow elements and `<ng-template>`s inside an element with an `i18n` attribute.
    allow_markup_in_content: bool,
    /// A pattern; interpolated text whose static parts match it needs no `i18n` attribute.
    #[serde(deserialize_with = "deserialize_regex_option")]
    #[schemars(with = "Option<String>")]
    bound_text_allowed_pattern: Option<Regex>,
    /// Report attributes with text that have no `i18n-<attribute>`.
    check_attributes: bool,
    /// Report custom IDs used more than once.
    check_duplicate_id: bool,
    /// Report `i18n` and `i18n-<attribute>` attributes without a custom ID (`@@id`).
    check_id: bool,
    /// Report text, interpolations and ICU expressions outside an element with `i18n`.
    check_text: bool,
    /// Attributes that need no translation, as `name` or `element[name]`, in addition to the
    /// defaults.
    ignore_attributes: Vec<String>,
    /// Elements whose text and attributes are not checked.
    ignore_tags: Vec<String>,
    /// Require a description in `i18n` and `i18n-<attribute>` attributes.
    require_description: bool,
    /// Require a meaning in `i18n` attributes.
    require_meaning: bool,
}

impl Default for I18nConfig {
    fn default() -> Self {
        Self {
            allow_markup_in_content: true,
            bound_text_allowed_pattern: None,
            check_attributes: true,
            check_duplicate_id: true,
            check_id: true,
            check_text: true,
            ignore_attributes: DEFAULT_IGNORE_ATTRIBUTES.iter().map(ToString::to_string).collect(),
            ignore_tags: vec![],
            require_description: false,
            require_meaning: false,
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures following best practices for i18n: text, ICU expressions and attributes with
    /// text are marked for translation with `i18n` or `i18n-<attribute>`. It can also require
    /// custom IDs (`@@id`) and report duplicate ones, require a description or meaning, and
    /// disallow markup inside translated elements.
    ///
    /// ### Why is this bad?
    ///
    /// Text that is not marked for translation stays in the source language in every
    /// translated build of the application.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div tooltip="This requires translation"></div>
    /// <span>Some text to translate</span>
    /// <span i18n>Missing custom ID</span>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div tooltip="This requires translation" i18n-tooltip="@@tooltip"></div>
    /// <span i18n="@@custom-id">Some text to translate</span>
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - With `allowMarkupInContent: false`, the ID, description and meaning checks run only on
    ///   elements with `i18n`; `@angular-eslint` reports every element, and crashes with
    ///   `requireDescription`.
    /// - The `meaning|description@@id` value is split by the template parser, which trims each
    ///   part, so values like `i18n="m | @@id"` or a `|` inside the custom ID are read differently.
    /// - Patterns in options use Rust regex syntax, which has no lookaround.
    I18n,
    angular_template,
    restriction,
    conditional_fix_suggestion,
    config = I18n,
    version = "next",
    short_description = "Ensures following best practices for i18n",
);

impl Rule for I18n {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            let mut walker = Walker { config: &self.0, ctx, template, custom_ids: vec![] };
            walker.walk(template.nodes, None);
            walker.report_duplicate_ids();
        }
    }
}

/// The nearest `Element` or `<ng-template>` around a node, as upstream's
/// `getNextElementOrTemplateParent` finds it.
struct Parent<'n> {
    tag: &'n str,
    has_i18n: bool,
    ancestor_has_i18n: bool,
    start: u32,
    has_markup_children: bool,
}

struct Walker<'c, 'a> {
    config: &'c I18nConfig,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
    custom_ids: Vec<(&'a str, Vec<Span>)>,
}

impl<'a> Walker<'_, 'a> {
    fn walk(&mut self, nodes: &'a [R3Node<'a>], parent: Option<&Parent<'a>>) {
        for node in nodes {
            match node {
                R3Node::Element(e) => self.element_like(
                    &e.name,
                    e.i18n.as_ref(),
                    e.source_span,
                    &e.children,
                    &e.attributes,
                    parent,
                ),
                // Only `<ng-template>` takes part; a structural directive's wrapper is see-through.
                R3Node::Template(t) if is_ng_template(t) => self.element_like(
                    "ng-template",
                    t.i18n.as_ref(),
                    t.source_span,
                    &t.children,
                    &[],
                    parent,
                ),
                R3Node::Template(t) => self.walk(&t.children, parent),
                R3Node::Content(c) => self.walk(&c.children, parent),
                R3Node::Component(c) => self.walk(&c.children, parent),
                R3Node::Text(t) if self.config.check_text && has_letter(&t.value) => {
                    self.text_like(self.skip_leading_trivia(t.source_span), None, parent);
                }
                R3Node::BoundText(t) if self.config.check_text => {
                    let strings = match &t.value {
                        AngularExpression::Interpolation(i) => {
                            i.strings.iter().map(oxc_str::Ident::as_str).collect::<String>()
                        }
                        _ => String::new(),
                    };
                    self.text_like(self.skip_leading_trivia(t.source_span), Some(&strings), parent);
                }
                // Its vars and placeholders are not nodes upstream, so they are not visited.
                R3Node::Icu(icu) if self.config.check_text => {
                    self.text_like(self.template.span(icu.source_span), None, parent);
                }
                R3Node::IfBlock(block) => {
                    for branch in &block.branches {
                        self.walk(&branch.children, parent);
                    }
                }
                R3Node::IfBlockBranch(branch) => self.walk(&branch.children, parent),
                R3Node::ForLoopBlock(block) => {
                    self.walk(&block.children, parent);
                    if let Some(empty) = &block.empty {
                        self.walk(&empty.children, parent);
                    }
                }
                R3Node::ForLoopBlockEmpty(empty) => self.walk(&empty.children, parent),
                R3Node::SwitchBlock(block) => {
                    for group in &block.groups {
                        self.walk(&group.children, parent);
                    }
                }
                R3Node::SwitchBlockCaseGroup(group) => self.walk(&group.children, parent),
                R3Node::DeferredBlock(block) => {
                    self.walk(&block.children, parent);
                    if let Some(b) = &block.placeholder {
                        self.walk(&b.children, parent);
                    }
                    if let Some(b) = &block.loading {
                        self.walk(&b.children, parent);
                    }
                    if let Some(b) = &block.error {
                        self.walk(&b.children, parent);
                    }
                }
                R3Node::DeferredBlockPlaceholder(b) => self.walk(&b.children, parent),
                R3Node::DeferredBlockLoading(b) => self.walk(&b.children, parent),
                R3Node::DeferredBlockError(b) => self.walk(&b.children, parent),
                _ => {}
            }
        }
    }

    fn element_like(
        &mut self,
        tag: &'a str,
        i18n: Option<&'a I18nMeta<'a>>,
        span: Span,
        children: &'a [R3Node<'a>],
        attributes: &'a [R3TextAttribute<'a>],
        parent: Option<&Parent<'a>>,
    ) {
        self.check_element(tag, i18n, span, parent);
        let this = Parent {
            tag,
            has_i18n: i18n.is_some(),
            ancestor_has_i18n: parent.is_some_and(|p| p.has_i18n || p.ancestor_has_i18n),
            start: span.start,
            has_markup_children: children.iter().any(|child| match child {
                R3Node::Element(_) => true,
                R3Node::Template(t) => is_ng_template(t),
                _ => false,
            }),
        };
        self.walk(children, Some(&this));
        for attribute in attributes {
            self.check_attribute(tag, attribute);
        }
    }

    /// Angular's lexer starts a text span after its leading trivia.
    fn skip_leading_trivia(&self, span: Span) -> Span {
        let span = self.template.span(span);
        let text = self.ctx.source_text().get(span.start as usize..span.end as usize).unwrap_or("");
        let trimmed = text.trim_start_matches([' ', '\n', '\r', '\t']);
        #[expect(clippy::cast_possible_truncation)]
        Span::new(span.end - trimmed.len() as u32, span.end)
    }

    fn is_tag_ignored(&self, tag: &str) -> bool {
        self.config.ignore_tags.iter().any(|t| t == tag)
    }

    fn check_element(
        &mut self,
        tag: &str,
        i18n: Option<&'a I18nMeta<'a>>,
        span: Span,
        parent: Option<&Parent>,
    ) {
        let config = self.config;
        if (i18n.is_none() && config.allow_markup_in_content) || self.is_tag_ignored(tag) {
            return;
        }
        let report_span = self.template.span(span);
        if parent.is_some_and(|p| p.has_i18n) {
            if !config.allow_markup_in_content {
                self.ctx.diagnostic(markup_in_content_diagnostic(report_span));
            }
            return;
        }
        // Upstream also checks elements without `i18n` here when markup is disallowed, which
        // reports every element and crashes on `requireDescription`.
        let Some(i18n) = i18n else { return };
        let (custom_id, description, meaning) = match i18n {
            // Angular trims the whole `i18n` value before splitting it.
            I18nMeta::Message(m) => {
                (m.custom_id.as_str().trim_end(), m.description.as_str(), m.meaning.as_str())
            }
            _ => ("", "", ""),
        };
        if config.check_id {
            if custom_id.is_empty() {
                self.ctx.diagnostic(custom_id_on_element_diagnostic(report_span));
            } else {
                self.collect_custom_id(custom_id, span);
            }
        }
        if config.require_description && description.is_empty() {
            self.ctx.diagnostic(missing_description_diagnostic(report_span));
        }
        if config.require_meaning && meaning.is_empty() {
            self.ctx.diagnostic(missing_meaning_diagnostic(report_span));
        }
    }

    fn check_attribute(&mut self, element: &str, attribute: &'a R3TextAttribute<'a>) {
        let config = self.config;
        if !has_letter(&attribute.value) || self.is_tag_ignored(element) {
            return;
        }
        let name = attribute.name.as_str();
        let key_span = self.template.span(attribute.key_span.unwrap_or(attribute.source_span));
        if let Some(i18n) = &attribute.i18n {
            let (custom_id, description) = match i18n {
                I18nMeta::Message(m) => (m.custom_id.as_str().trim_end(), m.description.as_str()),
                _ => ("", ""),
            };
            if config.check_id {
                if custom_id.is_empty() {
                    self.ctx.diagnostic(custom_id_on_attribute_diagnostic(name, key_span));
                } else {
                    self.collect_custom_id(custom_id, attribute.source_span);
                }
            }
            if config.require_description && description.trim().is_empty() {
                self.ctx.diagnostic(missing_description_diagnostic(key_span));
            }
            return;
        }
        if !config.check_attributes || self.is_attribute_allowed(element, name, &attribute.value) {
            return;
        }
        let end = self.template.span(attribute.source_span).end;
        self.ctx.diagnostic_with_fix(attribute_diagnostic(name, key_span), |fixer| {
            fixer.insert_text_after_range(Span::empty(end), format!(" i18n-{name}"))
        });
    }

    fn is_attribute_allowed(&self, element: &str, name: &str, value: &str) -> bool {
        let ignored = |entry: &str| {
            entry == name
                || entry
                    .strip_prefix(element)
                    .and_then(|rest| rest.strip_prefix('['))
                    .and_then(|rest| rest.strip_suffix(']'))
                    == Some(name)
        };
        DEFAULT_IGNORE_ATTRIBUTES.contains(&name)
            || self.config.ignore_attributes.iter().any(|entry| ignored(entry))
            || value.trim().is_empty()
            || value == "true"
            || value == "false"
            || is_numeric(value)
    }

    /// Text, an interpolation, or an ICU expression at `span` in the file; `bound_strings` is the
    /// static text of an interpolation.
    fn text_like(&self, span: Span, bound_strings: Option<&str>, parent: Option<&Parent>) {
        if let Some(strings) = bound_strings
            && self.is_bound_text_allowed(strings.trim())
        {
            return;
        }
        if let Some(p) = parent
            && (p.has_i18n || self.is_tag_ignored(p.tag))
        {
            return;
        }
        let diagnostic = icu_or_text_diagnostic(span);
        let Some(parent) = parent else {
            self.ctx.diagnostic_with_fix(diagnostic, |fixer| {
                fixer
                    .insert_text_before_range(Span::empty(span.start), "<ng-container i18n>")
                    .extend(fixer.insert_text_after_range(Span::empty(span.end), "</ng-container>"))
            });
            return;
        };
        let start = self.template.span(Span::empty(parent.start)).start as usize;
        let after_tag = start + 1 + parent.tag.len();
        // Upstream adds no fix under an `i18n` ancestor. The source check skips names that
        // differ from the written tag, as `:svg:` ones do.
        let source = self.ctx.source_text();
        if parent.ancestor_has_i18n
            || source.get(start..after_tag).and_then(|s| s.strip_prefix('<')) != Some(parent.tag)
            || !source[after_tag..].starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>')
        {
            self.ctx.diagnostic(diagnostic);
            return;
        }
        #[expect(clippy::cast_possible_truncation)]
        let insert = Span::empty(after_tag as u32);
        if parent.has_markup_children {
            self.ctx.diagnostic_with_suggestion(diagnostic, |fixer| {
                fixer
                    .insert_text_after_range(insert, " i18n")
                    .with_message("Add the `i18n` attribute")
            });
        } else {
            self.ctx.diagnostic_with_fix(diagnostic, |fixer| {
                fixer.insert_text_after_range(insert, " i18n")
            });
        }
    }

    fn is_bound_text_allowed(&self, text: &str) -> bool {
        !has_letter(text)
            // An empty pattern is unset upstream.
            || self.config.bound_text_allowed_pattern.as_ref().filter(|p| !p.as_str().is_empty()).map_or_else(
                // Upstream's default, `new RegExp(`[^${PL_PATTERN}]`)`, interpolates the regex
                // with its slashes into this class.
                || regex_is_match!(r"[^/\[\p{L}]/\]", text),
                |pattern| pattern.is_match(text),
            )
    }

    fn collect_custom_id(&mut self, id: &'a str, span: Span) {
        match self.custom_ids.iter_mut().find(|(collected, _)| *collected == id) {
            Some((_, spans)) => spans.push(span),
            None => self.custom_ids.push((id, vec![span])),
        }
    }

    fn report_duplicate_ids(&self) {
        if !self.config.check_duplicate_id {
            return;
        }
        for (id, spans) in &self.custom_ids {
            // As upstream: the first unique ID ends the search.
            if spans.len() <= 1 {
                break;
            }
            for span in spans {
                self.ctx.diagnostic(duplicate_custom_id_diagnostic(id, self.template.span(*span)));
            }
        }
    }
}

fn is_ng_template(template: &R3Template) -> bool {
    template.tag_name.as_ref().is_some_and(|name| name.as_str() == "ng-template")
}

/// Upstream's `PL_PATTERN`: a letter of the Basic Multilingual Plane.
fn has_letter(text: &str) -> bool {
    regex_is_match!(r"[\p{L}&&\x00-\x{FFFF}]", text)
}

/// `Number.parseFloat(value)` is finite: the value starts with a decimal number.
fn is_numeric(value: &str) -> bool {
    let value = value.trim_start();
    let value = value.strip_prefix(['+', '-']).unwrap_or(value);
    let value = value.strip_prefix('.').unwrap_or(value);
    value.starts_with(|c: char| c.is_ascii_digit())
}

#[test]
fn test() {
    use serde_json::json;

    use crate::{fixer::FixKind, tester::Tester};

    let pass = vec![
        (
            r#"
    <div>
      <span i18n="@@custom-id">Some text to translate</span>
    </div>
  "#,
            None,
        ),
        (
            r#"
    <div>
      <span class="red" i18n="@@custom-id">
        Some text to translate
      </span>
    </div>
  "#,
            None,
        ),
        (
            r#"
      <div tooltip="This tooltip property is ignored">
        <span i18n>Some text to translate</span>
      </div>
    "#,
            Some(json!([{ "checkId": false, "ignoreAttributes": ["tooltip"] }])),
        ),
        (
            r#"
      <div i18n-tooltip="@@tooltip.label" tooltip="This tooltip property is ignored">
        <span>Some text to translate</span>
      </div>
    "#,
            Some(json!([{ "checkText": false }])),
        ),
        (
            r#"
      <div i18n-tooltip="@@tooltip.label" tooltip="This tooltip property is ignored">
        <mat-icon>valid</mat-icon>
      </div>
    "#,
            Some(json!([{ "ignoreTags": ["mat-icon"] }])),
        ),
        ("<div>-{{data_from_backend}}</div>", None),
        ("<div>1{{data_from_backend}}</div>", None),
        ("<div>-1{{data_from_backend}}</div>", None),
        (
            "
      <div>
        My company untranslatable name{{data_from_backend}}
      </div>
    ",
            Some(json!([{ "boundTextAllowedPattern": "My company untranslatable name" }])),
        ),
        (
            r#"<my-component size="s"></my-component>"#,
            Some(json!([{ "ignoreTags": ["my-component"] }])),
        ),
        (
            r#"<p i18n="@@customId">Lorem ipsum <em>dolor</em> sit amet.</p>"#,
            Some(json!([{ "checkAttributes": false, "checkId": true, "checkText": false }])),
        ),
        (
            r#"
    <a
      mat-button
      ngClass="class"
      routerLink="exclusions"
      i18n="@@keywording.tools.exclusions"
    >
      Exclusions
    </a>
  "#,
            None,
        ),
        (
            r#"
      <a
        mat-button
        routerLink="exclusions"
        i18n="@@keywording.tools.exclusions"
      >
        Exclusions
      </a>
    "#,
            Some(json!([{ "checkId": true, "checkText": true }])),
        ),
        (
            "
    <ng-template #errorMessage>
      {{ error.title }}
    </ng-template>
  ",
            None,
        ),
        (
            r#"
    <ng-container i18n="@@description">
      { value, plural, =0 {No elements} =1 {111} }
    </ng-container>
  "#,
            None,
        ),
        (
            r#"
    <ng-container i18n="@@description">
      { value, plural, =0 {<div>No elements</div>} =1 {111} }
    </ng-container>
  "#,
            None,
        ),
        (
            "
      <span i18n>
        The author is {gender, select, male {male} female {female} other {other}}
      </span>
    ",
            Some(json!([{ "checkId": false }])),
        ),
        (
            r#"
    <mat-option *ngFor="let mode of modes" [value]="mode.id" i18n="@@option">
      {mode.name, select, mode {mode} other { {{mode.name}} } }
    </mat-option>
  "#,
            None,
        ),
        (r#"<ng-container ngProjectAs="top">&ngsp;</ng-container>"#, None),
        (
            r#"
      <p [ngPlural]="components">
        <ng-template ngPluralCase="1">1 component removed</ng-template>
        <ng-template ngPluralCase="1">{{components}} components removed</ng-template>
      </p>
    "#,
            Some(json!([{ "checkText": false }])),
        ),
        (
            r#"
      <div tooltip="This requires translation"></div>
      <div>
        <span i18n label="valid with i18n">Some text to translate</span>
      </div>
      <div tooltip="This requires translation" i18n-tooltip></div>
      <div>
        <ng-container>Some text&nbsp;t&#64; tr1nslate</ng-container>
      </div>
      <div>
        <span>-{{data_from_backend}}</span>
      </div>
    "#,
            Some(json!([{ "checkAttributes": false, "checkId": false, "checkText": false }])),
        ),
        (
            r#"
    <div label="1">
      <div matBadge="&#8288;">5</div>
    </div>
  "#,
            None,
        ),
        (
            r#"
    <div ariaselected="0"></div>
    <div>+</div>
    <span>&nbsp;</span>
    <span>123</span>
    <ng-content select=".content-area"></ng-content>
    <ul i18n="@@list">
      <li>ItemA</li>
      <li>ItemB</li>
      <li>ItemC</li>
    </ul>
     <ng-template i18n="@@asd">
      <div>ItemA</div>
      <div>ItemB</div>
      <div>ItemC</div>
    </ng-template>
  "#,
            None,
        ),
        (
            r#"
      <h1 i18n="An introduction header for this sample">Hello i18n!</h1>
    "#,
            Some(json!([{ "checkId": false, "requireDescription": true }])),
        ),
        (
            r#"
      <h1 i18n="An introduction header for this sample@@custom-id">Hello i18n!</h1>
    "#,
            Some(json!([{ "requireDescription": true }])),
        ),
        (
            r#"
      <h1 i18n="An introduction header for this sample" i18n-title="Title of the sample" title="Translated title">
        Hello i18n!
      </h1>
    "#,
            Some(json!([{ "checkId": false, "requireDescription": true }])),
        ),
        (
            r#"
      <h1 i18n="An introduction header for this sample@@custom-id" i18n-title="Title of the sample@@title-id" title="Translated title">
        Hello i18n!
      </h1>
    "#,
            Some(json!([{ "requireDescription": true }])),
        ),
        (
            r#"
      <img
        [src]="logo"
        i18n-title="Logo for the app"
        title="App Logo"
        i18n-alt="Translated alt logo"
        alt="Alternate logo"
      />
    "#,
            Some(json!([{ "checkId": false, "requireDescription": true }])),
        ),
        (
            r#"
      <span i18n="@@custom-id">Some text to translate</span>
      <span i18n="@@custom-id">Some text to translate</span>
    "#,
            Some(json!([{ "checkDuplicateId": false }])),
        ),
        (
            r#"
      <h1 i18n="site header|An introduction header for this sample">Hello i18n!</h1>
    "#,
            Some(json!([{ "checkId": false, "requireDescription": true, "requireMeaning": true }])),
        ),
        (
            r#"
    <ng-template i18n="@@foo"><p *ngIf="condition">Text</p></ng-template>
  "#,
            None,
        ),
        (
            r#"
      <h1 i18n="site header|">Hello i18n!</h1>
    "#,
            Some(json!([{ "checkId": false, "requireMeaning": true }])),
        ),
        (
            r#"
      <h1 i18n="site header|@@custom-id">Hello i18n!</h1>
    "#,
            Some(json!([{ "requireMeaning": true }])),
        ),
        (
            r#"
      <ng-template>Let's ignore "ng-template"</ng-template>
    "#,
            Some(json!([{ "ignoreTags": ["ng-template"] }])),
        ),
        (
            "
      <div>
      </div>
    ",
            Some(json!([{ "allowMarkupInContent": false, "checkId": false }])),
        ),
        (
            "
      <div i18n>
        Text to translate{{ Bound }}
      </div>
    ",
            Some(json!([{ "allowMarkupInContent": false, "checkId": false }])),
        ),
        (
            "
      <ng-template i18n>
        Text to translate{{ Bound }}
      </ng-template>
    ",
            Some(json!([{ "allowMarkupInContent": false, "checkId": false }])),
        ),
        (
            "
      <div i18n>
        { value, plural, =0 {No elements} =1 {111} }
      </div>
    ",
            Some(json!([{ "allowMarkupInContent": false, "checkId": false }])),
        ),
        (
            "
      <div i18n>
        { value, plural, =0 {<div>No elements</div>} =1 {111} }
      </div>
    ",
            Some(json!([{ "allowMarkupInContent": false, "checkId": false }])),
        ),
        (
            r#"
    <ng-template
      [ngIf]="userGroups?.length > 0"
      [ngIfElse]="noTeamsTmpl"
      i18n="@@cu-user-list__no-matching-users-groups"
    >
      No people or teams matched your search
    </ng-template>
    <ng-template i18n="@@cu-user-list__no-matching-users" #noTeamsTmpl>
      No people matched your search
    </ng-template>
  "#,
            None,
        ),
        (
            r#"
    <ng-template i18n="@@test">
      Text{{ Bound }}
    </ng-template>
  "#,
            None,
        ),
        (
            r#"
    <ng-template i18n="@@icu">
      Updated: {minutes, plural, =0 {just now} =1 {one minute ago} other
      {{{minutes}} minutes ago by {gender, select, male {male} female {female} other
      {other}}}}
    </ng-template>
  "#,
            None,
        ),
        (
            r#"
    <div i18n="@@foo">
        @if (a > b) {
          <span>{{a}} is greater than {{b}}</span>
        } @else if (b > a) {
          <span>{{a}} is less than {{b}}</span>
        } @else {
          <span>{{a}} is equal to {{b}}</span>
        }
    </div>
  "#,
            None,
        ),
        (
            r#"
    <ng-template i18n="@@foo">
        @if (a > b) {
          <span>{{a}} is greater than {{b}}</span>
        } @else if (b > a) {
          <span>{{a}} is less than {{b}}</span>
        } @else {
          <span>{{a}} is equal to {{b}}</span>
        }
    </ng-template>
  "#,
            None,
        ),
        (
            r#"
    <div i18n="@@foo">
        @for (item of items; track item.id) {
          <span>{{ item.name }}</span>
        } @empty {
          <span>There are no items.</span>
        }
    </div>
  "#,
            None,
        ),
        (
            r#"
    <ng-template i18n="@@foo">
        @for (item of items; track item.id) {
          <span>Item: {{ item.name }}</span>
        } @empty {
          <span>There are no items.</span>
        }
    </ng-template>
  "#,
            None,
        ),
        (
            r#"
    <div i18n="@@foo">
        @switch (condition) {
          @case (caseA) {
            <span>Case A.</span>
          }
          @case (caseB) {
            <span>Case B.</span>
          }
          @default {
            <span>Default case.</span>
          }
        }
    </div>
  "#,
            None,
        ),
        (
            r#"
    <ng-template i18n="@@foo">
        @switch (condition) {
          @case (caseA) {
            <span>Case A.</span>
          }
          @case (caseB) {
            <span>Case B.</span>
          }
          @default {
            <span>Default case.</span>
          }
        }
    </ng-template>
  "#,
            None,
        ),
        (
            r#"
    <div i18n="@@foo">
        @defer {
          <p>Large component:</p> <large-component />
        } @loading {
          <p>Loading...</p>
        } @error {
          <p>Error</p>
        } @placeholder (minimum 500ms) {
          <p>Placeholder content</p>
        }
    </div>
  "#,
            None,
        ),
        (
            r#"
    <ng-template i18n="@@foo">
        @defer {
          <p>Large component:</p> <large-component />
        } @loading {
          <p>Loading...</p>
        } @error {
          <p>Error</p>
        } @placeholder (minimum 500ms) {
          <p>Placeholder content</p>
        }
    </ng-template>
  "#,
            None,
        ),
        (
            r#"
    <ng-container i18n="@@foo">
      Foo @if (bar) { Bar <a>Bam</a> }
    </ng-container>
  "#,
            None,
        ),
        // Values that `parseFloat` reads as numbers need no translation.
        (r#"<div label="100px" title="12 items" data-x=".5em"></div>"#, None),
        (r#"<div *ngIf="x" i18n="@@a">Text</div>"#, None),
        (r#"<ng-template *ngIf="x" i18n="@@a">Text</ng-template>"#, None),
        // Only the first unique ID is looked at, as upstream.
        (r#"<p i18n="@@a">A</p><p i18n="@@b">B</p><p i18n="@@b">B</p>"#, None),
    ];

    let fail = vec![
        (r#"<div tooltip="This requires translation"></div>"#, None),
        (
            "
      <div>
        <span>test{{data_from_backend}}</span>
      </div>
    ",
            Some(json!([{ "ignoreTags": [] }])),
        ),
        ("{ value, plural, =0 {No elements} =1 {111} }", None),
        (
            "{ value, plural, =0 {<div>No elements</div>} =1 {111} }",
            Some(json!([{ "checkAttributes": false }])),
        ),
        (
            "
      <div>
        <ng-container>Some text&nbsp;t&#64; tr1nslate</ng-container>
      </div>
    ",
            Some(json!([{ "checkId": false }])),
        ),
        (r#"<p>Lorem ipsum <em i18n="@@dolor">dolor</em> sit amet.</p>"#, None),
        (r#"<div tooltip="This requires translation" i18n-tooltip></div>"#, None),
        (
            r#"
      <div>
        <span i18n label="label is ignored in 'ignoreAttributes'">
          Missing custom ID
        </span>
      </div>
    "#,
            Some(json!([{ "ignoreAttributes": ["span[label]"] }])),
        ),
        (
            r#"
      <div
        i18n-tooltip="@@custom-id"
        tooltip="This requires translation"
        label="Custom label"
        i18n-label="@@custom-id"
      ></div>
    "#,
            None,
        ),
        (
            r#"
      <h3 i18n="@@myId">Hello</h3>
      <p i18n="@@myId">Good bye</p>
    "#,
            Some(json!([{ "ignoreTags": [] }])),
        ),
        (
            r#"
      <div i18n-tooltip="@@custom-id" tooltip="This requires translation">
        <span i18n="@@custom-id">Some text to translate</span>
      </div>
    "#,
            None,
        ),
        (
            r#"
      <div i18n-tooltip="@@custom-id" tooltip="This requires translation">
        <span i18n="@@custom-id">Some text to translate</span>
      </div>
      <div i18n-label="@@custom-id" label="A label"></div>
    "#,
            None,
        ),
        (
            r#"
      <div
        tooltip="This requires translation"
        i18n-placeholder
        placeholder="More translation, please"
        class="red"
      >
        <div
          *ngIf="true"
          width="100px"
          label="Templates need translation too."
        >
          <span i18n label="label is ignored in 'ignoreAttributes'">
            Missing custom ID
          </span>
        </div>
        <ng-template i18n="@@template">
          <div>test{{binding}}</div>
        </ng-template>
      </div>
    "#,
            Some(json!([{ "ignoreAttributes": ["span[label]"] }])),
        ),
        ("<h1 i18n>Hello</h1>", Some(json!([{ "checkId": false, "requireDescription": true }]))),
        (r#"<h1 i18n="@@custom-id">Hello</h1>"#, Some(json!([{ "requireDescription": true }]))),
        (
            r#"
      <span i18n="A balloon that displays data" data-balloon="Translated title" i18n-data-balloon>
        Hello
      </span>
    "#,
            Some(json!([{ "checkId": false, "requireDescription": true }])),
        ),
        (
            r#"
      <h1 i18n="Title of the sample@@custom-id" i18n-title="@@title-id" title="Translated title">
        Hello
      </h1>
    "#,
            Some(json!([{ "requireDescription": true }])),
        ),
        (
            r#"<img [src]="logo" i18n-title title="App Logo" i18n-alt="App logo" alt="App Logo"/>"#,
            Some(json!([{ "requireDescription": true, "checkId": false }])),
        ),
        ("<h1 i18n>Hello</h1>", Some(json!([{ "checkId": false, "requireMeaning": true }]))),
        (
            r#"<h1 i18n="An introduction header for this sample">Hello</h1>"#,
            Some(json!([{ "checkId": false, "requireDescription": true, "requireMeaning": true }])),
        ),
        (
            r#"<h1 i18n="An introduction header for this sample@@custom-id">Hello</h1>"#,
            Some(json!([{ "requireDescription": true, "requireMeaning": true }])),
        ),
        (
            r#"<h1 i18n="|An introduction header for this sample">Hello</h1>"#,
            Some(json!([{ "checkId": false, "requireDescription": true, "requireMeaning": true }])),
        ),
        ("<ng-template>No people or teams matched your search</ng-template>", None),
        (
            "{ value, plural, =0 {<ng-template>No elements</ng-template>} =1 {111} }",
            Some(json!([{ "checkAttributes": false }])),
        ),
        (
            "
      <div>
        <ng-template>Some text&nbsp;t&#64; tr1nslate</ng-template>
      </div>
    ",
            Some(json!([{ "checkId": false }])),
        ),
        (
            r#"<ng-template>Lorem ipsum <ng-template i18n="@@dolor">dolor</ng-template> sit amet.</ng-template>"#,
            None,
        ),
        (
            "
      <ng-template i18n>
        The author is {gender, select, male {male} female {female} other {other}}
      </ng-template>
    ",
            None,
        ),
        (r#"<p *ngIf="condition" i18n>Text</p>"#, None),
        (
            r#"
      <h3 i18n="@@myId">Hello</h3>
      <ng-template i18n="@@myId">Good bye</ng-template>
    "#,
            Some(json!([{ "ignoreTags": [] }])),
        ),
        (
            "
        <div>
          @if (a > b) {
            <span>{{a}} is greater than {{b}}</span>
          } @else if (b > a) {
            <span>{{a}} is less than {{b}}</span>
          } @else {
            <span>{{a}} is equal to {{b}}</span>
          }
        </div>
    ",
            None,
        ),
        (
            "
        <div>
          @for (item of items; track item.id) {
            <span>Item: {{ item.name }}</span>
          } @empty {
             <span>There are no items.</span>
          }
        </div>
    ",
            None,
        ),
        (
            "
        <div>
          @switch (condition) {
            @case (caseA) {
              <span>Case A.</span>
            }
            @case (caseB) {
              <span>Case B.</span>
            }
            @default {
              <span>Default case.</span>
            }
          }
        </div>
    ",
            None,
        ),
        (
            "
        <div>
          @defer {
            <p>Large component:</p> <large-component />
          } @loading {
            <p>Loading...</p>
          } @error {
            <p>Error</p>
          } @placeholder (minimum 500ms) {
            <p>Placeholder content</p>
          }
        </div>
    ",
            None,
        ),
        (
            "
      <div i18n>
        Text to translate
        <foo></foo>
      </div>
    ",
            Some(json!([{ "allowMarkupInContent": false, "checkId": false }])),
        ),
        (
            "
      <ng-template i18n>
        Text to translate
        <foo></foo>
      </ng-template>
    ",
            Some(json!([{ "allowMarkupInContent": false, "checkId": false }])),
        ),
        (
            "
    <div i18n>
      <div i18n>
        Text to translate
      </div>
    </div>
    ",
            Some(json!([{ "allowMarkupInContent": false, "checkId": false }])),
        ),
        // Reported once, on the element rather than on the structural directive's template.
        (r#"<div *ngIf="x" title="Hello">Text</div>"#, Some(json!([{ "checkId": false }]))),
        (r#"<ng-template *ngIf="x">Text</ng-template>"#, None),
        (r#"<ng-template i18n="@@a"></ng-template><p i18n="@@a"></p>"#, None),
        // `boundTextAllowedPattern` only covers interpolated text.
        (
            "<div>My company untranslatable name</div>",
            Some(json!([{ "boundTextAllowedPattern": "My company untranslatable name" }])),
        ),
        // `<ng-content>` is not an element: its text belongs to the enclosing one.
        ("<ng-content>Fallback</ng-content>", None),
        // An empty pattern falls back to the default.
        ("<div>test{{x}}</div>", Some(json!([{ "boundTextAllowedPattern": "" }]))),
        // Custom IDs keep their leading whitespace on elements and attributes alike.
        (r#"<p i18n="@@ a">A</p><p title="T" i18n-title="@@ a"></p>"#, None),
        // A structural directive's wrapper is not markup, so the outer text gets a fix.
        (r#"<p>Hello <span *ngIf="x">a</span></p>"#, None),
    ];

    let fix = vec![
        (
            r#"<div tooltip="This requires translation"></div>"#,
            r#"<div tooltip="This requires translation" i18n-tooltip></div>"#,
            None,
            FixKind::SafeFix,
        ),
        (
            "<div><span>test{{data_from_backend}}</span></div>",
            "<div><span i18n>test{{data_from_backend}}</span></div>",
            None,
            FixKind::SafeFix,
        ),
        (
            "{ value, plural, =0 {No elements} =1 {111} }",
            "<ng-container i18n>{ value, plural, =0 {No elements} =1 {111} }</ng-container>",
            None,
            FixKind::SafeFix,
        ),
        (
            "{ value, plural, =0 {<div>No elements</div>} =1 {111} }",
            "<ng-container i18n>{ value, plural, =0 {<div>No elements</div>} =1 {111} }</ng-container>",
            Some(json!([{ "checkAttributes": false }])),
            FixKind::SafeFix,
        ),
        (
            "<div><ng-container>Some text&nbsp;t&#64; tr1nslate</ng-container></div>",
            "<div><ng-container i18n>Some text&nbsp;t&#64; tr1nslate</ng-container></div>",
            Some(json!([{ "checkId": false }])),
            FixKind::SafeFix,
        ),
        (
            r#"<p>Lorem ipsum <em i18n="@@dolor">dolor</em></p>"#,
            r#"<p i18n>Lorem ipsum <em i18n="@@dolor">dolor</em></p>"#,
            None,
            FixKind::Suggestion,
        ),
        (
            r#"<p>Lorem ipsum <em i18n="@@dolor">dolor</em></p>"#,
            r#"<p>Lorem ipsum <em i18n="@@dolor">dolor</em></p>"#,
            None,
            FixKind::SafeFix,
        ),
        (
            r#"<div tooltip="A" i18n-placeholder placeholder="B" class="red"><div *ngIf="true" width="100px" label="C"><span i18n label="D">Missing custom ID</span></div></div>"#,
            r#"<div tooltip="A" i18n-tooltip i18n-placeholder placeholder="B" class="red"><div *ngIf="true" width="100px" label="C" i18n-label><span i18n label="D">Missing custom ID</span></div></div>"#,
            Some(json!([{ "ignoreAttributes": ["span[label]"] }])),
            FixKind::SafeFix,
        ),
        (
            "<ng-template>No people or teams matched your search</ng-template>",
            "<ng-template i18n>No people or teams matched your search</ng-template>",
            None,
            FixKind::SafeFix,
        ),
        (
            "{ value, plural, =0 {<ng-template>No elements</ng-template>} =1 {111} }",
            "<ng-container i18n>{ value, plural, =0 {<ng-template>No elements</ng-template>} =1 {111} }</ng-container>",
            Some(json!([{ "checkAttributes": false }])),
            FixKind::SafeFix,
        ),
        (
            r#"<ng-template>Lorem ipsum <ng-template i18n="@@dolor">dolor</ng-template></ng-template>"#,
            r#"<ng-template i18n>Lorem ipsum <ng-template i18n="@@dolor">dolor</ng-template></ng-template>"#,
            None,
            FixKind::Suggestion,
        ),
        (
            "<div>@if (a > b) {<span>{{a}} is greater than {{b}}</span>} @else {<span>{{a}} is equal to {{b}}</span>}</div>",
            "<div>@if (a > b) {<span i18n>{{a}} is greater than {{b}}</span>} @else {<span i18n>{{a}} is equal to {{b}}</span>}</div>",
            None,
            FixKind::SafeFix,
        ),
        (
            "<div>@for (item of items; track item.id) {<span>Item: {{ item.name }}</span>} @empty {<span>There are no items.</span>}</div>",
            "<div>@for (item of items; track item.id) {<span i18n>Item: {{ item.name }}</span>} @empty {<span i18n>There are no items.</span>}</div>",
            None,
            FixKind::SafeFix,
        ),
        (
            "<div>@switch (c) {@case (a) {<span>Case A.</span>} @default {<span>Default case.</span>}}</div>",
            "<div>@switch (c) {@case (a) {<span i18n>Case A.</span>} @default {<span i18n>Default case.</span>}}</div>",
            None,
            FixKind::SafeFix,
        ),
        (
            "<div>@defer {<p>Large component:</p>} @loading {<p>Loading...</p>} @error {<p>Error</p>} @placeholder (minimum 500ms) {<p>Placeholder content</p>}</div>",
            "<div>@defer {<p i18n>Large component:</p>} @loading {<p i18n>Loading...</p>} @error {<p i18n>Error</p>} @placeholder (minimum 500ms) {<p i18n>Placeholder content</p>}</div>",
            None,
            FixKind::SafeFix,
        ),
        (
            r#"<div *ngIf="x">Text</div>"#,
            r#"<div i18n *ngIf="x">Text</div>"#,
            None,
            FixKind::SafeFix,
        ),
        (
            r#"<ng-template *ngIf="x">Text</ng-template>"#,
            r#"<ng-template i18n *ngIf="x">Text</ng-template>"#,
            None,
            FixKind::SafeFix,
        ),
        // The wrapper sits after the text's leading whitespace.
        (
            "\n   Hello {{x}}\n",
            "\n   <ng-container i18n>Hello {{x}}\n</ng-container>",
            None,
            FixKind::SafeFix,
        ),
        (
            r#"<p>Hello <ng-template *ngIf="x">a</ng-template></p>"#,
            r#"<p i18n>Hello <ng-template i18n *ngIf="x">a</ng-template></p>"#,
            None,
            FixKind::SafeFix,
        ),
        ("<DIV>Text</DIV>", "<DIV i18n>Text</DIV>", None, FixKind::SafeFix),
        // A namespaced element's name differs from its tag, so it gets no fix.
        ("<svg>Text</svg>", "<svg>Text</svg>", None, FixKind::SafeFix),
    ];

    Tester::new(I18n::NAME, I18n::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<span i18n=\"@@a\">Text</span>' }) class A {}",
        // Custom IDs are unique per template, as each inline template is linted on its own.
        "import { Component } from '@angular/core'; @Component({ template: '<p i18n=\"@@a\">A</p>' }) class A {} @Component({ template: '<p i18n=\"@@a\">B</p>' }) class B {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: `<div>\n  <span>Text</span>\n</div>` }) class A {}",
    ];
    let inline_fix = vec![(
        "import { Component } from '@angular/core'; @Component({ template: '<span>Text</span>' }) class A {}",
        "import { Component } from '@angular/core'; @Component({ template: '<span i18n>Text</span>' }) class A {}",
    )];
    Tester::new(I18n::NAME, I18n::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .expect_fix(inline_fix)
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
