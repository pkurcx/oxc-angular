use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{AstNode, context::LintContext, rule::Rule, utils::decorator_call_name};

fn no_attribute_decorator_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "`@Attribute` can only obtain a single value and is rarely what is required.",
    )
    .with_help("Use `@Input` instead to retrieve a stream of values.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoAttributeDecorator;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the `@Attribute` decorator on constructor parameters.
    ///
    /// ### Why is this bad?
    ///
    /// `@Attribute` reads a host attribute once, at construction, and never updates. It is often
    /// used by mistake where `@Input` (or `input()`), which tracks changes, was intended.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// class Test {
    ///   constructor(@Attribute('type') type: string) {}
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// class Test {
    ///   @Input() type: string;
    /// }
    /// ```
    NoAttributeDecorator,
    angular,
    restriction,
    version = "next",
    short_description = "Disallow the `@Attribute` decorator.",
);

impl Rule for NoAttributeDecorator {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else { return };
        if decorator_call_name(decorator) != Some("Attribute") {
            return;
        }
        let nodes = ctx.nodes();
        let Some(constructor) = nodes.ancestors(node.id()).find(|ancestor| {
            matches!(ancestor.kind(), AstKind::MethodDefinition(method) if method.key.is_specific_id("constructor"))
        }) else {
            return;
        };
        let in_class_declaration = nodes.ancestors(constructor.id()).any(
            |ancestor| matches!(ancestor.kind(), AstKind::Class(class) if class.is_declaration()),
        );
        if in_class_declaration {
            ctx.diagnostic(no_attribute_decorator_diagnostic(decorator.span));
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
            class Test {
              foo() {}
            }
        ",
        r"
            class Test {
              constructor() {}
            }
        ",
        r"
            class Test {
              constructor(@Optional() foo: string, @Optional() bar: string, baz: number) {}
            }
        ",
    ];

    let fail = vec![
        r"
            class Test {
              constructor(@Attribute() foo: string) {}
            }
        ",
        r"
            class Test {
              constructor(
                @Inject(TOKEN) token: string,
                randomNumber: number,
                @Attribute() foo: string,
                @Attribute('baz') bar: string
              ) {}
            }
        ",
    ];

    Tester::new(NoAttributeDecorator::NAME, NoAttributeDecorator::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
