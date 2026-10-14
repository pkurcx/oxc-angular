use oxc_angular_compiler::ast::expression::AngularExpression;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{context::LintContext, fixer::RuleFixer, rule::Rule, utils::for_each_expression};

fn negated_async_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Async pipe results should not be negated.")
        .with_help("Use `(observable | async) === false`, `(observable | async) === null`, or `(observable | async) === undefined` to check its value instead")
        .with_label(span)
}

fn negated_value_for_async_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Values used with the async pipe should not be negated.").with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoNegatedAsync;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that async pipe results, as well as values used with the async pipe, are not negated.
    ///
    /// ### Why is this bad?
    ///
    /// The async pipe emits `null` until the observable emits or the promise resolves, so
    /// `!(value$ | async)` is briefly `true` and can render, or fetch for, a branch that should
    /// stay hidden.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div *ngIf="!(loaded$ | async)"></div>
    /// <button [disabled]="!disabled$ | async"></button>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div *ngIf="(loaded$ | async) === false"></div>
    /// <button [disabled]="disabled$ | async"></button>
    /// ```
    NoNegatedAsync,
    angular_template,
    correctness,
    suggestion,
    version = "next",
    short_description = "Ensures that async pipe results, as well as values used with the async pipe, are not negated",
);

impl Rule for NoNegatedAsync {
    fn run_once(&self, ctx: &LintContext) {
        let fixer = RuleFixer::new(crate::fixer::FixKind::Suggestion, ctx);
        for template in ctx.angular_templates() {
            for_each_expression(template.nodes, &mut |expression, parent| match expression {
                AngularExpression::BindingPipe(pipe) if pipe.name == "async" => {
                    if matches!(pipe.exp, AngularExpression::PrefixNot(_)) {
                        let span = template.expression_span(pipe.source_span);
                        ctx.diagnostic_with_suggestion(
                            negated_value_for_async_diagnostic(span),
                            |fixer| {
                                fixer
                                    .delete_range(Span::sized(span.start, 1))
                                    .with_message("Use non-negated value")
                            },
                        );
                    }
                }
                AngularExpression::PrefixNot(not)
                    if !matches!(parent, Some(AngularExpression::PrefixNot(_)))
                        && is_async_pipe(&not.expression) =>
                {
                    let span = template.expression_span(not.source_span);
                    let negated = ctx.source_range(Span::new(span.start + 1, span.end));
                    let suggestions = ["false", "null", "undefined"].map(|value| {
                        fixer
                            .replace(span, format!("{negated} === {value}"))
                            .with_message(format!("Compare with `{value}`"))
                    });
                    ctx.diagnostic_with_suggestions(negated_async_diagnostic(span), suggestions);
                }
                _ => {}
            });
        }
    }
}

fn is_async_pipe(expression: &AngularExpression) -> bool {
    match expression {
        AngularExpression::BindingPipe(pipe) => pipe.name == "async",
        AngularExpression::ParenthesizedExpression(e) => {
            matches!(&e.expression, AngularExpression::BindingPipe(pipe) if pipe.name == "async")
        }
        _ => false,
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "{{ (foo | async) }}",
        "{{ !(foo | async | somethingElse) }}",
        "{{ (foo | async) == null }}",
        "{{ (foo | async) === false }}",
        "{{ !(foo | notAnAsyncPipe) }}",
        r#"<div [class.mx-4]="!!(foo | async)"></div>"#,
    ];

    let fail = vec![
        "{{ !(foo | async) }}",
        "{{ !(foo | somethingElse | async) }}",
        r#"<div *ngIf="!(a | async)"></div>"#,
        "{{ nullable ?? !(obsVar | async) }}",
        r#"<button [disabled]="!buttonDisabled$ | async">Click me!</button>"#,
        "@if (!(a$ | async)) { x }",
        "@defer (when !(loaded$ | async)) { <x-cmp/> }",
    ];

    let fix = vec![
        ("{{ !(foo | async) }}", "{{ (foo | async) === false }}"),
        (
            r#"<button [disabled]="!b$ | async"></button>"#,
            r#"<button [disabled]="b$ | async"></button>"#,
        ),
    ];

    Tester::new(NoNegatedAsync::NAME, NoNegatedAsync::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
