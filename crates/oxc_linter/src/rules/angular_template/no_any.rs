use oxc_angular_compiler::ast::expression::AngularExpression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{context::LintContext, rule::Rule, utils::for_each_expression};

fn no_any_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Avoid using \"$any\" in templates").with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoAny;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the `$any()` type cast function in templates.
    ///
    /// ### Why is this bad?
    ///
    /// `$any()` turns off type checking for an expression, so typos and wrong property access
    /// are only found at runtime.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// {{ $any(framework).name }}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// {{ framework.name }}
    /// ```
    NoAny,
    angular_template,
    restriction,
    suggestion,
    version = "next",
    short_description = "Disallows the `$any()` type cast function in templates",
);

impl Rule for NoAny {
    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            for_each_expression(template.nodes, &mut |expression, _| {
                let AngularExpression::Call(call) = expression else { return };
                let AngularExpression::PropertyRead(callee) = &call.receiver else { return };
                if callee.name != "$any"
                    || !matches!(
                        callee.receiver,
                        AngularExpression::ImplicitReceiver(_) | AngularExpression::ThisReceiver(_)
                    )
                {
                    return;
                }
                let span = template.expression_span(call.source_span);
                let name = template.expression_span(callee.name_span);
                ctx.diagnostic_with_suggestion(no_any_diagnostic(span), |fixer| {
                    // Keep only the argument list's contents: drop `$any(` and the closing `)`.
                    let inner = ctx.source_range(Span::new(name.end + 1, span.end - 1));
                    fixer
                        .replace(Span::new(name.start, span.end), inner.to_string())
                        .with_message("Remove $any")
                });
            });
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "{{ $any }}",
        "{{ obj.$any() }}",
        "{{ obj?.x?.y!.z!.$any() }}",
        "{{ obj[read][$any] }}",
        "{{ $any['test'] }}",
        "{{ obj['test'].$any() }}",
        r#"<a [href]="$test()">Click here</a>"#,
        r#"<button type="button" (click)="anyClick()">Click here</button>"#,
    ];

    let fail = vec![
        "{{ $any(framework).name }}",
        "{{ this.$any(framework).name }}",
        r#"<a [href]="$any(getHref())">Click here</a>"#,
        r#"<div *ngIf="$any(attributeList)['NPSScore']">Content</div>"#,
        r#"<button (click)="$any(this).x = 1"></button>"#,
        "@defer (prefetch when $any(x).y) { <x-cmp/> }",
    ];

    let fix = vec![
        ("{{ $any(framework).name }}", "{{ framework.name }}"),
        ("{{ this.$any(framework).name }}", "{{ this.framework.name }}"),
        (r#"<a [href]="$any(getHref())"></a>"#, r#"<a [href]="getHref()"></a>"#),
    ];

    Tester::new(NoAny::NAME, NoAny::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
