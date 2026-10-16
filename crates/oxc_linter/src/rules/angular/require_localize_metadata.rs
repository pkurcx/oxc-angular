use lazy_regex::Regex;
use oxc_ast::{AstKind, ast::TemplateElement};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::{ContextHost, LintContext},
    rule::{DefaultRuleConfig, Rule},
    utils::deserialize_regex,
};

const METADATA_LINK: &str = "https://angular.dev/guide/i18n/prepare#i18n-metadata-for-translation";

fn require_localize_description_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("$localize tagged messages should contain a description.")
        .with_help(format!("Add a description, as in `:description:text`. See {METADATA_LINK}"))
        .with_label(span)
}

fn require_localize_meaning_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("$localize tagged messages should contain a meaning.")
        .with_help(format!("Add a meaning, as in `:meaning|description:text`. See {METADATA_LINK}"))
        .with_label(span)
}

fn require_localize_custom_id_diagnostic(span: Span, pattern_message: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "$localize tagged messages should contain a custom id{pattern_message}."
    ))
    .with_help(format!("Add a custom id, as in `:@@custom-id:text`. See {METADATA_LINK}"))
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct RequireLocalizeMetadata(Box<RequireLocalizeMetadataConfig>);

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
#[expect(clippy::struct_field_names)]
pub struct RequireLocalizeMetadataConfig {
    /// Require a description, as in `` $localize`:description:text` ``.
    require_description: bool,
    /// Require a meaning, as in `` $localize`:meaning|description:text` ``.
    require_meaning: bool,
    /// Require a custom id, as in `` $localize`:@@custom-id:text` ``. A string also requires the
    /// id to match it as a regular expression.
    require_custom_id: RequireCustomId,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
enum RequireCustomId {
    Enabled(bool),
    Pattern(#[serde(deserialize_with = "deserialize_regex")] Regex),
}

impl Default for RequireCustomId {
    fn default() -> Self {
        Self::Enabled(false)
    }
}

impl RequireLocalizeMetadataConfig {
    // An empty pattern is falsy upstream, so it disables the check like `false`.
    fn requires_custom_id(&self) -> bool {
        match &self.require_custom_id {
            RequireCustomId::Enabled(enabled) => *enabled,
            RequireCustomId::Pattern(pattern) => !pattern.as_str().is_empty(),
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that `$localize` tagged messages contain the metadata that helps translators:
    /// a description, a meaning and a custom id, as configured.
    ///
    /// ### Why is this bad?
    ///
    /// Without a description or meaning, translators lack the context to pick the right
    /// translation, and identical text that needs different translations cannot be told apart.
    /// Without a custom id, the generated id changes with the text and forces retranslation.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule with `{ "requireDescription": true }`:
    /// ```ts
    /// const title = $localize`Hello i18n!`;
    /// ```
    ///
    /// Examples of **correct** code for this rule with `{ "requireDescription": true }`:
    /// ```ts
    /// const title = $localize`:An introduction header for this sample:Hello i18n!`;
    /// ```
    RequireLocalizeMetadata,
    angular,
    pedantic,
    none,
    config = RequireLocalizeMetadataConfig,
    version = "next",
    short_description = "Require description, meaning or custom id metadata in `$localize` tagged messages.",
);

impl Rule for RequireLocalizeMetadata {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn should_run(&self, _ctx: &ContextHost) -> bool {
        let config = &self.0;
        config.require_description || config.require_meaning || config.requires_custom_id()
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::TaggedTemplateExpression(tagged) = node.kind() else { return };
        if !tagged.tag.is_specific_id("$localize") {
            return;
        }
        let Some(element) = tagged.quasi.quasis.first() else { return };
        let metadata = parse_metadata(element.value.raw.as_str().trim());
        let span = ts_template_element_span(element);
        let config = &self.0;

        if config.require_description && metadata.description.is_none() {
            ctx.diagnostic(require_localize_description_diagnostic(span));
        }
        if config.require_meaning && metadata.meaning.is_none() {
            ctx.diagnostic(require_localize_meaning_diagnostic(span));
        }
        if !config.requires_custom_id() {
            return;
        }
        let custom_id = metadata.custom_id;
        match &config.require_custom_id {
            RequireCustomId::Pattern(pattern) => {
                if !custom_id.is_some_and(|id| pattern.is_match(id)) {
                    let pattern_message = format!(
                        " matching the pattern /{}/ on '{}'",
                        pattern.as_str(),
                        custom_id.unwrap_or("undefined")
                    );
                    ctx.diagnostic(require_localize_custom_id_diagnostic(span, &pattern_message));
                }
            }
            RequireCustomId::Enabled(_) => {
                if custom_id.is_none() {
                    ctx.diagnostic(require_localize_custom_id_diagnostic(span, ""));
                }
            }
        }
    }
}

// TS-ESLint's `TemplateElement` range includes the backtick and the `${` that delimit it.
fn ts_template_element_span(element: &TemplateElement) -> Span {
    Span::new(element.span.start - 1, element.span.end + if element.tail { 1 } else { 2 })
}

// Empty parts count as missing, as they are falsy upstream.
#[derive(Default)]
struct Metadata<'s> {
    meaning: Option<&'s str>,
    description: Option<&'s str>,
    custom_id: Option<&'s str>,
}

// Port of `parseMetadata` from `@angular/localize`; JS `split(sep, 2)` keeps the first two
// pieces, which `splitn` would not.
fn parse_metadata(raw: &str) -> Metadata<'_> {
    let Some(rest) = raw.strip_prefix(':') else { return Metadata::default() };
    let text = &rest[..rest.rfind(':').unwrap_or(0)];

    let mut id_parts = text.split("@@");
    let meaning_and_description = id_parts.next().unwrap_or_default();
    let custom_id = id_parts.next();

    let mut parts = meaning_and_description.split('|');
    let first = parts.next();
    let (meaning, description) = match parts.next() {
        Some(description) => (first, Some(description)),
        None => (None, first),
    };

    Metadata {
        meaning: meaning.filter(|s| !s.is_empty()),
        description: description.filter(|s| !s.is_empty()),
        custom_id: custom_id.filter(|s| !s.is_empty()),
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        ("const localizedText = $localize`Hello i18n!`;", None),
        ("const localizedText = $localize`:site header|:Hello i18n!`;", None),
        ("const localizedText = $localize`:@@custom_id:Hello i18n!`;", None),
        ("const localizedText = $localize`:site header|@@custom_id:Hello i18n!`;", None),
        (
            "let localizedText = $localize`:An introduction header for this sample:Hello i18n!`;
localizedText = $localize`:An introduction header for this sample modified:Hello i18n modified!`;",
            Some(json!([{ "requireDescription": true }])),
        ),
        (
            "const localizedTexts = {
  helloI18n: $localize`:An introduction header for this sample:Hello i18n!`
};
localizedTexts.helloI18n = $localize`:An introduction header for this sample modified:Hello i18n modified!`;",
            Some(json!([{ "requireDescription": true }])),
        ),
        (
            "return $localize`:An introduction header for this sample:Hello i18n!`;",
            Some(json!([{ "requireDescription": true }])),
        ),
        (
            "someFunction($localize`:An introduction header for this sample:Hello i18n!`);",
            Some(json!([{ "requireDescription": true }])),
        ),
        ("const localizedText = `Hello i18n!`;", Some(json!([{ "requireDescription": true }]))),
        (
            "const localizedText = $localize`:site header|:Hello i18n!`;",
            Some(json!([{ "requireMeaning": true }])),
        ),
        (
            "const localizedText = $localize`:site header|An introduction header for this sample:Hello i18n!`;",
            Some(json!([{ "requireDescription": true, "requireMeaning": true }])),
        ),
        (
            "const localizedText = $localize`:@@some.custom.id:Hello i18n!`;",
            Some(json!([{ "requireCustomId": true }])),
        ),
        (
            "const localizedText = $localize`:@@some.custom.id:Hello i18n!`;",
            Some(json!([{ "requireCustomId": "^some.*id$" }])),
        ),
    ];

    let fail = vec![
        (
            "const localizedText = $localize`Hello i18n!`;",
            Some(json!([{ "requireDescription": true }])),
        ),
        (
            "const localizedTexts = {
  helloI18n: $localize`:An introduction header for this sample:Hello i18n!`
};
localizedTexts.helloI18n = $localize`Hello i18n!`;",
            Some(json!([{ "requireDescription": true }])),
        ),
        ("return $localize`Hello i18n!`;", Some(json!([{ "requireDescription": true }]))),
        ("someFunction($localize`Hello i18n!`);", Some(json!([{ "requireDescription": true }]))),
        (
            "const localizedText = $localize`:site header|:Hello i18n!`;",
            Some(json!([{ "requireDescription": true }])),
        ),
        (
            "const localizedText = $localize`:@@custom_id:Hello i18n!`;",
            Some(json!([{ "requireDescription": true }])),
        ),
        (
            "const localizedText = $localize`:site header|@@custom_id:Hello i18n!`;",
            Some(json!([{ "requireDescription": true }])),
        ),
        (
            "const localizedText = $localize`Hello i18n!`;",
            Some(json!([{ "requireMeaning": true }])),
        ),
        (
            "const localizedText = $localize`:An introduction header for this sample:Hello i18n!`;",
            Some(json!([{ "requireDescription": true, "requireMeaning": true }])),
        ),
        (
            "const localizedText = $localize`:|An introduction header for this sample:Hello i18n!`;",
            Some(json!([{ "requireMeaning": true }])),
        ),
        (
            "const localizedText = $localize`:Hello i18n!`;",
            Some(json!([{ "requireDescription": true, "requireMeaning": true }])),
        ),
        (
            "const localizedText = $localize`Hello i18n!`;",
            Some(json!([{ "requireCustomId": true }])),
        ),
        (
            "const localizedText = $localize`:@some.custom.id:Hello i18n!`;",
            Some(json!([{ "requireCustomId": true }])),
        ),
        (
            "const localizedText = $localize`:meaning|description:Hello i18n!`;",
            Some(json!([{ "requireCustomId": true }])),
        ),
        (
            "const localizedText = $localize`:@@some.custom.id:Hello i18n!`;",
            Some(json!([{ "requireCustomId": "^some.wrong.pattern$" }])),
        ),
    ];

    Tester::new(RequireLocalizeMetadata::NAME, RequireLocalizeMetadata::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
