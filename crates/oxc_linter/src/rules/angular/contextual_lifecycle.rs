use oxc_ast::{AstKind, ast::ClassElement};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{decorator_call_name, is_angular_lifecycle_method},
};

fn contextual_lifecycle_diagnostic(span: Span, method: &str, decorator: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Angular will not invoke the `{method}` lifecycle method within `@{decorator}()` classes."
    ))
    .with_help("Remove the method, or move it to a class type whose lifecycle includes it.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct ContextualLifecycle;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that lifecycle methods are only declared in classes whose Angular decorator makes
    /// Angular invoke them: `ngDoBootstrap` only in `@NgModule`, `ngOnDestroy` only in
    /// `@Component`, `@Directive`, `@Injectable`, `@Pipe` and `@Service`, and the other hooks only
    /// in `@Component` and `@Directive`.
    ///
    /// ### Why is this bad?
    ///
    /// A lifecycle method in the wrong kind of class is never called, so its initialization or
    /// cleanup logic silently never runs.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Injectable()
    /// class UserService {
    ///   ngOnInit() { this.load(); }
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Injectable()
    /// class UserService {
    ///   ngOnDestroy() { this.subscription.unsubscribe(); }
    /// }
    /// ```
    ContextualLifecycle,
    angular,
    correctness,
    version = "next",
    short_description = "Ensure that lifecycle methods are used in a correct context.",
);

impl Rule for ContextualLifecycle {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if !class.is_declaration() {
            return;
        }
        for decorator_name in class.decorators.iter().filter_map(decorator_call_name) {
            for element in &class.body.body {
                let ClassElement::MethodDefinition(method) = element else { continue };
                if method.r#type.is_abstract() {
                    continue;
                }
                let Some(name) = method.key.static_name() else { continue };
                if is_angular_lifecycle_method(&name)
                    && invoked_by_angular(decorator_name, &name) == Some(false)
                {
                    ctx.diagnostic(contextual_lifecycle_diagnostic(
                        method.key.span(),
                        &name,
                        decorator_name,
                    ));
                }
            }
        }
    }
}

// `None` for decorators Angular does not manage.
fn invoked_by_angular(decorator: &str, method: &str) -> Option<bool> {
    match decorator {
        "Component" | "Directive" => Some(method != "ngDoBootstrap"),
        "Injectable" | "Pipe" | "Service" => Some(method == "ngOnDestroy"),
        "NgModule" => Some(method == "ngDoBootstrap"),
        _ => None,
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    @Injectable()
    class Test {
      ngOnDestroy() { console.log('OnDestroy'); }
    }
    ",
        r"
    @Component()
    class Test {
      ngAfterContentChecked() { console.log('AfterContentChecked'); }
    }
    ",
        r"
    @Component()
    class Test {
      ngAfterContentInit() { console.log('AfterContentInit'); }
    }
    ",
        r"
    @Component()
    class Test {
      ngAfterViewChecked() { console.log('AfterViewChecked'); }
    }
    ",
        r"
    @Component()
    class Test {
      ngAfterViewInit() { console.log('AfterViewInit'); }
    }
    ",
        r"
    @Component()
    class Test {
      ngDoCheck() { console.log('DoCheck'); }
    }",
        r"
    @Component()
    class Test {
      ngOnChanges() { console.log('OnChanges'); }
    }
    ",
        r"
    @Component()
    class Test {
      ngOnDestroy() { console.log('OnDestroy'); }
    }
    ",
        r"
    @Component()
    class Test {
      ngOnInit() { console.log('OnInit'); }
    }
    ",
        r"
    @Directive()
    class Test {
      ngAfterContentChecked() { console.log('AfterContentChecked'); }
    }
    ",
        r"
    @Directive()
    class Test {
      ngAfterContentInit() { console.log('AfterContentInit'); }
    }
    ",
        r"
    @Directive()
    class Test {
      ngAfterViewChecked() { console.log('AfterViewChecked'); }
    }
    ",
        r"
    @Directive()
    class Test {
      ngAfterViewInit() { console.log('AfterViewInit'); }
    }
    ",
        r"
    @Directive()
    class Test {
      ngDoCheck() { console.log('DoCheck'); }
    }
    ",
        r"
    @Directive()
    class Test {
      ngOnChanges() { console.log('OnChanges'); }
    }
    ",
        r"
    @Directive()
    class Test {
      ngOnDestroy() { console.log('OnDestroy'); }
    }
    ",
        r"
    @Directive()
    class Test {
      ngOnInit() { console.log('OnInit'); }
    }
    ",
        r"
    @NgModule()
    class Test {
      ngDoBootstrap() {}
    }
    ",
        r"
    @Pipe()
    class Test {
      ngOnDestroy() { console.log('OnDestroy'); }
    }
    ",
        r"
    @Service()
    class Test {
      ngOnDestroy() { console.log('OnDestroy'); }
    }
    ",
    ];

    let fail = vec![
        r"
        @Component()
        class Test {
          ngDoBootstrap() {}
        }
      ",
        r"
        @Directive()
        class Test {
          ngDoBootstrap() {}
        }
      ",
        r"
        @Injectable()
        class Test {
          ngAfterContentChecked() { console.log('AfterContentChecked'); }
        }
      ",
        r"
        @Injectable()
        class Test {
          ngAfterContentInit() { console.log('ngAfterContentInit'); }
        }
      ",
        r"
        @Injectable()
        class Test {
          ngAfterViewInit() { console.log('ngAfterViewInit'); }
        }
      ",
        r"
        @Injectable()
        class Test {
          ngDoBootstrap() {}
        }
      ",
        r"
        @Injectable()
        class Test {
          ngDoCheck() { console.log('ngDoCheck'); }
        }
      ",
        r"
        @Injectable()
        class Test {
          ngOnChanges() { console.log('ngOnChanges'); }
        }
      ",
        r"
        @Injectable()
        class Test {
          ngOnInit() { console.log('ngOnInit'); }
        }
      ",
        r"
        @NgModule()
        class Test {
          ngAfterContentChecked() { console.log('ngAfterContentChecked'); }
        }
      ",
        r"
        @NgModule()
        class Test {
          ngAfterContentInit() { console.log('ngAfterContentInit'); }
        }
      ",
        r"
        @NgModule()
        class Test {
          ngAfterViewChecked() { console.log('ngAfterViewChecked'); }
        }
      ",
        r"
        @NgModule()
        class Test {
          ngAfterViewInit() { console.log('ngAfterViewInit'); }
        }
      ",
        r"
        @NgModule()
        class Test {
          ngDoCheck() { console.log('ngDoCheck'); }
        }
      ",
        r"
        @NgModule()
        class Test {
          ngOnChanges() { console.log('ngOnChanges'); }
        }
      ",
        r"
        @NgModule()
        class Test {
          ngOnInit() { console.log('ngOnInit'); }
        }
      ",
        r"
        @NgModule()
        class Test {
          ngOnDestroy() { console.log('ngOnDestroy'); }
        }
      ",
        r"
        @Pipe()
        class Test {
          ngAfterContentChecked() { console.log('ngAfterContentChecked'); }
        }
      ",
        r"
        @Pipe()
        class Test {
          ngAfterContentInit() { console.log('ngAfterContentInit'); }
        }
      ",
        r"
        @Pipe()
        class Test {
          ngAfterViewChecked() { console.log('ngAfterViewChecked'); }
        }
      ",
        r"
        @Pipe()
        class Test {
          ngAfterViewInit() { console.log('ngAfterViewInit'); }
        }
      ",
        r"
        @Pipe()
        class Test {
          ngDoBootstrap() {}
        }
      ",
        r"
        @Pipe()
        class Test {
          ngDoCheck() { console.log('ngDoCheck'); }
        }
      ",
        r"
        @Pipe()
        class Test {
          ngOnChanges() { console.log('ngOnChanges'); }
        }
      ",
        r"
        @Pipe()
        class Test {
          ngOnInit() { console.log('ngOnInit'); }
        }
      ",
        r"
        @Service()
        class Test {
          ngOnInit() { console.log('ngOnInit'); }
        }
      ",
        r"
        @Service()
        class Test {
          ngAfterViewInit() { console.log('ngAfterViewInit'); }
        }
      ",
        r"
        @Pipe()
        class Test implements DoCheck {
          constructor() {}
          
          ngDoCheck() {}
        }
 
        @Directive()
        class TestDirective implements OnInit {
          ngOnInit() {
            console.log('Initialized');
          }
        }
      ",
    ];

    Tester::new(ContextualLifecycle::NAME, ContextualLifecycle::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
