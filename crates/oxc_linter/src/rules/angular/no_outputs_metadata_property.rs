use oxc_ast::{AstKind, ast::PropertyKey};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{decorator_call_name, enclosing_class_decorator},
};

fn no_outputs_metadata_property_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Use `@Output` rather than the `outputs` metadata property")
        .with_help("Declare the output on the class member with `output()` or `@Output()`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoOutputsMetadataProperty;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the `outputs` metadata property of `@Component` and `@Directive`.
    /// The `outputs` of a `hostDirectives` entry are allowed.
    ///
    /// ### Why is this bad?
    ///
    /// Outputs listed in the decorator metadata are declared away from the class member they
    /// bind to, so the component API is split across two places. `output()` and `@Output()` keep
    /// the declaration next to the property and work with Find References and rename refactoring.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({ outputs: ['change'] })
    /// class Test {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class Test {
    ///   change = output<string>();
    /// }
    /// ```
    NoOutputsMetadataProperty,
    angular,
    style,
    version = "next",
    short_description = "Disallow the `outputs` metadata property.",
);

impl Rule for NoOutputsMetadataProperty {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::ObjectProperty(property) = node.kind() else { return };
        if !property.key.is_specific_static_name("outputs") || is_host_directive_entry(node, ctx) {
            return;
        }
        let Some(decorator) = enclosing_class_decorator(node, ctx) else { return };
        if matches!(decorator_call_name(decorator), Some("Component" | "Directive")) {
            ctx.diagnostic(no_outputs_metadata_property_diagnostic(property.span));
        }
    }
}

// `hostDirectives: [{ outputs: [...] }]` exposes a host directive's outputs, which is allowed.
fn is_host_directive_entry(node: &AstNode, ctx: &LintContext) -> bool {
    let mut ancestors = ctx.nodes().ancestor_kinds(node.id());
    matches!(
        (ancestors.next(), ancestors.next(), ancestors.next()),
        (
            Some(AstKind::ObjectExpression(_)),
            Some(AstKind::ArrayExpression(_)),
            Some(AstKind::ObjectProperty(property)),
        ) if is_host_directives_key(&property.key)
    )
}

fn is_host_directives_key(key: &PropertyKey) -> bool {
    match key {
        PropertyKey::StaticIdentifier(ident) => ident.name.as_str() == "hostDirectives",
        PropertyKey::Identifier(ident) => ident.name.as_str() == "hostDirectives",
        PropertyKey::StringLiteral(lit) => lit.value.as_str() == "hostDirectives",
        _ => false,
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "class Test {}",
        r"
    @Component()
    class Test {}
    ",
        r"
    @Directive({})
    class Test {}
    ",
        r"
    const options = {};
    @Component(options)
    class Test {}
    ",
        r"
    @Directive({
      selector: 'app-test',
      template: 'Hello'
    })
    class Test {}
    ",
        r"
    @Component({
      selector: 'app-test',
      queries: {},
    })
    class Test {}
    ",
        r"
    const outputs = 'providers';
    @Directive({
      [outputs]: [],
    })
    class Test {}
    ",
        r"
    @NgModule({
      bootstrap: [Foo]
    })
    class Test {}
    ",
        r"
    @Component({
      selector: 'foo',
      hostDirectives: [{
        directive: CdkMenuItem,
        outputs: ['cdkMenuItemTriggered: triggered'],
      }]
    })
    class Test {}
  ",
        r"
    @Component({
      selector: 'foo',
      'hostDirectives': [{
        directive: CdkMenuItem,
        outputs: ['cdkMenuItemTriggered: triggered'],
      }]
    })
    class Test {}
  ",
        r"
    @Component({
      selector: 'foo',
      ['hostDirectives']: [{
        directive: CdkMenuItem,
        outputs: ['cdkMenuItemTriggered: triggered'],
      }]
    })
    class Test {}
  ",
    ];

    let fail = vec![
        r"
        @Component({
          outputs: [
            'id: foo'
          ],
          selector: 'app-test'
        })
        class Test {}
      ",
        r"
        @Directive({
          outputs: [
            'id: foo'
          ],
          selector: 'app-test'
        })
        class Test {}
      ",
        r"
        @Component({
          outputs,
        })
        class Test {}
      ",
        r"
        @Directive({
          outputs: [],
        })
        class Test {}
      ",
        r"
        const test = [];
        @Component({
          'outputs': test,
        })
        class Test {}
      ",
        r"
        @Directive({
          ['outputs']: undefined,
        })
        class Test {}
      ",
        r"
        function outputs() {
          return [];
        }

        @Component({
          [`outputs`]: outputs(),
        })
        class Test {}
      ",
    ];

    Tester::new(NoOutputsMetadataProperty::NAME, NoOutputsMetadataProperty::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
