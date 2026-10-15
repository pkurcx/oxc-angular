use std::cmp::Ordering;

use oxc_ast::{
    AstKind,
    ast::{Argument, Expression, ObjectPropertyKind},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{AstNode, context::LintContext, rule::Rule, rules::eslint::sort_keys::natural_compare};

fn sorted_component_imports_diagnostic(span: Span, member: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Member '{member}' of the imports array should be sorted alphabetically."
    ))
    .with_help("Sort the `imports` array of `@Component`, with spread elements last.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct SortedComponentImports;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces that the `imports` array of an Angular `@Component` decorator is sorted
    /// alphabetically by source text, case-insensitively and with digit runs compared as numbers.
    /// Spread elements go last.
    ///
    /// ### Why is this bad?
    ///
    /// An unsorted `imports` array is harder to scan and produces noisy diffs and merge conflicts.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({ imports: [RouterLink, MatButton, ...SHARED, AsyncPipe] })
    /// class AppComponent {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({ imports: [AsyncPipe, MatButton, RouterLink, ...SHARED] })
    /// class AppComponent {}
    /// ```
    SortedComponentImports,
    angular,
    style,
    fix,
    version = "next",
    short_description = "Enforce an alphabetically sorted `imports` array in `@Component`.",
);

impl Rule for SortedComponentImports {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Decorator(decorator) = node.kind() else { return };
        let Expression::CallExpression(call) = &decorator.expression else { return };
        if !call.callee.is_specific_id("Component") {
            return;
        }
        let Some(Argument::ObjectExpression(metadata)) = call.arguments.first() else { return };
        let Some(Expression::ArrayExpression(array)) =
            metadata.properties.iter().find_map(|prop| match prop {
                ObjectPropertyKind::ObjectProperty(prop)
                    if prop.key.is_specific_static_name("imports") =>
                {
                    Some(&prop.value)
                }
                _ => None,
            })
        else {
            return;
        };

        let elements: Vec<_> = array
            .elements
            .iter()
            .filter(|el| !el.is_elision())
            .map(|el| (ctx.source_range(el.span()), el))
            .collect();
        if elements.len() <= 1 {
            return;
        }

        let mut sorted = elements.clone();
        // Stable, so equal keys keep their order.
        sorted.sort_by(|(a_text, a), (b_text, b)| {
            a.is_spread().cmp(&b.is_spread()).then_with(|| compare(a_text, b_text))
        });

        let Some(&(member, element)) =
            elements.iter().zip(&sorted).find(|((a, _), (b, _))| a != b).map(|(el, _)| el)
        else {
            return;
        };

        ctx.diagnostic_with_fix(
            sorted_component_imports_diagnostic(element.span(), member),
            |fixer| {
                // Rebuilding the array would drop its comments.
                if ctx.has_comments_between(array.span) {
                    return fixer.noop();
                }
                let last_end = elements[elements.len() - 1].1.span().end;
                let has_trailing_comma = ctx
                    .source_range(Span::new(last_end, array.span.end - 1))
                    .trim_start()
                    .starts_with(',');
                let texts = sorted.iter().map(|(text, _)| *text);

                let array_text = ctx.source_range(array.span);
                if !array_text.contains('\n') {
                    let inner = texts.collect::<Vec<_>>().join(", ");
                    let comma = if has_trailing_comma { "," } else { "" };
                    return fixer.replace(array.span, format!("[{inner}{comma}]"));
                }

                let source = ctx.source_text();
                let element_indent = line_indent(source, elements[0].1.span().start);
                let count = sorted.len();
                let mut body = String::from("[\n");
                for (i, text) in texts.enumerate() {
                    body.push_str(element_indent);
                    body.push_str(text);
                    if i + 1 < count || has_trailing_comma {
                        body.push(',');
                    }
                    body.push('\n');
                }
                body.push_str(line_indent(source, array.span.start));
                body.push(']');
                fixer.replace(array.span, body)
            },
        );
    }
}

// Approximates `Intl.Collator('en', { numeric: true, sensitivity: 'case' })`: lowercase
// sorts before uppercase only when the strings are otherwise equal.
fn compare(a: &str, b: &str) -> Ordering {
    natural_compare(a, b, false).then_with(|| b.cmp(a))
}

fn line_indent(source: &str, offset: u32) -> &str {
    let line_start = source[..offset as usize].rfind('\n').map_or(0, |i| i + 1);
    let line = &source[line_start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "@Component({ imports: [A, B, C] }) class X {}",
        "@Component({\n  imports: [\n    A,\n    B,\n    C,\n  ]\n}) class X {}",
        "@Component({ imports: [A, B, ...extra] }) class X {}",
        "@Component({ imports: [A, RouterModule.forRoot({}), SomeModule.init(),] }) class X {}",
        "@Component({ imports: [Feature2Module, Feature10Module] }) class X {}",
        "@Component({ imports: [aModule, AModule, BModule] }) class X {}",
        "@Component({ imports: [B] }) class X {}",
        "@Component({ 'imports': [A, B] }) class X {}",
        "@Directive({ imports: [B, A] }) class X {}",
        "@Component({ providers: [B, A] }) class X {}",
        "@Component({ imports }) class X {}",
    ];

    let fail = vec![
        "@Component({ imports: [B, A, C] }) class X {}",
        "@Component({ imports: [Z, ...extra, A] }) class X {}",
        "@Component({\n  imports: [\n    C,\n    A,\n    B,\n  ]\n}) class X {}",
        "@Component({ imports: [RouterModule.forRoot({}), A, Z] }) class X {}",
        "@Component({ imports: [AlphaModule, ...provideBeta(), GammaModule] }) class X {}",
        "@Component({ imports: [Z, ...provideFoo(), A] }) class X {}",
        "@Component({ imports: [Feature10Module, Feature2Module] }) class X {}",
        "@Component({ imports: [AModule, aModule] }) class X {}",
        "@Component({ 'imports': [B, A] }) class X {}",
        "@Component({ imports: [B, /* keep */ A] }) class X {}",
    ];

    let fix = vec![
        (
            "@Component({ imports: [B, A, C] }) class X {}",
            "@Component({ imports: [A, B, C] }) class X {}",
        ),
        (
            "@Component({ imports: [Z, ...extra, A] }) class X {}",
            "@Component({ imports: [A, Z, ...extra] }) class X {}",
        ),
        (
            "@Component({\n  imports: [\n    C,\n    A,\n    B,\n  ]\n}) class X {}",
            "@Component({\n  imports: [\n    A,\n    B,\n    C,\n  ]\n}) class X {}",
        ),
        (
            "@Component({\n  imports: [\n    C,\n    A,\n    B\n  ]\n}) class X {}",
            "@Component({\n  imports: [\n    A,\n    B,\n    C\n  ]\n}) class X {}",
        ),
        (
            "@Component({ imports: [RouterModule.forRoot({}), A, Z] }) class X {}",
            "@Component({ imports: [A, RouterModule.forRoot({}), Z] }) class X {}",
        ),
        (
            "@Component({ imports: [AlphaModule, ...provideBeta(), GammaModule] }) class X {}",
            "@Component({ imports: [AlphaModule, GammaModule, ...provideBeta()] }) class X {}",
        ),
        (
            "@Component({ imports: [Z, ...provideFoo(), A] }) class X {}",
            "@Component({ imports: [A, Z, ...provideFoo()] }) class X {}",
        ),
        (
            "@Component({ imports: [Feature10Module, Feature2Module] }) class X {}",
            "@Component({ imports: [Feature2Module, Feature10Module] }) class X {}",
        ),
        (
            "@Component({ imports: [B, A,] }) class X {}",
            "@Component({ imports: [A, B,] }) class X {}",
        ),
        (
            "@Component({ imports: [B, /* keep */ A] }) class X {}",
            "@Component({ imports: [B, /* keep */ A] }) class X {}",
        ),
    ];

    Tester::new(SortedComponentImports::NAME, SortedComponentImports::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
