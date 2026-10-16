use oxc_ast::{AstKind, ast::ObjectExpression};
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

const STYLE_GUIDE_HELP: &str = "See https://angular.dev/style-guide#choosing-component-selectors";

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

fn style_and_prefix_diagnostic(
    span: Span,
    style: SelectorStyle,
    prefixes: &[String],
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The selector should be {} and start with one of these prefixes: {}",
        style.as_str(),
        to_human_readable_text(prefixes)
    ))
    .with_help(STYLE_GUIDE_HELP)
    .with_label(span)
}

fn type_diagnostic(span: Span, kind: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("The selector should be used as an {kind}"))
        .with_help(STYLE_GUIDE_HELP)
        .with_label(span)
}

fn shadow_dom_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The selector of a ShadowDom-encapsulated component should be `kebab-case`")
        .with_help("Custom element names must contain a hyphen.")
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
pub struct ComponentSelector(Box<SelectorOptions>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces the type (element or attribute), prefix and style (`kebab-case` or `camelCase`)
    /// of `@Component` selectors. Components with `ViewEncapsulation.ShadowDom` always need a
    /// `kebab-case` element selector. The rule does nothing until it is configured.
    ///
    /// ### Why is this bad?
    ///
    /// Inconsistent selectors make components harder to identify in templates and DevTools, and
    /// unprefixed selectors can collide with native elements and third-party components.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule with
    /// `{ "type": "element", "prefix": "app", "style": "kebab-case" }`:
    /// ```ts
    /// @Component({ selector: 'appFooBar' })
    /// class FooBarComponent {}
    /// ```
    ///
    /// Examples of **correct** code for this rule with
    /// `{ "type": "element", "prefix": "app", "style": "kebab-case" }`:
    /// ```ts
    /// @Component({ selector: 'app-foo-bar' })
    /// class FooBarComponent {}
    /// ```
    ComponentSelector,
    angular,
    style,
    config = SelectorOptions,
    version = "next",
    short_description = "Enforce naming rules for `@Component` selectors.",
);

impl Rule for ComponentSelector {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        let Some(metadata) = class_decorator(class, "Component").and_then(decorator_metadata)
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
        let Some(mut config) = self.0.applicable_config(&selectors) else { return };

        let span = selector.span();
        let style = config.style;
        let shadow_dom =
            style != SelectorStyle::KebabCase && has_shadow_dom_encapsulation(metadata);
        if shadow_dom {
            let has_hyphen = selectors
                .iter()
                .any(|selector| selector.element.as_deref().is_some_and(|el| el.contains('-')));
            if !has_hyphen {
                ctx.diagnostic(shadow_dom_diagnostic(span));
                return;
            }
            config.style = SelectorStyle::KebabCase;
        }

        let check = check_selector(&selectors, &config);
        let diagnostic = if !check.has_expected_type {
            type_diagnostic(span, config.kind.as_str())
        } else if !check.has_selector_after_prefix {
            selector_after_prefix_diagnostic(span, &config.prefixes)
        } else if !check.has_expected_style {
            if shadow_dom {
                shadow_dom_diagnostic(span)
            } else if check.has_expected_prefix {
                style_diagnostic(span, style)
            } else {
                style_and_prefix_diagnostic(span, style, &config.prefixes)
            }
        } else if !check.has_expected_prefix {
            prefix_diagnostic(span, &config.prefixes)
        } else {
            return;
        };
        ctx.diagnostic(diagnostic);
    }
}

fn has_shadow_dom_encapsulation(metadata: &ObjectExpression) -> bool {
    metadata_property(metadata, "encapsulation")
        .is_some_and(|prop| prop.value.is_specific_member_access("ViewEncapsulation", "ShadowDom"))
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
            "@Component({ selector: 'sg-foo-bar' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "prefix": "sg", "style": "kebab-case" }])),
        ),
        (
            "@Component({ selector: '[ng-foo-bar]' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["app", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'app-foo-bar[baz].app' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "cd", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'app-bar' }) class TestOne {}
            @Component({ selector: 'ngg-bar' }) class TestTwo {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "cd", "ngg"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'appBarFoo' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "prefix": "app", "style": "camelCase" }])),
        ),
        (
            "@Component({ selector: 'app1-foo-bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app1", "style": "kebab-case" }]),
            ),
        ),
        (
            "const selectorName = 'appFooBar';
            @Component({ selector: selectorName }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        (
            "const selecto = 'appFooBar';
            @Component({ selector, }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'app-foo-bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'baz-[app-bar-foo][foe].bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["app", "baz"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'app-bar-foo[baz].bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: `[appFooBar]` }) class Test {}",
            Some(
                serde_json::json!([{ "type": ["attribute", "element"], "prefix": ["app", "ng"], "style": "camelCase" }]),
            ),
        ),
        (
            "@Component({
              selector: `
                [appFooBar]
              `
            })
            class Test {}",
            Some(
                serde_json::json!([{ "type": ["attribute", "element"], "prefix": ["app", "ng"], "style": "camelCase" }]),
            ),
        ),
        (
            "@Component({
              selector: `
                [appFooBar],
                [appBarFoo]
              `
            })
            class Test {}",
            Some(
                serde_json::json!([{ "type": ["attribute", "element"], "prefix": ["app", "ng"], "style": "camelCase" }]),
            ),
        ),
        (
            "@Component({ selector: `button[appFooBar]` }) class Test {}",
            Some(
                serde_json::json!([{ "type": ["attribute", "element"], "prefix": ["app", "ng"], "style": "camelCase" }]),
            ),
        ),
        (
            "@Component({
              selector: `app-foo-bar`,
              encapsulation: ViewEncapsulation.ShadowDom
            })
            class Test {}",
            Some(
                serde_json::json!([{ "type": ["element"], "prefix": ["app"], "style": "camelCase" }]),
            ),
        ),
        (
            "@Component({
              selector: `app-foo-bar`,
              encapsulation: ViewEncapsulation.ShadowDom
            })
            class Test {}",
            Some(
                serde_json::json!([{ "type": ["element"], "prefix": ["app"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Directive({ selector: 'app-foo-bar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": ["element"], "prefix": ["bar"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'singleword' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "style": "kebab-case", "prefix": "" }])),
        ),
        (
            "@Component({ selector: 'foo-bar' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "style": "kebab-case", "prefix": [] }])),
        ),
        (
            "@Component({ selector: 'any-name-without-prefix' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "style": "kebab-case", "prefix": "" }])),
        ),
        (
            "@Component({ selector: 'app-foo-bar' }) class Test {}",
            Some(
                serde_json::json!([[{ "type": "element", "prefix": "app", "style": "kebab-case" }]]),
            ),
        ),
        (
            "@Component({ selector: '[appFooBar]' }) class Test {}",
            Some(
                serde_json::json!([[{ "type": "attribute", "prefix": "app", "style": "camelCase" }]]),
            ),
        ),
        (
            "@Component({ selector: 'app-foo-bar' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Component({ selector: '[appFooBar]' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Component({ selector: 'lib-foo-bar' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": ["app", "lib"], "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Component({ selector: '[libFooBar]' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": ["app", "lib"], "style": "camelCase" },
            ]])),
        ),
        (
            "@Component({ selector: '[appFooBar]' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
                { "type": "element", "prefix": "app", "style": "kebab-case" },
            ]])),
        ),
    ];

    let fail = vec![
        (
            "@Component({ selector: 'foo-bar' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "prefix": "sg", "style": "kebab-case" }])),
        ),
        (
            "@Component({ selector: 'app-foo-bar' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "prefix": "sg", "style": "kebab-case" }])),
        ),
        (
            "@Component({ selector: '[app-foo-bar]' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["cd", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'app-foo-bar[baz].app' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["foo", "cd", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: '[ng-bar-foo]' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": "ng", "style": "camelCase" }]),
            ),
        ),
        (
            "@Component({ selector: 'appFooBar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'app' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": "app", "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'app' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "style": "camelCase", "prefix": "app" }])),
        ),
        (
            "@Component({ selector: '[appFooBar]' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "ng"], "style": "camelCase" }]),
            ),
        ),
        (
            "@Component({ selector: `app-foo-bar` }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["app", "ng"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'appFooBar' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "attribute", "prefix": ["app", "ng"], "style": "camelCase" }]),
            ),
        ),
        (
            "@Component({
              encapsulation: ViewEncapsulation.ShadowDom,
              selector: 'appFooBar'
            })
            class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app"], "style": "camelCase" }]),
            ),
        ),
        (
            "@Component({
              encapsulation: ViewEncapsulation.ShadowDom,
              selector: 'appselector'
            })
            class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app"], "style": "camelCase" }]),
            ),
        ),
        (
            "@Component({ selector: 'root' }) class Test {}",
            Some(
                serde_json::json!([{ "type": "element", "prefix": ["app", "toh"], "style": "kebab-case" }]),
            ),
        ),
        (
            "@Component({ selector: 'sgggg-bar' }) class Test {}",
            Some(serde_json::json!([{ "type": "element", "prefix": "sg", "style": "kebab-case" }])),
        ),
        (
            "@Component({ selector: 'appFooBar' }) class Test {}",
            Some(
                serde_json::json!([[{ "type": "element", "prefix": "app", "style": "kebab-case" }]]),
            ),
        ),
        (
            "@Component({ selector: '[app-foo-bar]' }) class Test {}",
            Some(
                serde_json::json!([[{ "type": "attribute", "prefix": "app", "style": "camelCase" }]]),
            ),
        ),
        (
            "@Component({ selector: 'appFooBar' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Component({ selector: '[app-foo-bar]' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Component({ selector: 'lib-foo-bar' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
        (
            "@Component({ selector: '[libFooBar]' }) class Test {}",
            Some(serde_json::json!([[
                { "type": "element", "prefix": "app", "style": "kebab-case" },
                { "type": "attribute", "prefix": "app", "style": "camelCase" },
            ]])),
        ),
    ];

    Tester::new(ComponentSelector::NAME, ComponentSelector::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
