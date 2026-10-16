use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{
        SelectorStyle, class_decorator, decorator_metadata, has_selector_prefix, literal_text,
        metadata_property, strip_selector_prefix, to_human_readable_text,
    },
};

fn pipe_prefix_diagnostic(span: Span, prefixes: &[String]) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "@Pipes should be prefixed with {}",
        to_human_readable_text(prefixes)
    ))
    .with_label(span)
}

fn selector_after_prefix_diagnostic(span: Span, prefixes: &[String]) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "@Pipes should have a selector after the {} prefix",
        to_human_readable_text(prefixes)
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PipePrefixConfig {
    /// Prefixes a pipe `name` may start with, followed by a non-lowercase character. The rule
    /// does nothing while the list is empty.
    prefixes: Vec<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct PipePrefix(Box<PipePrefixConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the `name` of every `@Pipe` to start with one of the configured prefixes, in
    /// camelCase, with something after the prefix. The rule does nothing until `prefixes` is set.
    ///
    /// ### Why is this bad?
    ///
    /// Unprefixed pipe names can collide with Angular's built-in pipes and with pipes from other
    /// libraries, and make it harder to tell which pipes belong to the application.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule with `{ "prefixes": ["app"] }`:
    /// ```ts
    /// @Pipe({ name: 'currency' })
    /// class CurrencyPipe {}
    /// ```
    ///
    /// Examples of **correct** code for this rule with `{ "prefixes": ["app"] }`:
    /// ```ts
    /// @Pipe({ name: 'appCurrency' })
    /// class CurrencyPipe {}
    /// ```
    PipePrefix,
    angular,
    style,
    config = PipePrefixConfig,
    version = "next",
    short_description = "Enforce a consistent prefix for pipe names.",
);

impl Rule for PipePrefix {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        let prefixes = &self.0.prefixes;
        if prefixes.is_empty() {
            return;
        }
        let Some(name) = class_decorator(class, "Pipe")
            .and_then(decorator_metadata)
            .and_then(|metadata| metadata_property(metadata, "name"))
            .map(|prop| &prop.value)
        else {
            return;
        };
        let Some(text) = literal_text(name).filter(|text| !text.is_empty()) else { return };

        let span = name.span();
        let mut matched = prefixes
            .iter()
            .filter(|prefix| has_selector_prefix(text, prefix, SelectorStyle::CamelCase))
            .peekable();
        if matched.peek().is_none() {
            ctx.diagnostic(pipe_prefix_diagnostic(span, prefixes));
            return;
        }
        if matched.all(|prefix| strip_selector_prefix(text, prefix) == Some("")) {
            ctx.diagnostic(selector_after_prefix_diagnostic(span, prefixes));
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        ("@Pipe class Test {}", Some(serde_json::json!([{ "prefixes": ["ng"] }]))),
        ("@Pipe({}) class Test {}", Some(serde_json::json!([{ "prefixes": ["ng"] }]))),
        (
            "export function mockPipe(name: string): any {
              @Pipe({ name })
              class MockPipe implements PipeTransform {
                transform(input: any): any {
                  return input;
                }
              }
              return MockPipe;
            }",
            Some(serde_json::json!([{ "prefixes": ["ng"] }])),
        ),
        (
            "@Pipe({ name: 'ngBarFoo' }) class Test {}",
            Some(serde_json::json!([{ "prefixes": [] }])),
        ),
        (
            "@Pipe({ name: 'ngBarFoo' }) class Test {}",
            Some(serde_json::json!([{ "prefixes": ["ng"] }])),
        ),
        (
            "@Pipe({ name: 'ngBarFoo' }) class Test {}",
            Some(serde_json::json!([{ "prefixes": ["ng", "sg", "mg"] }])),
        ),
        (
            "@Pipe({ name: `ngBarFoo` }) class Test {}",
            Some(serde_json::json!([{ "prefixes": ["ng", "sg", "mg"] }])),
        ),
        ("class Test {}", Some(serde_json::json!([{ "prefixes": ["ng"] }]))),
        (
            "const pipeName = 'fooBar';
            @Pipe({ name: pipeName }) class Test {}",
            Some(serde_json::json!([{ "prefixes": ["ng"] }])),
        ),
    ];

    let fail = vec![
        (
            "@Pipe({ name: 'foo-bar' }) class Test {}",
            Some(serde_json::json!([{ "prefixes": ["ng"] }])),
        ),
        ("@Pipe({ name: 'ng' }) class Test {}", Some(serde_json::json!([{ "prefixes": ["ng"] }]))),
        (
            "@Pipe({ name: 'foo-bar' }) class Test {}",
            Some(serde_json::json!([{ "prefixes": ["ng", "mg", "sg"] }])),
        ),
    ];

    Tester::new(PipePrefix::NAME, PipePrefix::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
