use oxc_ast::{
    AstKind,
    ast::{Class, ClassElement, PropertyKey},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{ANGULAR_CLASS_DECORATORS, decorator_call_name, is_angular_lifecycle_method},
};

fn no_async_lifecycle_method_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Angular lifecycle method should not be async.")
        .with_help("Angular does not await lifecycle methods; call an async function from a synchronous hook instead.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoAsyncLifecycleMethod;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows `async` lifecycle methods in Angular classes.
    ///
    /// ### Why is this bad?
    ///
    /// Angular does not wait for an async lifecycle method to settle before it continues the
    /// lifecycle. An `async ngOnInit` suggests that rendering waits for its `await`s, but it does
    /// not, so the component may render with incomplete data.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent {
    ///   async ngOnInit() { this.user = await this.api.load(); }
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent {
    ///   ngOnInit() { this.api.load().then((user) => (this.user = user)); }
    /// }
    /// ```
    NoAsyncLifecycleMethod,
    angular,
    suspicious,
    version = "next",
    short_description = "Disallow async Angular lifecycle methods.",
);

impl Rule for NoAsyncLifecycleMethod {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if !is_angular_class(class) {
            return;
        }
        for element in &class.body.body {
            let ClassElement::MethodDefinition(method) = element else { continue };
            if method.value.r#async
                && !method.r#type.is_abstract()
                && let PropertyKey::StaticIdentifier(key) = &method.key
                && is_angular_lifecycle_method(key.name.as_str())
            {
                ctx.diagnostic(no_async_lifecycle_method_diagnostic(key.span));
            }
        }
    }
}

// A class declaration with `@X(...)` for any Angular class decorator X.
fn is_angular_class(class: &Class) -> bool {
    class.is_declaration()
        && class.decorators.iter().any(|decorator| {
            decorator_call_name(decorator)
                .is_some_and(|name| ANGULAR_CLASS_DECORATORS.contains(&name))
        })
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    @Component()
    class Test {
      ngAfterContentChecked() { }
    }
  ",
        r"
    @Component()
    class Test {
      ngAfterContentInit() { }
    }
  ",
        r"
    @Component()
    class Test {
      ngAfterViewChecked() { }
    }
  ",
        r"
    @Component()
    class Test {
      ngAfterViewInit() { }
    }
  ",
        r"
    @Component()
    class Test {
      ngDoBootstrap() { }
    }
  ",
        r"
    @Component()
    class Test {
      ngDoCheck() { }
    }
  ",
        r"
    @Component()
    class Test {
      ngOnChanges() { }
    }
  ",
        r"
    @Component()
    class Test {
      ngOnDestroy() { }
    }
  ",
        r"
    @Component()
    class Test {
      ngOnInit() { }
    }
  ",
        r"
    async function ngAfterContentChecked() {}
    async function ngAfterContentInit() {}
    async function ngAfterViewChecked() {}
    async function ngAfterViewInit() {}
    async function ngDoBootstrap() {}
    async function ngDoCheck() {}
    async function ngOnChanges() {}
    async function ngOnDestroy() {}
    async function ngOnInit() {}
  ",
        r"
  class Test {
    async ngOnInit() {}
  }
  ",
    ];

    let fail = vec![
        r"
      @Component()
      class Test {
        async ngAfterContentChecked() { }
      }
    ",
        r"
      @Component()
      class Test {
        async ngAfterContentInit() { }
      }
    ",
        r"
      @Component()
      class Test {
        async ngAfterViewChecked() { }
      }
    ",
        r"
      @Component()
      class Test {
        async ngAfterViewInit() { }
      }
    ",
        r"
      @Pipe()
      class Test {
        async ngDoBootstrap() { }
      }
    ",
        r"
      @Component()
      class Test {
        async ngDoCheck() { }
      }
    ",
        r"
      @Component()
      class Test {
        async ngOnChanges() { }
      }
    ",
        r"
      @Component()
      class Test {
        async ngOnDestroy() { }
      }
    ",
        r"
      @Component()
      class Test {
        async ngOnInit() { }
      }
    ",
        r"
      @Service()
      class Test {
        async ngOnDestroy() { }
      }
    ",
    ];

    Tester::new(NoAsyncLifecycleMethod::NAME, NoAsyncLifecycleMethod::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
