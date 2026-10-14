use oxc_angular_compiler::ast::expression::AngularExpression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{context::LintContext, rule::Rule, utils::for_each_expression};

fn no_non_null_assertion_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Avoid using the non-null assertion operator (!) in templates. This bypasses type safety and can lead to runtime errors.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoNonNullAssertion;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the non-null assertion operator (`!`) in templates.
    ///
    /// ### Why is this bad?
    ///
    /// `!` tells the type checker that a value is neither `null` nor `undefined` without
    /// checking it, so a wrong assumption only shows up as a runtime error. Optional chaining,
    /// `??` or an `@if` let the template handle the missing value instead.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// {{ user!.name }}
    /// <div [title]="item!.label"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// {{ user?.name }}
    /// <div [title]="item?.label ?? ''"></div>
    /// ```
    NoNonNullAssertion,
    angular_template,
    restriction,
    version = "next",
    short_description = "Disallows the non-null assertion operator (!) in templates",
);

impl Rule for NoNonNullAssertion {
    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            for_each_expression(template.nodes, &mut |expression, _| {
                if let AngularExpression::NonNullAssert(assert) = expression {
                    ctx.diagnostic(no_non_null_assertion_diagnostic(
                        template.expression_span(assert.source_span),
                    ));
                }
            });
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "{{ foo }}",
        "{{ foo.bar }}",
        "{{ foo.bar.baz }}",
        "{{ foo?.bar }}",
        "{{ foo?.bar?.baz }}",
        "{{ user?.profile?.address?.city }}",
        "{{ foo ?? 'default' }}",
        "{{ (data?.items?.length ?? 0) > 0 }}",
        "{{ getValue() }}",
        "{{ foo?.getValue() }}",
        "{{ user?.getProfile()?.getName() }}",
        "{{ items[0] }}",
        "{{ items?.[0] }}",
        "{{ value | uppercase }}",
        "{{ user?.name | uppercase }}",
        "{{ value | pipe : arg }}",
        r#"<div [title]="user?.name"></div>"#,
        r#"<button (click)="handler(item)">Click</button>"#,
        "@if (condition) { <div>Content</div> }",
        "@if (user?.isActive) { <div>Active</div> }",
        "@for (item of items; track item.id) { <div>{{ item.name }}</div> }",
        "@for (item of data?.items; track item.id) { <div>{{ item.name }}</div> }",
        "@switch (status) { @case ('active') { <div>Active</div> } }",
        "@switch (user?.status) { @case ('active') { <div>Active</div> } }",
        "@let userId = 42;",
        "@let user = getUser();",
        r#"<div *ngIf="condition">Content</div>"#,
        "{{ a + b }}",
        "{{ value + 10 }}",
        "{{ a === b }}",
        "{{ value === 'test' }}",
        "{{ !value }}",
        "{{ a && b }}",
        "{{ condition ? value : other }}",
        "{{ condition ? 'yes' : 'no' }}",
        // `!=` and a negation are not assertions.
        "{{ a != b }}",
        "{{ !!a }}",
    ];

    let fail = vec![
        "{{ foo! }}",
        "{{ foo!.bar }}",
        "{{ user.name! | uppercase }}",
        "{{ user | uppercase : firstArg : secondArg! }}",
        "{{ user.name | uppercase : (prefix! | lowercase) }}",
        r#"<div [title]="user!.name"></div>"#,
        "@let user = a.b!.c;",
        "@let user = getUser()!;",
        "{{ x.y({ param: a.b!.c }) }}",
        "{{ getValue()! }}",
        "{{ foo.bar!.baz }}",
        "{{ foo?.bar!.baz }}",
        r#"<button (click)="handler(item!)">Click</button>"#,
        r#"<div *ngIf="foo!">Content</div>"#,
        "@if (user!) { <div>Content</div> }",
        "@if (user!.isActive) { <div>Active</div> }",
        "@for (item of items!; track item.id) { <div>{{ item.name }}</div> }",
        "@for (item of items; track item!.id) { <div>{{ item.name }}</div> }",
        "@switch (status!) { @case ('active') { <div>Active</div> } }",
        "@switch (status) { @case (activeStatus!) { <div>Active</div> } }",
        "{{ user.getProfile()!.getName() }}",
        "@if ((data!.items?.length ?? 0) > 0) { <div>Has items</div> }",
        "{{ user! ? user.name : 'Guest' }}",
        "{{ items![0] }}",
        "{{ items![0]?.[0] }}",
        "{{ items[0]! }}",
        "{{ value! + 10 }}",
        "{{ 10 + value! }}",
        "{{ value! === 'test' }}",
        "{{ !value! }}",
        "{{ condition! ? 'yes' : 'no' }}",
        "{{ condition ? value! : 'no' }}",
        "{{ a + b?.c! - d == 42 }}",
        "{{ a + b?.c! - d === 42 }}",
        "{{ a + b?.c! - d < 42 }}",
        "{{ a + b?.c! - d <= 42 }}",
        "{{ a + b?.c! - d > 42 }}",
        "{{ a + b?.c! - d >= 42 }}",
        "{{ condition ?? value! }}",
        "{{ condition ? 'yes' : value! }}",
        "{{ service!.getData() }}",
        "{{ x[y]![z] }}",
        "{{ x[y][z]! }}",
        "{{ x.y.z!() }}",
        "{{ x.y?.z!() }}",
        // Reported once, though the structural template holds a copy of the input.
        r#"<div *ngIf="show" [title]="user!.name"></div>"#,
        r#"<ng-template [ngIf]="user!"><p>x</p></ng-template>"#,
        "@defer (when ready!) { <x-cmp/> }",
    ];

    Tester::new(NoNonNullAssertion::NAME, NoNonNullAssertion::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '{{ user?.name }}' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '{{ user!.name }}' }) class A {}",
    ];
    Tester::new(NoNonNullAssertion::NAME, NoNonNullAssertion::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
