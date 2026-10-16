use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{
        SelectorOptions, SelectorStyle, check_selector, class_decorator, decorator_metadata,
        literal_text, metadata_property, parse_selector, to_human_readable_text,
    },
};

const STYLE_GUIDE_HELP: &str = "See https://angular.dev/style-guide#choosing-directive-selectors";

fn prefix_diagnostic(span: Span, prefixes: &[String]) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The selector should start with one of these prefixes: {}",
        to_human_readable_text(prefixes)
    ))
    .with_help(STYLE_GUIDE_HELP)
    .with_label(span)
}

fn style_diagnostic(span: Span, style: SelectorStyle) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("The selector should be {}", style.as_str()))
        .with_help(STYLE_GUIDE_HELP)
        .with_label(span)
}

fn type_diagnostic(span: Span, kind: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("The selector should be used as an {kind}"))
        .with_help(STYLE_GUIDE_HELP)
        .with_label(span)
}

fn selector_after_prefix_diagnostic(span: Span, prefixes: &[String]) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "There should be a selector after the {} prefix",
        to_human_readable_text(prefixes)
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct DirectiveSelector(Box<SelectorOptions>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces the type (element or attribute), prefix and style (`kebab-case` or `camelCase`)
    /// of `@Directive` selectors. The rule does nothing until it is configured.
    ///
    /// ### Why is this bad?
    ///
    /// Inconsistent selectors make it harder to tell application directives from third-party
    /// ones, and unprefixed selectors can collide with native attributes and other directives.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule with
    /// `{ "type": "attribute", "prefix": "app", "style": "camelCase" }`:
    /// ```ts
    /// @Directive({ selector: '[highlight]' })
    /// class HighlightDirective {}
    /// ```
    ///
    /// Examples of **correct** code for this rule with
    /// `{ "type": "attribute", "prefix": "app", "style": "camelCase" }`:
    /// ```ts
    /// @Directive({ selector: '[appHighlight]' })
    /// class HighlightDirective {}
    /// ```
    DirectiveSelector,
    angular,
    style,
    config = SelectorOptions,
    version = "next",
    short_description = "Enforce naming rules for `@Directive` selectors.",
);

impl Rule for DirectiveSelector {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        let Some(metadata) = class_decorator(class, "Directive").and_then(decorator_metadata)
        else {
            return;
        };
        let Some(selector) = metadata_property(metadata, "selector").map(|prop| &prop.value) else {
            return;
        };
        let Some(text) = literal_text(selector) else { return };
        let selectors = parse_selector(text);
        if selectors.is_empty() {
            return;
        }
        let Some(config) = self.0.applicable_config(&selectors) else { return };

        let span = selector.span();
        let check = check_selector(&selectors, &config);
        let diagnostic = if !check.has_expected_type {
            type_diagnostic(span, config.kind.as_str())
        } else if !check.has_selector_after_prefix {
            selector_after_prefix_diagnostic(span, &config.prefixes)
        } else if !check.has_expected_style {
            style_diagnostic(span, config.style)
        } else if !check.has_expected_prefix {
            prefix_diagnostic(span, &config.prefixes)
        } else {
            return;
        };
        ctx.diagnostic(diagnostic);
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        (
            "",
            Some(serde_json::json!([{ "type": "element", "prefix": "sg", "style": "kebab-case" }])),
        ),
        (
            "@Directive({ selector: 'app-foo-bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: '[app-foo-bar]' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["app", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'app-foo-bar[baz].app' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "cd", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'app-bar' }) class TestOne {}
            @Directive({ selector: 'ngg-bar' }) class TestTwo {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "cd", "ngg"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'appBarFoo' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "prefix": "app", "style": "camelCase" }])),
        ),
        (
            "@Directive({ selector: 'app1-foo-bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app1", "style": "kebab-case" }]),
            ),
        ),
        (
            "const selectorName = 'appFooBar';
            @Directive({ selector: selectorName }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        (
            "const selector = 'appFooBar';
            @Directive({ selector, }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: `[app-foo-bar]` }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["app", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'baz-[app-bar-foo][foe].bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["app", "baz"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'app-bar-foo[baz].bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'app-foo-bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": ["element"], "prefix": ["bar"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: '[fooBar]' }) class Test {}",
            Some(serde_json::json!([{ "type": "attribute", "style": "camelCase", "prefix": [] }])),
        ),
        (
            "@Directive({ selector: '[fooBar]' }) class Test {}",
            Some(serde_json::json!([{ "type": "attribute", "style": "camelCase", "prefix": "" }])),
        ),
        (
            "@Directive({
              selector: `
                [app-foo-bar]
              `
            })
            class Test {}",
            Some(
                serde_json::json!([{ "type": ["attribute"], "prefix": ["app"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({
              selector: `
                [app-foo-bar],
                [app-bar-foo]
              `
            })
            class Test {}",
            Some(
                serde_json::json!([{ "type": ["attribute"], "prefix": ["app"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'button[app-foo-bar]' }) class Test {}",
            Some(
                serde_json::json!([{ "type": ["attribute"], "prefix": ["app"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'app-foo-bar' }) class Test {}",
            Some(
                serde_json::json!([[{ "type": "element", "prefix": "app", "style": "kebab-case" }]]),
            ),
        ),
        (
            "@Directive({ selector: '[appFooBar]' }) class Test {}",
            Some(
                serde_json::json!([[{ "type": "attribute", "prefix": "app", "style": "camelCase" }]]),
            ),
        ),
        (
            "@Directive({ selector: 'app-foo-bar' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Directive({ selector: '[appFooBar]' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Directive({ selector: 'lib-foo-bar' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": ["app", "lib"], "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Directive({ selector: '[libFooBar]' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": ["app", "lib"], "style": "camelCase" },
            ]])),
        ),
        (
            "@Directive({ selector: '[appFooBar]' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
                { "type": "element", "prefix": "app", "style": "kebab-case" },
            ]])),
        ),
    ];

    let fail = vec![
        (
            "@Directive({ selector: 'app-foo-bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "bar", "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: '[app-foo-bar]' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["cd", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'app-foo-bar[baz].app' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["foo", "cd", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: '[app-bar-foo]' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": "app", "style": "camelCase" }]),
            ),
        ),
        (
            "@Directive({ selector: 'appFooBar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'app' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'app' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "prefix": "app", "style": "camelCase" }])),
        ),
        (
            "@Directive({ selector: `app-foo-bar` }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["app", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: '[appFooBar]' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "ng"], "style": "camelCase" }]),
            ),
        ),
        (
            "@Directive({ selector: 'appFooBar' }) class Test {}",
            Some(
                serde_json::json!([[{ "type": "element", "prefix": "app", "style": "kebab-case" }]]),
            ),
        ),
        (
            "@Directive({ selector: '[app-foo-bar]' }) class Test {}",
            Some(
                serde_json::json!([[{ "type": "attribute", "prefix": "app", "style": "camelCase" }]]),
            ),
        ),
        (
            "@Directive({ selector: 'appFooBar' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Directive({ selector: '[app-foo-bar]' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Directive({ selector: 'lib-foo-bar' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Directive({ selector: '[libFooBar]' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
    ];

    Tester::new(DirectiveSelector::NAME, DirectiveSelector::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
