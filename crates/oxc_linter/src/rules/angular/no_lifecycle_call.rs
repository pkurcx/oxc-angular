use oxc_ast::{
    AstKind,
    ast::{Expression, PropertyKey},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{angular_class_decorator, is_angular_lifecycle_method, nearest_class_declaration},
};

fn no_lifecycle_call_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Avoid explicit calls to lifecycle methods.")
        .with_help("Move the shared logic into a separate method and call that instead.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoLifecycleCall;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows explicit calls to lifecycle methods inside Angular classes. Calling
    /// `super.ngOnInit()` from the class's own `ngOnInit` is allowed.
    ///
    /// ### Why is this bad?
    ///
    /// Angular calls lifecycle hooks at the right moments. Calling one manually can run it twice
    /// or out of order, and makes the execution flow harder to follow.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent {
    ///   reset() { this.ngOnInit(); }
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent extends BaseComponent {
    ///   ngOnInit() { super.ngOnInit(); }
    /// }
    /// ```
    NoLifecycleCall,
    angular,
    suspicious,
    version = "next",
    short_description = "Disallow explicit calls to lifecycle methods.",
);

impl Rule for NoLifecycleCall {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::StaticMemberExpression(member) = node.kind() else { return };
        let name = member.property.name.as_str();
        if !is_angular_lifecycle_method(name) {
            return;
        }
        // Upstream matches the member as any child of the call, so `foo(this.ngOnInit)` counts too.
        let AstKind::CallExpression(call) = ctx.nodes().parent_kind(node.id()) else { return };
        if nearest_class_declaration(node, ctx)
            .is_none_or(|class| angular_class_decorator(class).is_none())
        {
            return;
        }
        if matches!(member.object, Expression::Super(_)) && is_inside_method_named(node, name, ctx)
        {
            return;
        }
        ctx.diagnostic(no_lifecycle_call_diagnostic(call.span));
    }
}

fn is_inside_method_named(node: &AstNode, name: &str, ctx: &LintContext) -> bool {
    ctx.nodes()
        .ancestors(node.id())
        .find_map(|ancestor| match ancestor.kind() {
            AstKind::MethodDefinition(method) => Some(method),
            _ => None,
        })
        .is_some_and(
            |method| matches!(&method.key, PropertyKey::StaticIdentifier(key) if key.name == name),
        )
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    @Component()
    class Test {
      ngAfterContentChecked(): void { 
        super.ngAfterContentChecked(); 
      }
    }

    @Directive()
    class Test2 {
      test(): void {
        this.ngAfterContentChecked1();
        this.angAfterContentInit();
        this.ngAfterViewChecked2();
        this.ngAfterViewInit3();
        this.ngOnChange$();
        this.ngOnDestroyx();
        this.ngOnInitialize();
        this.ngDoChecking();
        ngOnInit();
      }
    }

    @MyDecorator()
    class Test3 {
      test(): void {
        ngDoCheck();
      }
    }

    ngOnDestroy();
    ",
    ];

    let fail = vec![
        r"
        @Component()
        class Test {
          test(): void {
            this.ngAfterContentChecked();
          }
        }
      ",
        r"
        @Directive()
        class Test {
          test(): void {
            this.ngAfterContentInit();
          }
        }
      ",
        r"
        @Injectable()
        class Test {
          test(): void {
            this.ngAfterViewChecked();
          }
        }
      ",
        r"
        @NgModule()
        class Test {
          test(): void {
            this.ngAfterViewInit();
          }
        }
      ",
        r"
        @Pipe()
        class Test {
          test(): void {
            this.ngDoBootstrap();
          }
        }
      ",
        r"
        @Component()
        class Test {
          test(): void {
            this.ngDoCheck();
          }
        }
      ",
        r"
        @Directive()
        class Test {
          test(): void {
            this.ngOnChanges();
          }
        }
      ",
        r"
        @Injectable()
        class Test {
          test(): void {
            this.ngOnDestroy();
          }
        }
      ",
        r"
        @NgModule()
        class Test {
          test(): void {
            this.ngOnInit();
          }
        }
      ",
        r"
        @Service()
        class Test {
          test(): void {
            this.ngOnDestroy();
          }
        }
      ",
        r"
        @Component({ template: '' })
        class Test extends ParentComponent {
          test(): void {
            super.ngOnChanges();
          }
        }
      ",
    ];

    Tester::new(NoLifecycleCall::NAME, NoLifecycleCall::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
