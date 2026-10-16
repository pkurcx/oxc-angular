use oxc_ast::{
    AstKind,
    ast::{ClassElement, PropertyKey},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        angular_class_decorator, implemented_interface_names, implements_insertion,
        is_angular_lifecycle_method, named_import_insertion,
    },
};

fn use_lifecycle_interface_diagnostic(span: Span, interface: &str, method: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Lifecycle interface '{interface}' should be implemented for method '{method}'."
    ))
    .with_help(format!(
        "Add `implements {interface}` to the class. See https://angular.dev/style-guide#use-lifecycle-hook-interfaces"
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct UseLifecycleInterface;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that Angular classes implement the lifecycle interface of every lifecycle method
    /// they declare, such as `OnInit` for `ngOnInit`. Methods marked `override` are skipped,
    /// since the base class is responsible for the interface.
    ///
    /// ### Why is this bad?
    ///
    /// The interface lets TypeScript check the method's name and signature, so a typo such as
    /// `ngOninit` is caught at compile time. It also documents which hooks the class uses.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent {
    ///   ngOnInit() {}
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// import { OnInit } from '@angular/core';
    ///
    /// @Component({})
    /// class UserComponent implements OnInit {
    ///   ngOnInit() {}
    /// }
    /// ```
    UseLifecycleInterface,
    angular,
    style,
    fix,
    version = "next",
    short_description = "Ensure that classes implement the lifecycle interfaces of their lifecycle methods.",
);

impl Rule for UseLifecycleInterface {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if angular_class_decorator(class).is_none() {
            return;
        }
        for element in &class.body.body {
            let ClassElement::MethodDefinition(method) = element else { continue };
            if method.r#override || method.r#type.is_abstract() {
                continue;
            }
            let PropertyKey::StaticIdentifier(key) = &method.key else { continue };
            let method_name = key.name.as_str();
            if !is_angular_lifecycle_method(method_name) {
                continue;
            }
            let interface = &method_name[2..];
            if implemented_interface_names(class).any(|name| name == interface) {
                continue;
            }
            ctx.diagnostic_with_fix(
                use_lifecycle_interface_diagnostic(key.span, interface, method_name),
                |fixer| {
                    let fixer = fixer.for_multifix();
                    let mut fix = fixer.new_fix_with_capacity(2);
                    if let Some((span, text)) = named_import_insertion(
                        ctx.nodes().program(),
                        "@angular/core",
                        interface,
                        false,
                    ) {
                        fix.push(fixer.insert_text_after_range(span, text));
                    }
                    let (span, text) = implements_insertion(class, interface);
                    fix.push(fixer.insert_text_after_range(span, text));
                    fix.with_message(format!("Implement `{interface}`"))
                },
            );
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    class Test implements OnInit {
      ngOnInit() {}
    }
    ",
        r"class Test implements DoBootstrap {
      ngDoBootstrap() {}
    }
    ",
        r"
    class Test extends Component implements OnInit, OnDestroy  {
      ngOnInit() {}

      private ngOnChanges = '';

      ngOnDestroy() {}

      ngOnSmth() {}
    }
    ",
        r"
    class Test extends Component implements ng.OnInit, ng.OnDestroy  {
      ngOnInit() {}

      private ngOnChanges = '';

      ngOnDestroy() {}

      ngOnSmth() {}
    }
    ",
        r"
    @Directive()
    class FoobarBase implements OnDestroy {
      ngOnDestroy(): void {
        /* some base logic here */
      }
    }

    @Component()
    class FoobarComponent extends FoobarBase {
      override ngOnDestroy(): void {
        super.ngOnDestroy();
        /* some concrete logic here */
      }
    }
  ",
        r"
    class BaseClass {
      ngOnInit(): void {
        /* base initialization */
      }
    }

    @Component()
    class DerivedComponent extends BaseClass {
      override ngOnInit(): void {
        super.ngOnInit();
        /* derived initialization */
      }
    }
  ",
        r"
    @Directive()
    class BaseDirective implements OnInit {
      ngOnInit(): void {
        /* base initialization */
      }
    }

    @Component()
    class DerivedComponent extends BaseDirective {
      override ngOnInit(): void {
        super.ngOnInit();
        /* derived initialization */
      }
    }
  ",
        r"class Test {}",
    ];

    let fail = vec![
        r"
        @Component()
        class Test {
          ngOnInit() {
          }
        }
      ",
        r"import { OnInit } from '@angular/core';

        @Directive()
        class Test extends Component implements OnInit {
          ngOnInit() {}

          ngOnDestroy() {
          }
        }
      ",
        r"
        @Injectable()
        class Test {
          ngDoBootstrap() {}

          ngOnInit() {}

          ngOnDestroy() {}
        }
      ",
        r"
        @NgModule()
        class Test extends Component implements ng.OnInit {
          ngOnInit() {}

          ngOnDestroy() {
          }
        }
      ",
        r"
        @NgModule()
        class Test extends Component {
          ngOnInit() {
          }
        }
      ",
        r"
        @Directive()
        class FoobarBase implements OnDestroy {
          ngOnDestroy(): void {
            /* some base logic here */
          }
        }

        @Component()
        class FoobarComponent extends FoobarBase {
          ngOnDestroy(): void {
            super.ngOnDestroy();
            /* some concrete logic here */
          }
        }
      ",
        r"
        @Service()
        class Test {
          ngOnDestroy() {
          }
        }
      ",
        r"
        @Component()
        class Test<T> {
          ngOnInit() {
          }
        }
      ",
        r"
        @Injectable()
        class Test<A, B extends C = D> {
          ngOnInit() {
          }
        }
      ",
    ];

    let fix = vec![
        (
            r"
        @Component()
        class Test {
          ngOnInit() {
          }
        }
      ",
            r"import { OnInit } from '@angular/core';

        @Component()
        class Test implements OnInit {
          ngOnInit() {
          }
        }
      ",
        ),
        (
            r"import { OnInit } from '@angular/core';

        @Directive()
        class Test extends Component implements OnInit {
          ngOnInit() {}

          ngOnDestroy() {
          }
        }
      ",
            r"import { OnInit, OnDestroy } from '@angular/core';

        @Directive()
        class Test extends Component implements OnInit, OnDestroy {
          ngOnInit() {}

          ngOnDestroy() {
          }
        }
      ",
        ),
        (
            r"
        @Injectable()
        class Test {
          ngDoBootstrap() {}

          ngOnInit() {}

          ngOnDestroy() {}
        }
      ",
            r"import { DoBootstrap } from '@angular/core';

        @Injectable()
        class Test implements DoBootstrap {
          ngDoBootstrap() {}

          ngOnInit() {}

          ngOnDestroy() {}
        }
      ",
        ),
        (
            r"
        @NgModule()
        class Test extends Component implements ng.OnInit {
          ngOnInit() {}

          ngOnDestroy() {
          }
        }
      ",
            r"import { OnDestroy } from '@angular/core';

        @NgModule()
        class Test extends Component implements ng.OnInit, OnDestroy {
          ngOnInit() {}

          ngOnDestroy() {
          }
        }
      ",
        ),
        (
            r"
        @NgModule()
        class Test extends Component {
          ngOnInit() {
          }
        }
      ",
            r"import { OnInit } from '@angular/core';

        @NgModule()
        class Test extends Component implements OnInit {
          ngOnInit() {
          }
        }
      ",
        ),
        (
            r"
        @Directive()
        class FoobarBase implements OnDestroy {
          ngOnDestroy(): void {
            /* some base logic here */
          }
        }

        @Component()
        class FoobarComponent extends FoobarBase {
          ngOnDestroy(): void {
            super.ngOnDestroy();
            /* some concrete logic here */
          }
        }
      ",
            r"import { OnDestroy } from '@angular/core';

        @Directive()
        class FoobarBase implements OnDestroy {
          ngOnDestroy(): void {
            /* some base logic here */
          }
        }

        @Component()
        class FoobarComponent extends FoobarBase implements OnDestroy {
          ngOnDestroy(): void {
            super.ngOnDestroy();
            /* some concrete logic here */
          }
        }
      ",
        ),
        (
            r"
        @Service()
        class Test {
          ngOnDestroy() {
          }
        }
      ",
            r"import { OnDestroy } from '@angular/core';

        @Service()
        class Test implements OnDestroy {
          ngOnDestroy() {
          }
        }
      ",
        ),
        (
            r"
        @Component()
        class Test<T> {
          ngOnInit() {
          }
        }
      ",
            r"import { OnInit } from '@angular/core';

        @Component()
        class Test<T> implements OnInit {
          ngOnInit() {
          }
        }
      ",
        ),
        (
            r"
        @Injectable()
        class Test<A, B extends C = D> {
          ngOnInit() {
          }
        }
      ",
            r"import { OnInit } from '@angular/core';

        @Injectable()
        class Test<A, B extends C = D> implements OnInit {
          ngOnInit() {
          }
        }
      ",
        ),
    ];

    Tester::new(UseLifecycleInterface::NAME, UseLifecycleInterface::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
