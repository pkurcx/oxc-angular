use oxc_ast::{
    AstKind,
    ast::{Argument, ArrowFunctionExpression, Function, ReturnStatement},
};
use oxc_ast_visit::VisitJs;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_semantic::ScopeFlags;
use oxc_span::Span;

use crate::{AstNode, context::LintContext, rule::Rule};

fn computed_must_return_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("computed() is missing a return value")
        .with_help("Return the derived value from the `computed()` callback.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ComputedMustReturn;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that the callback passed to Angular's `computed()` returns a value.
    ///
    /// ### Why is this bad?
    ///
    /// `computed()` derives a signal value from its callback. A callback that never returns a
    /// value produces a signal that is always `undefined`, which is almost always a mistake.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// const total = computed(() => {
    ///   price() * quantity();
    /// });
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// const total = computed(() => {
    ///   return price() * quantity();
    /// });
    /// const double = computed(() => count() * 2);
    /// ```
    ComputedMustReturn,
    angular,
    correctness,
    none,
    version = "next",
    short_description = "Ensure that `computed()` returns a value.",
);

impl Rule for ComputedMustReturn {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::CallExpression(call) = node.kind() else { return };
        if !call.callee.is_specific_id("computed") {
            return;
        }
        let body = match call.arguments.first() {
            Some(Argument::ArrowFunctionExpression(arrow)) => arrow.get_function_body(),
            Some(Argument::FunctionExpression(func)) => func.body.as_deref(),
            _ => return,
        };
        // An arrow with an expression body always returns it.
        let Some(body) = body else { return };

        let mut finder = ReturnFinder::default();
        finder.visit_function_body(body);
        if !finder.found {
            ctx.diagnostic(computed_must_return_diagnostic(call.span));
        }
    }
}

#[derive(Default)]
struct ReturnFinder {
    found: bool,
}

impl<'a> VisitJs<'a> for ReturnFinder {
    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        self.found |= it.argument.is_some();
    }

    // Returns inside nested functions belong to them.
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "const computed = 2;",
        "class computed {}",
        "const x = computed;",
        "computed()",
        "class Test {\n  computed = 2;\n}",
        "class Test {\n  computed() {};\n}",
        "class Test {\n  x = computed(() => y());\n}",
        "function computedFactory() {\n  return computed(() => {\n    return 2;\n  });\n}",
        "computed(() => {\n  return 2;\n});",
        "computed(function() {\n  return 2;\n});",
        "computed(() => {\n  return () => {};\n});",
        "computed(() => {\n  return undefined;\n});",
        "Test.computed(() => {});",
        "instance.computed(() => {});",
        "computed(() => {\n  if (condition()) {\n    return;\n  }\n\n  return 2;\n})",
        "computed(() => {\n  if (condition()) {\n    return;\n  }\n\n  return 2;\n})",
    ];

    let fail = vec![
        "computed(() => {});",
        "computed(() => {\n  y();\n});",
        "computed(function() {\n  y();\n});",
        "computed(() => {\n  return;\n});",
        "computed(function() {\n  return;\n});",
        "class Test {\n  x = computed(() => {\n    y();\n  });\n}",
        "computed(() => {}, { equal: (a, b) => {} });",
        "computed(() => {\n  const nested = () => {\n    return 2;\n  }\n});",
    ];

    Tester::new(ComputedMustReturn::NAME, ComputedMustReturn::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
