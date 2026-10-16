use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{class_decorator, decorator_metadata, metadata_property},
};

fn use_component_selector_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The selector of the component is mandatory")
        .with_help("Add a `selector` to the `@Component` metadata.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct UseComponentSelector;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires every `@Component` to declare a non-empty `selector`.
    ///
    /// ### Why is this bad?
    ///
    /// A component without a selector cannot be used in templates, only through routing or
    /// dynamic loading, and it is harder to identify in the component tree and DevTools.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({ template: '<p>Hello</p>' })
    /// class HelloComponent {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({ selector: 'app-hello', template: '<p>Hello</p>' })
    /// class HelloComponent {}
    /// ```
    UseComponentSelector,
    angular,
    pedantic,
    version = "next",
    short_description = "Require a selector in `@Component` metadata.",
);

impl Rule for UseComponentSelector {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        let Some(decorator) = class_decorator(class, "Component") else { return };
        let has_selector = decorator_metadata(decorator)
            .and_then(|metadata| metadata_property(metadata, "selector"))
            .is_some_and(|prop| match &prop.value {
                Expression::StringLiteral(lit) => !lit.value.is_empty(),
                Expression::TemplateLiteral(_) | Expression::Identifier(_) => true,
                _ => false,
            });
        if !has_selector {
            ctx.diagnostic(use_component_selector_diagnostic(decorator.span));
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "@Component({ selector: 'sg-bar-foo' }) class Test {}",
        r#"@Component({ selector: "sg-bar-foo" }) class Test {}"#,
        "@Component({ selector: `sg-bar-foo` }) class Test {}",
        "const selector = 'sg-bar-foo';
        @Component({ selector, }) class Test {}",
        "const selectorVar = 'sg-bar-foo';
        @Component({ selector: selectorVar, }) class Test {}",
    ];

    let fail = vec![
        "@Component() class Test {}",
        "@Component({ selector: '' }) class Test {}",
        "@Component({ selector: 0 }) class Test {}",
        "@Component({ selector: null }) class Test {}",
    ];

    Tester::new(UseComponentSelector::NAME, UseComponentSelector::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
