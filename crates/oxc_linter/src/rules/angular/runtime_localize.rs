use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{AstNode, context::LintContext, rule::Rule};

fn runtime_localize_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("$localize could be called before translations are loaded")
        .with_help(
            "Call `$localize` inside a function, a method or a non-static class property initializer.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RuntimeLocalize;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that `$localize` tagged messages can use translations loaded at runtime, by
    /// disallowing `$localize` calls that run when the module loads.
    ///
    /// ### Why is this bad?
    ///
    /// A `$localize` call at the top level of a module or in a static class property runs as soon
    /// as the module loads, typically before runtime translations are loaded. The message is then
    /// frozen in the source language and never picks up the translations loaded later.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// const title = $localize`Hello`;
    ///
    /// class Test {
    ///   static title = $localize`Hello`;
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// const title = () => $localize`Hello`;
    ///
    /// class Test {
    ///   title = $localize`Hello`;
    /// }
    /// ```
    RuntimeLocalize,
    angular,
    restriction,
    none,
    version = "next",
    short_description = "Disallow `$localize` calls that run before runtime translations are loaded.",
);

impl Rule for RuntimeLocalize {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::TaggedTemplateExpression(tagged) = node.kind() else { return };
        if !tagged.tag.is_specific_id("$localize") {
            return;
        }
        let deferred = ctx.nodes().ancestors(node.id()).any(|ancestor| match ancestor.kind() {
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => true,
            AstKind::PropertyDefinition(prop) => !prop.r#static,
            _ => false,
        });
        if !deferred {
            ctx.diagnostic(runtime_localize_diagnostic(tagged.tag.span()));
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "function test() { return $localize`foo`; }",
        "const test = function () { return $localize`foo`; }",
        "const test = () => $localize`foo`;",
        "function test(foo = $localize`foo`) { return foo; }",
        "class Test {\n  foo() { $localize`foo`; }\n}",
        "class Test {\n  foo = () => $localize`foo`;\n}",
        "class Test {\n  static foo = () => $localize`foo`;\n}",
        "class Test {\n  readonly foo = $localize`foo`;\n}",
        "class Test {\n  readonly foo = { x: [`--${$localize`foo`}--`] };\n}",
    ];

    let fail = vec![
        "const foo = $localize`foo`;",
        "let foo = {\n  x: [{\n    y: `--${$localize`foo`}--`\n  }]\n};",
        "(x => { return x; })($localize`foo`);",
        "class Test {\n  static foo = $localize`foo`;\n}",
        "class Test {\n  static foo = {\n    x: [{\n      y: `--${$localize`foo`}--`\n    }]\n  }\n}",
    ];

    Tester::new(RuntimeLocalize::NAME, RuntimeLocalize::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
