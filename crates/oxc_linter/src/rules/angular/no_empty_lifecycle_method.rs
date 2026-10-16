use oxc_ast::{
    AstKind,
    ast::{Class, ClassElement, TSTypeName},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        ANGULAR_CLASS_DECORATORS, decorator_call_name, is_angular_lifecycle_method,
        named_import_removal_span,
    },
};

fn no_empty_lifecycle_method_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Lifecycle methods should not be empty.")
        .with_help("Remove the lifecycle method.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoEmptyLifecycleMethod;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows empty lifecycle methods in Angular classes.
    ///
    /// ### Why is this bad?
    ///
    /// An empty lifecycle method does nothing, yet readers expect initialization or cleanup logic
    /// in it. Such methods are usually left over after a refactoring, along with the now unneeded
    /// lifecycle interface and its import.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent implements OnInit {
    ///   ngOnInit() {}
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent implements OnInit {
    ///   ngOnInit() { this.load(); }
    /// }
    /// ```
    NoEmptyLifecycleMethod,
    angular,
    style,
    suggestion,
    version = "next",
    short_description = "Disallow empty lifecycle methods.",
);

impl Rule for NoEmptyLifecycleMethod {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if !is_angular_class(class) {
            return;
        }
        for element in &class.body.body {
            let ClassElement::MethodDefinition(method) = element else { continue };
            if method.r#type.is_abstract() {
                continue;
            }
            // A computed identifier such as `[ngOnInit]` has no static name, as upstream requires.
            let Some(name) = method.key.static_name() else { continue };
            if !is_angular_lifecycle_method(&name) {
                continue;
            }
            let Some(body) = &method.value.body else { continue };
            if !body.statements.is_empty() || !body.directives.is_empty() {
                continue;
            }
            ctx.diagnostic_with_suggestion(
                no_empty_lifecycle_method_diagnostic(method.span),
                |fixer| {
                    let interface = &name[2..];
                    let source = ctx.source_text();
                    let fixer = fixer.for_multifix();
                    let mut fix = fixer.new_fix_with_capacity(3);
                    fix.push(fixer.delete_range(method.span));
                    if let Some(span) = implements_removal_span(source, class, interface) {
                        fix.push(fixer.delete_range(span));
                    }
                    // Upstream heuristic: more than two mentions means the interface is still used.
                    if interface_occurrences(source, interface) <= 2
                        && let Some(span) = named_import_removal_span(
                            ctx.nodes().program(),
                            "@angular/core",
                            interface,
                        )
                    {
                        fix.push(fixer.delete_range(span));
                    }
                    fix.with_message("Remove lifecycle method")
                },
            );
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

// The only entry goes with its `implements` keyword; otherwise one adjacent comma goes with it.
fn implements_removal_span(source: &str, class: &Class, interface: &str) -> Option<Span> {
    let entries = &class.implements;
    let index = entries.iter().position(|entry| match &entry.expression {
        TSTypeName::IdentifierReference(ident) => ident.name == interface,
        TSTypeName::QualifiedName(name) => name.right.name == interface,
        TSTypeName::ThisExpression(_) => false,
    })?;
    let entry = entries[index].span;
    if entries.len() == 1 {
        let keyword_start =
            source[..entry.start as usize].trim_end().strip_suffix("implements")?.len();
        return Some(Span::new(u32::try_from(keyword_start).ok()?, entry.end));
    }
    if index == 0 {
        let comma = source[entry.end as usize..].find(',')?;
        return Some(Span::new(entry.start, entry.end + u32::try_from(comma).ok()? + 1));
    }
    let previous_end = entries[index - 1].span.end;
    let comma = source[previous_end as usize..entry.start as usize].rfind(',')?;
    Some(Span::new(previous_end + u32::try_from(comma).ok()?, entry.end))
}

// Space-separated words that equal `interface` once non-word characters are stripped.
fn interface_occurrences(source: &str, interface: &str) -> usize {
    source
        .split(' ')
        .filter(|word| {
            word.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').eq(interface.chars())
        })
        .count()
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    @Component()
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
    @Injectable()
    class Test {
      ngAfterViewChecked() { console.log('AfterViewChecked'); }
    }
  ",
        r"
    @NgModule()
    class Test {
      ngAfterViewInit() { console.log('AfterViewInit'); }
    }
  ",
        r"
    @Pipe()
    class Test {
      ngDoBootstrap() { console.log('DoBootstrap'); }
    }
  ",
        r"
    @Component()
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
    @Injectable()
    class Test {
      ngOnDestroy() { console.log('OnDestroy'); }
    }
  ",
        r"
    @NgModule()
    class Test {
      ngOnInit() { console.log('OnInit'); }
    }
  ",
        r"
    @Component()
    class Test {
      [ngOnInit]() {}
    }
  ",
        r"
    class Test {
      ngOnInit() {}
    }
  ",
    ];

    let fail = vec![
        r"
      @Component()
      class Test {
        ngAfterContentChecked() {}
      }
    ",
        r"
      @Directive()
      class Test extends BaseDirective {
        ngAfterContentInit() {}
      }
    ",
        r"
      @Injectable()
      class Test extends BaseTest implements AfterViewChecked, OnDestroy {
        'ngAfterViewChecked'() {}
      }
    ",
        r"
      @NgModule()
      class Test {
        ['ngAfterViewInit']() {}
      }
    ",
        r"
      import { HttpInterceptor } from '@angular/common/http';
      import {
        DoCheck,
        DoBootstrap
      } from '@angular/core';
      @Pipe()
      class Test {
        [`ngDoBootstrap`]() {}
      }
    ",
        r"
      import {
        AfterViewChecked,
        DoCheck,
        AfterViewInit,
      } from '@angular/core';
      @Component()
      class Test 
          implements AfterViewChecked,
                      AfterViewInit,
                      DoCheck {
        ngDoCheck() {}
      }
    ",
        r"
      import {OnChanges, AfterContentInit} from '@angular/core';
      @Directive()
      class Test implements OnChanges, AfterContentInit {
        ngOnChanges() {}
      }
    ",
        r"
      import * as ng from '@angular/core';
      @Injectable()
      class Test implements ng.OnDestroy {
        ngOnDestroy() {}
      }
    ",
        r"
      import {DoBootstrap, OnInit} from '@angular/core';
      @NgModule()
      class Test implements OnInit, DoBootstrap {
        ngOnInit() {
          this.init();
        }
      }

      @NgModule()
      class Test2 implements OnInit, DoBootstrap {
        ngOnInit() {}
      }
    ",
        r"
      @Component()
      class Test extends BaseComponent<unknown> implements OnInit {
        ngOnInit() {}
      }
    ",
        r"
      @Service()
      class Test {
        ngOnDestroy() {}
      }
    ",
    ];

    let fix = vec![
        (
            r"
      @Component()
      class Test {
        ngAfterContentChecked() {}
      }
    ",
            r"
      @Component()
      class Test {
        
      }
    ",
        ),
        (
            r"
      @Directive()
      class Test extends BaseDirective {
        ngAfterContentInit() {}
      }
    ",
            r"
      @Directive()
      class Test extends BaseDirective {
        
      }
    ",
        ),
        (
            r"
      @Injectable()
      class Test extends BaseTest implements AfterViewChecked, OnDestroy {
        'ngAfterViewChecked'() {}
      }
    ",
            r"
      @Injectable()
      class Test extends BaseTest implements  OnDestroy {
        
      }
    ",
        ),
        (
            r"
      @NgModule()
      class Test {
        ['ngAfterViewInit']() {}
      }
    ",
            r"
      @NgModule()
      class Test {
        
      }
    ",
        ),
        (
            r"
      import { HttpInterceptor } from '@angular/common/http';
      import {
        DoCheck,
        DoBootstrap
      } from '@angular/core';
      @Pipe()
      class Test {
        [`ngDoBootstrap`]() {}
      }
    ",
            r"
      import { HttpInterceptor } from '@angular/common/http';
      import {
        DoCheck
      } from '@angular/core';
      @Pipe()
      class Test {
        
      }
    ",
        ),
        (
            r"
      import {
        AfterViewChecked,
        DoCheck,
        AfterViewInit,
      } from '@angular/core';
      @Component()
      class Test 
          implements AfterViewChecked,
                      AfterViewInit,
                      DoCheck {
        ngDoCheck() {}
      }
    ",
            r"
      import {
        AfterViewChecked,
        AfterViewInit,
      } from '@angular/core';
      @Component()
      class Test 
          implements AfterViewChecked,
                      AfterViewInit {
        
      }
    ",
        ),
        (
            r"
      import {OnChanges, AfterContentInit} from '@angular/core';
      @Directive()
      class Test implements OnChanges, AfterContentInit {
        ngOnChanges() {}
      }
    ",
            r"
      import { AfterContentInit} from '@angular/core';
      @Directive()
      class Test implements  AfterContentInit {
        
      }
    ",
        ),
        (
            r"
      import * as ng from '@angular/core';
      @Injectable()
      class Test implements ng.OnDestroy {
        ngOnDestroy() {}
      }
    ",
            r"
      import * as ng from '@angular/core';
      @Injectable()
      class Test  {
        
      }
    ",
        ),
        (
            r"
      import {DoBootstrap, OnInit} from '@angular/core';
      @NgModule()
      class Test implements OnInit, DoBootstrap {
        ngOnInit() {
          this.init();
        }
      }

      @NgModule()
      class Test2 implements OnInit, DoBootstrap {
        ngOnInit() {}
      }
    ",
            r"
      import {DoBootstrap, OnInit} from '@angular/core';
      @NgModule()
      class Test implements OnInit, DoBootstrap {
        ngOnInit() {
          this.init();
        }
      }

      @NgModule()
      class Test2 implements  DoBootstrap {
        
      }
    ",
        ),
        (
            r"
      @Component()
      class Test extends BaseComponent<unknown> implements OnInit {
        ngOnInit() {}
      }
    ",
            r"
      @Component()
      class Test extends BaseComponent<unknown>  {
        
      }
    ",
        ),
        (
            r"
      @Service()
      class Test {
        ngOnDestroy() {}
      }
    ",
            r"
      @Service()
      class Test {
        
      }
    ",
        ),
    ];

    Tester::new(NoEmptyLifecycleMethod::NAME, NoEmptyLifecycleMethod::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
