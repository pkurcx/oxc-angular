use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{AstNode, context::LintContext, rule::Rule, utils::decorator_call_name};

fn prefer_output_emitter_ref_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Use `OutputEmitterRef` via `output()` for Component and Directive outputs rather than the legacy `@Output()` decorator",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferOutputEmitterRef;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the `@Output()` decorator in favor of the `output()` function, which returns an
    /// `OutputEmitterRef`.
    ///
    /// ### Why is this bad?
    ///
    /// `output()` is Angular's current API for component and directive outputs. It fits the
    /// signal-based APIs and has a simpler, more type-safe surface than `@Output()` with
    /// `EventEmitter`, which remains only for backwards compatibility.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// class Test {
    ///   @Output() readonly change = new EventEmitter<number>();
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// class Test {
    ///   readonly change = output<number>();
    /// }
    /// ```
    PreferOutputEmitterRef,
    angular,
    restriction,
    version = "next",
    short_description = "Use `OutputEmitterRef` instead of `@Output()`.",
);

impl Rule for PreferOutputEmitterRef {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else { return };
        if decorator_call_name(decorator) == Some("Output") {
            ctx.diagnostic(prefer_output_emitter_ref_diagnostic(decorator.span));
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    class Test {
      change = outputValue();
    }
    ",
        r"
    class Test {
      change = new EventEmitter();
    }
    ",
        r"
    class Test {
      readonly change = output();
    }
    ",
        r"
    class Test {
      readonly change = output<string>();
    }
    ",
    ];

    let fail = vec![
        r"
      class Test {
        @Output()
        readonly change: EventEmitter<number>;
      }
      ",
    ];

    Tester::new(PreferOutputEmitterRef::NAME, PreferOutputEmitterRef::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
