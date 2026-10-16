use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        class_decorator, implemented_interface_names, implements_insertion, named_import_insertion,
    },
};

const PIPE_TRANSFORM: &str = "PipeTransform";

fn use_pipe_transform_interface_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Pipes should implement `PipeTransform` interface")
        .with_help("Add `implements PipeTransform` to the `@Pipe` class.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct UsePipeTransformInterface;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that classes decorated with `@Pipe` implement the `PipeTransform` interface.
    ///
    /// ### Why is this bad?
    ///
    /// `PipeTransform` lets TypeScript check the signature of the `transform()` method. Without
    /// it, a misspelled method name or wrong parameters only surface at runtime.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Pipe({ name: 'test' })
    /// class TestPipe {
    ///   transform(value: string) {}
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// import { PipeTransform } from '@angular/core';
    ///
    /// @Pipe({ name: 'test' })
    /// class TestPipe implements PipeTransform {
    ///   transform(value: string) {}
    /// }
    /// ```
    UsePipeTransformInterface,
    angular,
    style,
    fix,
    version = "next",
    short_description = "Ensure that pipes implement the `PipeTransform` interface.",
);

impl Rule for UsePipeTransformInterface {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if class_decorator(class, "Pipe").is_none()
            || implemented_interface_names(class).any(|name| name == PIPE_TRANSFORM)
        {
            return;
        }
        let span = class.id.as_ref().map_or(class.span, |id| id.span);
        ctx.diagnostic_with_fix(use_pipe_transform_interface_diagnostic(span), |fixer| {
            let mut fix = fixer.new_fix_with_capacity(2);
            if let Some((span, text)) =
                named_import_insertion(ctx.nodes().program(), "@angular/core", PIPE_TRANSFORM, true)
            {
                fix.push(fixer.insert_text_after_range(span, text));
            }
            let (span, text) = implements_insertion(class, PIPE_TRANSFORM);
            fix.push(fixer.insert_text_after_range(span, text));
            fix.with_message("Implement `PipeTransform`")
        });
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "@Component({ template: 'test' })\nclass Test {}",
        "@Pipe({ name: 'test' })\nclass Test implements PipeTransform {\n  transform(value: string) {}\n}",
        "@OtherDecorator() @Pipe({ name: 'test' })\nclass Test implements PipeTransform {\n  transform(value: string) {}\n}",
        "@Pipe({ name: 'test' })\nclass Test implements ng.PipeTransform {\n  transform(value: string) {}\n}",
    ];

    let fail = vec![
        "@Pipe({ name: 'test' })\nclass Test {\n  transform(value: string) {}\n}",
        "import { HttpClient } from '@angular/common/http';
import type { PipeTransform } from '@angular/core';
import { Component,
  Pipe,
  Directive } from '@angular/core';

@Pipe({ name: 'test' })
class Test implements AnInterface {
  transform(value: string) {}
}",
        "import type { OnInit } from '@angular/core';

@OtherDecorator() @Pipe({ name: 'test' })
class Test implements AnInterface, AnotherInterface {
  transform(value: string) {}
}",
        "@Pipe({ name: 'test' })\nclass Test<T> {\n  transform(value: T): T {}\n}",
        "@Pipe({ name: 'test' })\nclass Test<T, U extends SomeType> {\n  transform(value: T): U {}\n}",
    ];

    let fix = vec![
        (
            "@Pipe({ name: 'test' })\nclass Test {\n  transform(value: string) {}\n}",
            "import { PipeTransform } from '@angular/core';\n@Pipe({ name: 'test' })\nclass Test implements PipeTransform {\n  transform(value: string) {}\n}",
        ),
        (
            "import { HttpClient } from '@angular/common/http';
import type { PipeTransform } from '@angular/core';
import { Component,
  Pipe,
  Directive } from '@angular/core';

@Pipe({ name: 'test' })
class Test implements AnInterface {
  transform(value: string) {}
}",
            "import { HttpClient } from '@angular/common/http';
import type { PipeTransform } from '@angular/core';
import { Component,
  Pipe,
  Directive } from '@angular/core';

@Pipe({ name: 'test' })
class Test implements AnInterface, PipeTransform {
  transform(value: string) {}
}",
        ),
        (
            "import type { OnInit } from '@angular/core';

@OtherDecorator() @Pipe({ name: 'test' })
class Test implements AnInterface, AnotherInterface {
  transform(value: string) {}
}",
            "import type { OnInit, PipeTransform } from '@angular/core';

@OtherDecorator() @Pipe({ name: 'test' })
class Test implements AnInterface, AnotherInterface, PipeTransform {
  transform(value: string) {}
}",
        ),
        (
            "@Pipe({ name: 'test' })\nclass Test<T> {\n  transform(value: T): T {}\n}",
            "import { PipeTransform } from '@angular/core';\n@Pipe({ name: 'test' })\nclass Test<T> implements PipeTransform {\n  transform(value: T): T {}\n}",
        ),
        (
            "@Pipe({ name: 'test' })\nclass Test<T, U extends SomeType> {\n  transform(value: T): U {}\n}",
            "import { PipeTransform } from '@angular/core';\n@Pipe({ name: 'test' })\nclass Test<T, U extends SomeType> implements PipeTransform {\n  transform(value: T): U {}\n}",
        ),
    ];

    Tester::new(UsePipeTransformInterface::NAME, UsePipeTransformInterface::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
