use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{class_decorator, to_human_readable_text},
};

fn component_class_suffix_diagnostic(span: Span, suffixes: &[String]) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Component class names should end with one of these suffixes: {}",
        to_human_readable_text(suffixes)
    ))
    .with_label(span)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ComponentClassSuffixConfig {
    /// Suffixes a `@Component` class name may end with.
    suffixes: Vec<String>,
}

impl Default for ComponentClassSuffixConfig {
    fn default() -> Self {
        Self { suffixes: vec!["Component".to_string()] }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct ComponentClassSuffix(Box<ComponentClassSuffixConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires classes decorated with `@Component` to end with `Component`, or with one of the
    /// configured suffixes. Since Angular v20 the Angular team no longer recommends this naming
    /// convention.
    ///
    /// ### Why is this bad?
    ///
    /// In a codebase that follows the suffix convention, a component class without the suffix is
    /// harder to recognize as a component.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({ selector: 'app-foo' })
    /// class Foo {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({ selector: 'app-foo' })
    /// class FooComponent {}
    /// ```
    ComponentClassSuffix,
    angular,
    style,
    config = ComponentClassSuffixConfig,
    version = "next",
    short_description = "Require a suffix on `@Component` class names.",
);

impl Rule for ComponentClassSuffix {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if class_decorator(class, "Component").is_none() {
            return;
        }
        let suffixes = &self.0.suffixes;
        if class.id.as_ref().is_some_and(|id| {
            suffixes.iter().any(|suffix| id.name.as_str().ends_with(suffix.as_str()))
        }) {
            return;
        }
        let span = class.id.as_ref().map_or(class.span, |id| id.span);
        ctx.diagnostic(component_class_suffix_diagnostic(span, suffixes));
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        (
            "@Component({
              selector: 'sg-foo-bar',
              template: '<foo-bar [foo]=\"bar\">{{baz + 42}}</foo-bar>'
            })
            class TestComponent {}",
            None,
        ),
        ("@Directive({ selector: '[myHighlight]' }) class TestDirective {}", None),
        ("@Pipe({ name: 'sg-test-pipe' }) class TestPipe {}", None),
        ("@Injectable() class TestService {}", None),
        ("class TestEmpty {}", None),
        (
            "@Component({ selector: 'sgBarFoo' }) class TestPage {}",
            Some(serde_json::json!([{ "suffixes": ["Page"] }])),
        ),
        (
            "@Component({ selector: 'sgBarFoo' }) class TestPage {}",
            Some(serde_json::json!([{ "suffixes": ["Page", "View"] }])),
        ),
    ];

    let fail = vec![
        ("@Component({ selector: 'sg-foo-bar' }) class Test {}", None),
        (
            "@Component({ selector: 'sgBarFoo' }) class TestPage {}",
            Some(serde_json::json!([{ "suffixes": ["Component", "View"] }])),
        ),
        (
            "@Component({ selector: 'sgBarFoo' }) class TestPage {}",
            Some(serde_json::json!([{ "suffixes": ["Component"] }])),
        ),
        (
            "@Component({ selector: 'sgBarFoo' }) class TestDirective {}",
            Some(serde_json::json!([{ "suffixes": ["Page"] }])),
        ),
    ];

    Tester::new(ComponentClassSuffix::NAME, ComponentClassSuffix::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
