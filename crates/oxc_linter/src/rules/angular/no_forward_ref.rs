use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{AstNode, context::LintContext, rule::Rule};

fn no_forward_ref_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Avoid using `forwardRef`")
        .with_help("Restructure the code to remove the circular dependency that `forwardRef` works around.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoForwardRef;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows calls to Angular's `forwardRef()` for dependency injection.
    ///
    /// ### Why is this bad?
    ///
    /// `forwardRef()` works around circular dependencies, but it hides the dependency graph and
    /// usually signals a design problem that a shared abstraction or service would resolve.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({
    ///   providers: [{ provide: NG_VALUE_ACCESSOR, useExisting: forwardRef(() => TagsValueAccessor) }],
    /// })
    /// class TagsValueAccessor {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({
    ///   providers: [{ provide: NG_VALUE_ACCESSOR, useExisting: TagsValueAccessor }],
    /// })
    /// class TagsValueAccessor {}
    /// ```
    NoForwardRef,
    angular,
    restriction,
    version = "next",
    short_description = "Disallow `forwardRef` references for dependency injection.",
);

impl Rule for NoForwardRef {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::CallExpression(call) = node.kind() else { return };
        if call.callee.is_specific_id("forwardRef") {
            ctx.diagnostic(no_forward_ref_diagnostic(call.span));
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
            @Component({
              selector: 'test',
              template: ''
            })
            export class Test {
              constructor() {
                this.test();
              }

              test() {}
            }
        ",
    ];

    let fail = vec![
        r"
            @Component({
              selector: 'test',
              template: ''
            })
            export class Test {
              constructor(@Inject(forwardRef(() => TestService)) testService) {}
            }
            export class TestService {}
        ",
        r"
            @Component({
              providers: [
                {
                  multi: true,
                  provide: NG_VALUE_ACCESSOR,
                  useExisting: forwardRef(() => TagsValueAccessor)
                }
              ],
              selector: '[tags]',
            })
            export class TagsValueAccessor {}
        ",
        r"
            const TAGS_VALUE_ACCESSOR: StaticProvider = {
              multi: true,
              provide: NG_VALUE_ACCESSOR,
              useExisting: forwardRef(() => TagsValueAccessor)
            };
            @Directive({
              providers: [TAGS_VALUE_ACCESSOR],
              selector: '[tags]'
            })
            export class TagsValueAccessor {}
        ",
    ];

    Tester::new(NoForwardRef::NAME, NoForwardRef::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
