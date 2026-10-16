use oxc_ast::{AstKind, ast::ClassElement};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{class_decorator, is_angular_lifecycle_method},
};

// Execution order; `ngDoBootstrap` is absent and ranks 0, before every listed hook, as upstream.
const LIFECYCLE_METHODS_ORDERED: [&str; 8] = [
    "ngOnChanges",
    "ngOnInit",
    "ngDoCheck",
    "ngAfterContentInit",
    "ngAfterContentChecked",
    "ngAfterViewInit",
    "ngAfterViewChecked",
    "ngOnDestroy",
];

fn sort_lifecycle_methods_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Lifecycle methods are not declared in order of execution.")
        .with_help(
            "Declare them in the order ngOnChanges, ngOnInit, ngDoCheck, ngAfterContentInit, \
             ngAfterContentChecked, ngAfterViewInit, ngAfterViewChecked, ngOnDestroy.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct SortLifecycleMethods;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that the lifecycle methods of an Angular `@Component` are declared in the order
    /// Angular executes them.
    ///
    /// ### Why is this bad?
    ///
    /// Hooks in execution order let readers follow the component lifecycle from top to bottom
    /// instead of jumping around the class.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent {
    ///   ngOnInit() {}
    ///   ngOnChanges() {}
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent {
    ///   ngOnChanges() {}
    ///   ngOnInit() {}
    /// }
    /// ```
    SortLifecycleMethods,
    angular,
    style,
    version = "next",
    short_description = "Ensure that lifecycle methods are declared in order of execution.",
);

impl Rule for SortLifecycleMethods {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if class_decorator(class, "Component").is_none() {
            return;
        }
        let mut previous: Option<usize> = None;
        for element in &class.body.body {
            let ClassElement::MethodDefinition(method) = element else { continue };
            if method.r#type.is_abstract() {
                continue;
            }
            let Some(name) = method.key.static_name() else { continue };
            if !is_angular_lifecycle_method(&name) {
                continue;
            }
            let rank = LIFECYCLE_METHODS_ORDERED
                .iter()
                .position(|ordered| *ordered == name)
                .map_or(0, |index| index + 1);
            if previous.is_some_and(|previous| rank < previous) {
                ctx.diagnostic(sort_lifecycle_methods_diagnostic(method.key.span()));
            }
            previous = Some(rank);
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    @Component()
    class Test {
      ngOnChanges(): void {}
      ngOnInit(): void {}
      ngDoCheck(): void {}
      ngAfterContentInit(): void {}
      ngAfterContentChecked(): void {}
      ngAfterViewInit(): void {}
      ngAfterViewChecked(): void {}
      ngOnDestroy(): void {}
      doSomething(): void {}
    }
  ",
        r"
    @Component()
    class Test {
      ngOnChanges(): void {}
      ngOnInit(): void {}
      ngAfterContentInit(): void {}
      ngAfterContentChecked(): void {}
      ngAfterViewChecked(): void {}
      ngOnDestroy(): void {}
    }
  ",
        r"
    @Component()
    class Test {
      ngDoCheck(): void {}
      ngAfterContentInit(): void {}
      ngAfterContentChecked(): void {}
      ngAfterViewChecked(): void {}
      doSomething(): void {}
      doSomethingElse(): void {}
      doSomethingElseAgain(): void {}
    }
  ",
        r"
    @Component()
    class Test {
      ngOnInit(): void {}
    }
  ",
        r"
    @Component()
    class Test {}
  ",
    ];

    let fail = vec![
        r"
      @Component()
      class Test {
        ngOnInit(): void {}
        ngOnChanges(): void {}
        ngDoCheck(): void {}
        ngAfterContentInit(): void {}
        ngAfterContentChecked(): void {}
        ngAfterViewInit(): void {}
        ngAfterViewChecked(): void {}
        ngOnDestroy(): void {}
        doSomething(): void {}
      }
      ",
        r"
      @Component()
      class Test {
        ngOnChanges(): void {}
        ngOnInit(): void {}
        ngAfterContentInit(): void {}
        ngAfterContentChecked(): void {}
        ngOnDestroy(): void {}
        ngAfterViewChecked(): void {}
        doSomething(): void {}
      }
      ",
        r"
      @Component()
      class Test {
        ngDoCheck(): void {}
        ngAfterContentInit(): void {}
        ngOnDestroy(): void {}
        ngAfterContentChecked(): void {}
        ngAfterViewChecked(): void {}
        doSomething(): void {}
        doSomethingElse(): void {}
      }
      ",
        r"
      @Component()
      class Test {
        ngOnInit(): void {}
        ngOnChanges(): void {}
       }
      ",
    ];

    Tester::new(SortLifecycleMethods::NAME, SortLifecycleMethods::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
