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
    utils::{
        class_decorator, decorator_metadata, implemented_interface_names, metadata_property,
        to_human_readable_text,
    },
};

const VALIDATOR_SUFFIX: &str = "Validator";

fn directive_class_suffix_diagnostic(span: Span, suffixes: &[&str]) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Directive class names should end with one of these suffixes: {}",
        to_human_readable_text(suffixes)
    ))
    .with_label(span)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct DirectiveClassSuffixConfig {
    /// Suffixes a `@Directive` class name may end with. `Validator` is also accepted when the
    /// class implements an interface whose name ends with `Validator`.
    suffixes: Vec<String>,
}

impl Default for DirectiveClassSuffixConfig {
    fn default() -> Self {
        Self { suffixes: vec!["Directive".to_string()] }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct DirectiveClassSuffix(Box<DirectiveClassSuffixConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires classes decorated with `@Directive` that declare a `selector` to end with
    /// `Directive`, or with one of the configured suffixes. A class implementing a `*Validator`
    /// interface may end with `Validator` instead. Since Angular v20 the Angular team no longer
    /// recommends this naming convention.
    ///
    /// ### Why is this bad?
    ///
    /// In a codebase that follows the suffix convention, a directive class without the suffix is
    /// harder to recognize as a directive.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Directive({ selector: '[appHighlight]' })
    /// class Highlight {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Directive({ selector: '[appHighlight]' })
    /// class HighlightDirective {}
    ///
    /// @Directive({ selector: '[appRange]' })
    /// class RangeValidator implements Validator {}
    /// ```
    DirectiveClassSuffix,
    angular,
    style,
    config = DirectiveClassSuffixConfig,
    version = "next",
    short_description = "Require a suffix on `@Directive` class names.",
);

impl Rule for DirectiveClassSuffix {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        let Some(decorator) = class_decorator(class, "Directive") else { return };
        if decorator_metadata(decorator)
            .and_then(|metadata| metadata_property(metadata, "selector"))
            .is_none()
        {
            return;
        }
        let mut suffixes: Vec<&str> = self.0.suffixes.iter().map(String::as_str).collect();
        if implemented_interface_names(class).any(|name| name.ends_with(VALIDATOR_SUFFIX)) {
            suffixes.push(VALIDATOR_SUFFIX);
        }
        if class
            .id
            .as_ref()
            .is_some_and(|id| suffixes.iter().any(|&suffix| id.name.as_str().ends_with(suffix)))
        {
            return;
        }
        let span = class.id.as_ref().map_or(class.span, |id| id.span);
        ctx.diagnostic(directive_class_suffix_diagnostic(span, &suffixes));
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        ("@Directive({ selector: 'sgBarFoo' }) class TestDirective {}", None),
        ("@Directive({ selector: 'sgBarFoo' }) class TestValidator implements Validator {}", None),
        (
            "@Directive({ selector: 'sgBarFoo' }) class TestValidator implements AsyncValidator {}",
            None,
        ),
        ("@Directive class Test {}", None),
        ("@Directive() class Test {}", None),
        ("@Component({ selector: 'sg-bar-foo' }) class TestComponent {}", None),
        ("@Pipe({ name: 'sgPipe' }) class TestPipe {}", None),
        ("@Injectable() class TestService {}", None),
        ("class TestEmpty {}", None),
        (
            "@Directive({ selector: 'sgBarFoo' }) class TestDir {}",
            Some(serde_json::json!([{ "suffixes": ["Dir"] }])),
        ),
        (
            "@Directive({ selector: 'sgBarFoo' }) class TestPage {}",
            Some(serde_json::json!([{ "suffixes": ["Page", "View"] }])),
        ),
    ];

    let fail = vec![
        ("@Directive({ selector: 'sg-foo-bar' }) class Test {}", None),
        (
            "@Directive({ selector: 'sg-foo-bar' }) class TestDirectivePage implements AsyncValidator {}",
            None,
        ),
        (
            "@Directive({ selector: 'sgBarFoo' }) class TestPageDirective {}",
            Some(serde_json::json!([{ "suffixes": ["Page"] }])),
        ),
    ];

    Tester::new(DirectiveClassSuffix::NAME, DirectiveClassSuffix::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
