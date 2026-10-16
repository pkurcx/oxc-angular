use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{decorator_call_name, enclosing_class_decorator, list_item_removal_span},
};

fn no_pipe_impure_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Impure pipes should be avoided because they are invoked on each change-detection cycle",
    )
    .with_help("Remove `pure: false` and keep the pipe pure.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoPipeImpure;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the declaration of impure pipes, that is `@Pipe({ pure: false })`.
    ///
    /// ### Why is this bad?
    ///
    /// An impure pipe runs on every change-detection cycle, whether or not its inputs changed.
    /// In a template rendering a list, that can mean thousands of calls per second. A pure pipe,
    /// the default, only runs again when its input references change.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Pipe({ name: 'test', pure: false })
    /// class TestPipe {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Pipe({ name: 'test' })
    /// class TestPipe {}
    /// ```
    NoPipeImpure,
    angular,
    perf,
    suggestion,
    version = "next",
    short_description = "Disallow impure pipes.",
);

impl Rule for NoPipeImpure {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::ObjectProperty(prop) = node.kind() else { return };
        if !prop.key.is_specific_static_name("pure") || !is_false(&prop.value) {
            return;
        }
        if enclosing_class_decorator(node, ctx).and_then(decorator_call_name) != Some("Pipe") {
            return;
        }
        ctx.diagnostic_with_suggestion(no_pipe_impure_diagnostic(prop.span), |fixer| {
            fixer
                .delete_range(list_item_removal_span(ctx.source_text(), prop.span))
                .with_message("Remove `pure` property")
        });
    }
}

// `false` or `!true`.
fn is_false(expr: &Expression) -> bool {
    match expr {
        Expression::BooleanLiteral(lit) => !lit.value,
        Expression::UnaryExpression(unary) => {
            unary.operator.is_not()
                && matches!(&unary.argument, Expression::BooleanLiteral(lit) if lit.value)
        }
        _ => false,
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "class Test {}",
        "@Pipe()\nclass Test {}",
        "@Pipe({})\nclass Test {}",
        "const options = {};\n@Pipe(options)\nclass Test {}",
        "@Pipe({\n  name: 'test',\n})\nclass Test {}",
        "@Pipe({\n  pure: true\n})\nclass Test {}",
        "@Pipe({\n  'pure': !0,\n})\nclass Test {}",
        "@Pipe({\n  ['pure']: !!isPure(),\n})\nclass Test {}",
        "const pure = 'pure';\n@Pipe({\n  [pure]: false\n})\nclass Test {}",
        "const pure = false;\n@Pipe({\n  pure,\n})\nclass Test {}",
        "function isPure() {\n  return false;\n}\n\n@Pipe({\n  [`pure`]: isPure(),\n})\nclass Test {}",
        "@NgModule({\n  bootstrap: [Foo]\n})\nclass Test {}",
    ];

    let fail = vec![
        "@Pipe({\n  pure: false\n})\nclass Test {}",
        "@Pipe({\n  name: 'test',\n  'pure': false,\n})\nclass Test {}",
        "@Pipe({\n  name: 'test',\n  ['pure']: !true\n})\nclass Test {}",
        "@Pipe({\n  name: 'test',\n  [`pure`]: false\n})\nclass Test {}",
    ];

    let fix = vec![
        ("@Pipe({\n  pure: false\n})\nclass Test {}", "@Pipe({\n  \n})\nclass Test {}"),
        (
            "@Pipe({\n  name: 'test',\n  'pure': false,\n})\nclass Test {}",
            "@Pipe({\n  name: 'test',\n})\nclass Test {}",
        ),
        (
            "@Pipe({\n  name: 'test',\n  ['pure']: !true\n})\nclass Test {}",
            "@Pipe({\n  name: 'test',\n})\nclass Test {}",
        ),
        (
            "@Pipe({\n  name: 'test',\n  [`pure`]: false\n})\nclass Test {}",
            "@Pipe({\n  name: 'test',\n})\nclass Test {}",
        ),
    ];

    Tester::new(NoPipeImpure::NAME, NoPipeImpure::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
