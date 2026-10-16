use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{AstNode, context::LintContext, rule::Rule, utils::decorator_call_name};

fn prefer_host_metadata_property_diagnostic(
    span: Span,
    kind: &str,
    decorator: &str,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Prefer using the `host` metadata property in the `@Component` decorator for host {kind} instead of the `@{decorator}` decorator."
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferHostMetadataProperty;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the `@HostBinding` and `@HostListener` decorators in favor of the `host`
    /// metadata property of `@Component` and `@Directive`.
    ///
    /// ### Why is this bad?
    ///
    /// The `host` metadata property keeps every host binding and listener in one place, while the
    /// decorators scatter them across the class. The Angular documentation recommends `host` and
    /// keeps the decorators only for backwards compatibility.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({ selector: 'app-test' })
    /// class Test {
    ///   @HostBinding('class') readonly cssClass = 'my-class';
    ///   @HostListener('click') onClick() {}
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({
    ///   selector: 'app-test',
    ///   host: { class: 'my-class', '(click)': 'onClick()' },
    /// })
    /// class Test {
    ///   onClick() {}
    /// }
    /// ```
    PreferHostMetadataProperty,
    angular,
    restriction,
    version = "next",
    short_description = "Use the `host` metadata property instead of `@HostBinding` and `@HostListener`.",
);

impl Rule for PreferHostMetadataProperty {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else { return };
        let Some(name) = decorator_call_name(decorator) else { return };
        let kind = match name {
            "HostBinding" => "bindings",
            "HostListener" => "listeners",
            _ => return,
        };
        ctx.diagnostic(prefer_host_metadata_property_diagnostic(decorator.span, kind, name));
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    @Component({
      host: {
        class: 'my-class',
      },
    })
    class Test {}
  ",
        r"
    @Component({
      host: {
        '(click)': 'bar()'
      },
    })
    class Test {
      bar() {}
    }
  ",
    ];

    let fail = vec![
        r"
      class Test {
        @HostBinding('class')
        readonly cssClass = 'my-class';
      }
      ",
        r"
      class Test {
        @HostListener('click')
        bar() {}
      }
      ",
    ];

    Tester::new(PreferHostMetadataProperty::NAME, PreferHostMetadataProperty::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
