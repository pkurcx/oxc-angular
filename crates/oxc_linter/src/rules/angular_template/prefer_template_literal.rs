use std::{ffi::OsStr, path::Path};

use oxc_angular_compiler::ast::expression::{AngularExpression, BinaryOperator, LiteralValue};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_syntax::number::ToJsString;

use crate::{
    context::LintContext, frameworks::FrameworkOptions, rule::Rule, utils::for_each_expression,
};

fn prefer_template_literal_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Prefer using template literal instead of concatenating strings or expressions",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferTemplateLiteral;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that template literals are used instead of concatenating strings or expressions.
    ///
    /// Inline templates are not checked, since they are usually written in a template literal
    /// themselves, where backticks would have to be escaped.
    ///
    /// ### Why is this bad?
    ///
    /// Concatenation like `'Hello ' + name + '!'` is harder to read and easier to get wrong
    /// (missing spaces or quotes) than the template literal `` `Hello ${name}!` ``, which Angular
    /// templates support since v19.2.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// {{ 'prefix-' + value + '-suffix' }}
    /// <div [class]="'a' + 'b'"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// {{ `prefix-${value}-suffix` }}
    /// <div [class]="'ab'"></div>
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - The fix escapes `\` and `${` in the strings it joins.
    PreferTemplateLiteral,
    angular_template,
    style,
    fix,
    version = "next",
    short_description = "Ensure that template literals are used instead of concatenating strings or expressions.",
);

impl Rule for PreferTemplateLiteral {
    fn run_once(&self, ctx: &LintContext) {
        if ctx.frameworks_options() != FrameworkOptions::AngularTemplate
            || is_extracted_inline_template(ctx.file_path())
        {
            return;
        }
        for template in ctx.angular_templates() {
            for_each_expression(template.nodes, &mut |expression, parent| {
                if !is_concat(expression)
                    || parent.is_some_and(is_concat)
                    || !contains_string(expression)
                {
                    return;
                }
                let in_template_literal = matches!(
                    parent,
                    Some(
                        AngularExpression::TemplateLiteral(_)
                            | AngularExpression::TaggedTemplateLiteral(_)
                    )
                );
                let span = template.expression_span(expression.source_span());
                let mut parts = vec![];
                flatten(expression, &mut parts);
                ctx.diagnostic_with_fix(prefer_template_literal_diagnostic(span), |fixer| {
                    let source = ctx.source_text();
                    let text = |span: Span| source.get(span.start as usize..span.end as usize);
                    let all_literals = parts.iter().all(|part| matches!(part, Part::Literal(_)));
                    let quote = if in_template_literal || !all_literals {
                        '`'
                    } else {
                        text(span)
                            .and_then(|text| text.chars().find(|c| matches!(c, '\'' | '"' | '`')))
                            .unwrap_or('\'')
                    };
                    let mut replacement = String::new();
                    for part in &parts {
                        match part {
                            Part::Literal(value) => push_escaped(&mut replacement, value, quote),
                            Part::Expression(expression) => {
                                let Some(expression) =
                                    text(template.expression_span(expression.source_span()))
                                else {
                                    return fixer.noop();
                                };
                                replacement.push_str("${");
                                replacement.push_str(expression);
                                replacement.push('}');
                            }
                        }
                    }
                    if !in_template_literal {
                        return fixer.replace(span, format!("{quote}{replacement}{quote}"));
                    }
                    // Inline into the enclosing template literal, dropping the `${` and `}`
                    // around the concatenation.
                    let (Some(open), Some(close)) = (
                        source.get(..span.start as usize).and_then(|s| s.rfind("${")),
                        source.get(span.end as usize..).and_then(|s| s.find('}')),
                    ) else {
                        return fixer.noop();
                    };
                    #[expect(clippy::cast_possible_truncation)]
                    let range = Span::new(open as u32, span.end + close as u32 + 1);
                    fixer.replace(range, replacement)
                });
            });
        }
    }
}

enum Part<'e, 'a> {
    Literal(String),
    Expression(&'e AngularExpression<'a>),
}

fn is_concat(expression: &AngularExpression) -> bool {
    matches!(expression, AngularExpression::Binary(binary) if binary.operation == BinaryOperator::Add)
}

fn unwrap_parenthesized<'e, 'a>(
    expression: &'e AngularExpression<'a>,
) -> &'e AngularExpression<'a> {
    match expression {
        AngularExpression::ParenthesizedExpression(parenthesized) => &parenthesized.expression,
        _ => expression,
    }
}

/// Whether a `+` chain has a string or template literal operand anywhere.
fn contains_string(expression: &AngularExpression) -> bool {
    match unwrap_parenthesized(expression) {
        AngularExpression::LiteralPrimitive(literal) => {
            matches!(literal.value, LiteralValue::String(_))
        }
        AngularExpression::TemplateLiteral(_) => true,
        AngularExpression::Binary(binary) if binary.operation == BinaryOperator::Add => {
            contains_string(&binary.left) || contains_string(&binary.right)
        }
        _ => false,
    }
}

/// The operands of a `+` chain, left to right, with literal text inlined.
fn flatten<'e, 'a>(expression: &'e AngularExpression<'a>, parts: &mut Vec<Part<'e, 'a>>) {
    let expression = unwrap_parenthesized(expression);
    match expression {
        AngularExpression::Binary(binary)
            if binary.operation == BinaryOperator::Add && contains_string(expression) =>
        {
            flatten(&binary.left, parts);
            flatten(&binary.right, parts);
        }
        AngularExpression::TemplateLiteral(literal) => {
            for (i, element) in literal.elements.iter().enumerate() {
                if !element.text.is_empty() {
                    parts.push(Part::Literal(element.text.to_string()));
                }
                if let Some(expression) = literal.expressions.get(i) {
                    parts.push(Part::Expression(expression));
                }
            }
        }
        AngularExpression::LiteralPrimitive(literal) => {
            parts.push(Part::Literal(match &literal.value {
                LiteralValue::String(value) => value.to_string(),
                LiteralValue::Number(value) => value.to_js_string(),
                LiteralValue::Boolean(value) => value.to_string(),
                LiteralValue::Null => "null".to_string(),
                LiteralValue::Undefined => "undefined".to_string(),
            }));
        }
        _ => parts.push(Part::Expression(expression)),
    }
}

/// Appends `value` as the content of a string quoted with `quote`.
fn push_escaped(out: &mut String, value: &str, quote: char) {
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' || c == quote || (quote == '`' && c == '$' && chars.peek() == Some(&'{')) {
            out.push('\\');
        }
        out.push(c);
    }
}

/// Whether the file is an inline template that `@angular-eslint`'s processor extracted, which
/// upstream skips by name. ESLint names it `<file>/<index>_inline-template-…`, so the match is
/// unanchored, as upstream's regex.
fn is_extracted_inline_template(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .and_then(|name| name.split_once("inline-template-"))
        .map(|(_, name)| name)
        .and_then(|name| name.strip_suffix(".component.html"))
        .and_then(|name| name.rsplit_once('-'))
        .is_some_and(|(name, index)| {
            !name.is_empty()
                && !name.contains('\\')
                && !index.is_empty()
                && index.bytes().all(|b| b.is_ascii_digit())
        })
}

#[test]
fn test() {
    use std::path::PathBuf;

    use crate::tester::Tester;

    let pass = vec![
        ("{{ `prefix-${value}-suffix` }}", None, None, None),
        ("{{ 42 + 42 }}", None, None, None),
        ("{{ value + value2 }}", None, None, None),
        ("{{ value() + value2() }}", None, None, None),
        ("{{ 'simple-quote' | pipe }}", None, None, None),
        (r#"{{ "double-quote" }}""#, None, None, None),
        ("{{ `backquote` }}", None, None, None),
        ("@if (`prefix-${value}-suffix`) {}", None, None, None),
        ("@defer (when `prefix-${value}-suffix`) {}", None, None, None),
        ("@let letValue = `prefix-${value}-suffix`;", None, None, None),
        ("<h1>{{ `prefix-${value}-suffix` }}</h1>", None, None, None),
        (r#"<my-component class="prefix-{{value}}-suffix"></my-component>"#, None, None, None),
        (r#"<my-component [class]="`prefix-${value}-suffix`"></my-component>"#, None, None, None),
        (r#"<my-component *directive="`prefix-${value}-suffix` | pipe" />"#, None, None, None),
        (
            r#"<div [style.--row-height]="itemSize() + 'px'">Example</div>"#,
            None,
            None,
            Some(PathBuf::from("/components/widget/inline-template-widget-1.component.html")),
        ),
        (
            r#"<div [style.--row-height]="itemSize() + 'px'">Example</div>"#,
            None,
            None,
            Some(PathBuf::from(
                "/components/widget/widget.component.ts/0_inline-template-widget-1.component.html",
            )),
        ),
        // A parenthesized operand without a string is not a chain of its own.
        ("{{ (value + 1) - 2 }}", None, None, None),
        ("{{ a - 'b' }}", None, None, None),
    ];

    let fail = vec![
        ("{{\n'a'\n + \n'b'\n}}", None, None, None),
        (r#"{{ 'pre"fix-' + '-suf\'fix' }}"#, None, None, None),
        (r#"{{ "pre'fix-" + "-suf\"fix" }}"#, None, None, None),
        ("{{ `prefix-${value}-suffix` + `-prefix2-${value2}-suffix2` }}", None, None, None),
        (r#"{{ `prefix-${a}-${b + '-special\'"\`-char'}-${d}-suffix` }}"#, None, None, None),
        ("{{ `prefix-${a}-${b + `-inside-${c}`}-${d}-suffix` }}", None, None, None),
        ("{{ `prefix-${a}-${'b' + 'c'}-${d}-suffix` }}", None, None, None),
        (r#"{{ 'pre"fix-' + "-suf'fix" }}"#, None, None, None),
        (r#"{{ 'pre`fix-' + `'pre\`fix"-${value}-"suf\`fix'` }}"#, None, None, None),
        (r#"{{ "pre'fix-" + '-suf"fix' }}"#, None, None, None),
        (r#"{{ "pre`fix-" + `'pre\`fix"-${value}-"suf\`fix'` }}"#, None, None, None),
        (r#"<my-component [class]="'prefix-' + myClass | pipe"></my-component>"#, None, None, None),
        (r#"@if (value() + "-suffix" | pipe) {}"#, None, None, None),
        ("@defer (when value() + '-suffix' | pipe) {}", None, None, None),
        ("@let letValue = value() + '-suffix';", None, None, None),
        (
            "<ng-container *ngTemplateOutlet=\"selector;\n    context: {\n        name: 'test-' + item.id,\n        value: 42\n    }\n\"/>\n\n<div></div>",
            None,
            None,
            None,
        ),
        ("<div [ngStyle]=\"{\n    width: 10 + 'px'\n}\"></div>", None, None, None),
        ("<div [ngStyle]=\"\n     { width: 10 + 'px' }\n\"></div>", None, None, None),
        ("<div [class]=\"\n     'a' + 'b'\n\"></div>", None, None, None),
        ("{{ x.type + '' + $any(x).label + '' + $any(x).to }}", None, None, None),
        // A structural directive and `<ng-template>`.
        (r#"<div *ngIf="'a' + b" [title]="'c' + d"></div>"#, None, None, None),
        (r#"<ng-template [ngIf]="'a' + b"><p>x</p></ng-template>"#, None, None, None),
        // A parenthesized chain is reported on its own too, as upstream.
        ("{{ ('a' + b) + c }}", None, None, None),
    ];

    let fix = vec![
        ("{{\n'a'\n + \n'b'\n}}", "{{\n'ab'\n}}"),
        (r#"{{ 'pre"fix-' + '-suf\'fix' }}"#, r#"{{ 'pre"fix--suf\'fix' }}"#),
        (r#"{{ "pre'fix-" + "-suf\"fix" }}"#, r#"{{ "pre'fix--suf\"fix" }}"#),
        (
            "{{ `prefix-${value}-suffix` + `-prefix2-${value2}-suffix2` }}",
            "{{ `prefix-${value}-suffix-prefix2-${value2}-suffix2` }}",
        ),
        (
            r#"{{ `prefix-${a}-${b + '-special\'"\`-char'}-${d}-suffix` }}"#,
            r#"{{ `prefix-${a}-${b}-special'"\`-char-${d}-suffix` }}"#,
        ),
        (
            "{{ `prefix-${a}-${b + `-inside-${c}`}-${d}-suffix` }}",
            "{{ `prefix-${a}-${b}-inside-${c}-${d}-suffix` }}",
        ),
        ("{{ `prefix-${a}-${'b' + 'c'}-${d}-suffix` }}", "{{ `prefix-${a}-bc-${d}-suffix` }}"),
        (r#"{{ 'pre"fix-' + "-suf'fix" }}"#, r#"{{ 'pre"fix--suf\'fix' }}"#),
        (
            r#"{{ 'pre`fix-' + `'pre\`fix"-${value}-"suf\`fix'` }}"#,
            r#"{{ `pre\`fix-'pre\`fix"-${value}-"suf\`fix'` }}"#,
        ),
        (r#"{{ "pre'fix-" + '-suf"fix' }}"#, r#"{{ "pre'fix--suf\"fix" }}"#),
        (
            r#"{{ "pre`fix-" + `'pre\`fix"-${value}-"suf\`fix'` }}"#,
            r#"{{ `pre\`fix-'pre\`fix"-${value}-"suf\`fix'` }}"#,
        ),
        (
            r#"<my-component [class]="'prefix-' + myClass | pipe"></my-component>"#,
            r#"<my-component [class]="`prefix-${myClass}` | pipe"></my-component>"#,
        ),
        (r#"@if (value() + "-suffix" | pipe) {}"#, "@if (`${value()}-suffix` | pipe) {}"),
        (
            "@defer (when value() + '-suffix' | pipe) {}",
            "@defer (when `${value()}-suffix` | pipe) {}",
        ),
        ("@let letValue = value() + '-suffix';", "@let letValue = `${value()}-suffix`;"),
        // Left: single quote.
        ("{{ 'prefix-' + 42 }}", "{{ 'prefix-42' }}"),
        ("{{ 'prefix-' + null }}", "{{ 'prefix-null' }}"),
        ("{{ 'prefix-' + undefined }}", "{{ 'prefix-undefined' }}"),
        ("{{ 'prefix-' + true }}", "{{ 'prefix-true' }}"),
        ("{{ 'prefix-' + value }}", "{{ `prefix-${value}` }}"),
        ("{{ 'prefix-' + value() }}", "{{ `prefix-${value()}` }}"),
        ("{{ 'prefix-' + [42] }}", "{{ `prefix-${[42]}` }}"),
        (
            "{{ 'prefix-' + (condition ? 'true' : 'false') }}",
            "{{ `prefix-${condition ? 'true' : 'false'}` }}",
        ),
        ("{{ 'prefix-' + ('value' | pipe) }}", "{{ `prefix-${'value' | pipe}` }}"),
        // Left: double quote.
        (r#"{{ "prefix-" + 42 }}"#, r#"{{ "prefix-42" }}"#),
        (r#"{{ "prefix-" + null }}"#, r#"{{ "prefix-null" }}"#),
        (r#"{{ "prefix-" + undefined }}"#, r#"{{ "prefix-undefined" }}"#),
        (r#"{{ "prefix-" + true }}"#, r#"{{ "prefix-true" }}"#),
        (r#"{{ "prefix-" + value }}"#, "{{ `prefix-${value}` }}"),
        (r#"{{ "prefix-" + value() }}"#, "{{ `prefix-${value()}` }}"),
        (r#"{{ "prefix-" + [42] }}"#, "{{ `prefix-${[42]}` }}"),
        // Left: template.
        ("{{ `prefix-${value}-suffix` + 42 }}", "{{ `prefix-${value}-suffix42` }}"),
        ("{{ `prefix-${value}-suffix` + null }}", "{{ `prefix-${value}-suffixnull` }}"),
        ("{{ `prefix-${value}-suffix` + undefined }}", "{{ `prefix-${value}-suffixundefined` }}"),
        ("{{ `prefix-${value}-suffix` + false }}", "{{ `prefix-${value}-suffixfalse` }}"),
        ("{{ `prefix-${value}-suffix` + value2 }}", "{{ `prefix-${value}-suffix${value2}` }}"),
        ("{{ `prefix-${value}-suffix` + value2() }}", "{{ `prefix-${value}-suffix${value2()}` }}"),
        ("{{ `prefix-${value}-suffix` + [42] }}", "{{ `prefix-${value}-suffix${[42]}` }}"),
        (
            "{{ `prefix-${value}-suffix` + (condition ? 'true' : 'false') }}",
            "{{ `prefix-${value}-suffix${condition ? 'true' : 'false'}` }}",
        ),
        (
            "{{ `prefix-${value}-suffix` + ('value' | pipe) }}",
            "{{ `prefix-${value}-suffix${'value' | pipe}` }}",
        ),
        // Right: single quote.
        ("{{ 42 + '-suffix' }}", "{{ '42-suffix' }}"),
        ("{{ null + '-suffix' }}", "{{ 'null-suffix' }}"),
        ("{{ undefined + '-suffix' }}", "{{ 'undefined-suffix' }}"),
        ("{{ true + '-suffix' }}", "{{ 'true-suffix' }}"),
        ("{{ value + '-suffix' }}", "{{ `${value}-suffix` }}"),
        ("{{ value() + '-suffix' }}", "{{ `${value()}-suffix` }}"),
        ("{{ [42] + '-suffix' }}", "{{ `${[42]}-suffix` }}"),
        (
            r#"{{ `'pre\`fix"-${value}-"suf\`fix'` + '-suf`fix' }}"#,
            r#"{{ `'pre\`fix"-${value}-"suf\`fix'-suf\`fix` }}"#,
        ),
        (
            "{{ (condition ? 'true' : 'false') + '-suffix' }}",
            "{{ `${condition ? 'true' : 'false'}-suffix` }}",
        ),
        ("{{ (value + 1) + '-suffix' }}", "{{ `${value + 1}-suffix` }}"),
        ("{{ ('value' | pipe) + '-suffix' }}", "{{ `${'value' | pipe}-suffix` }}"),
        // Right: double quote.
        (r#"{{ 42 + "-suffix" }}"#, r#"{{ "42-suffix" }}"#),
        (r#"{{ null + "-suffix" }}"#, r#"{{ "null-suffix" }}"#),
        (r#"{{ undefined + "-suffix" }}"#, r#"{{ "undefined-suffix" }}"#),
        (r#"{{ true + "-suffix" }}"#, r#"{{ "true-suffix" }}"#),
        (r#"{{ value + "-suffix" }}"#, "{{ `${value}-suffix` }}"),
        (r#"{{ value() + "-suffix" }}"#, "{{ `${value()}-suffix` }}"),
        (r#"{{ [42] + "-suffix" }}"#, "{{ `${[42]}-suffix` }}"),
        (
            r#"{{ `'pre\`fix"-${value}-"suf\`fix'` + "-suf`fix" }}"#,
            r#"{{ `'pre\`fix"-${value}-"suf\`fix'-suf\`fix` }}"#,
        ),
        (
            r#"{{ (condition ? 'true' : 'false') + "-suffix" }}"#,
            "{{ `${condition ? 'true' : 'false'}-suffix` }}",
        ),
        (r#"{{ ('value' | pipe) + "-suffix" }}"#, "{{ `${'value' | pipe}-suffix` }}"),
        // Right: template.
        ("{{ 42 + `prefix-${value}-suffix` }}", "{{ `42prefix-${value}-suffix` }}"),
        ("{{ null + `prefix-${value}-suffix` }}", "{{ `nullprefix-${value}-suffix` }}"),
        ("{{ undefined + `prefix-${value}-suffix` }}", "{{ `undefinedprefix-${value}-suffix` }}"),
        ("{{ false + `prefix-${value}-suffix` }}", "{{ `falseprefix-${value}-suffix` }}"),
        ("{{ value2 + `prefix-${value}-suffix` }}", "{{ `${value2}prefix-${value}-suffix` }}"),
        ("{{ value2() + `prefix-${value}-suffix` }}", "{{ `${value2()}prefix-${value}-suffix` }}"),
        ("{{ [42] + `prefix-${value}-suffix` }}", "{{ `${[42]}prefix-${value}-suffix` }}"),
        (
            "{{ (condition ? 'true' : 'false') + `prefix-${value}-suffix` }}",
            "{{ `${condition ? 'true' : 'false'}prefix-${value}-suffix` }}",
        ),
        (
            "{{ ('value' | pipe) + `prefix-${value}-suffix` }}",
            "{{ `${'value' | pipe}prefix-${value}-suffix` }}",
        ),
        // Bound attributes and object literals across lines.
        (
            "<ng-container\n    *ngTemplateOutlet=\"selector;\n        context: {\n            name: 'test-' + item.id,\n            value: 42\n        }\n    \"\n/>\n\n<div></div>",
            "<ng-container\n    *ngTemplateOutlet=\"selector;\n        context: {\n            name: `test-${item.id}`,\n            value: 42\n        }\n    \"\n/>\n\n<div></div>",
        ),
        (
            "<div [ngStyle]=\"{\n    width: 10 + 'px'\n}\"></div>",
            "<div [ngStyle]=\"{\n    width: '10px'\n}\"></div>",
        ),
        (
            "<div [ngStyle]=\"\n     { width: 10 + 'px' }\n\"></div>",
            "<div [ngStyle]=\"\n     { width: '10px' }\n\"></div>",
        ),
        (
            "<ng-container\n    *ngTemplateOutlet=\"\n      selector;\n        context: {\n            name: 'test-' + item.id,\n            value: 42\n        }\n    \"\n/>\n\n<div></div>",
            "<ng-container\n    *ngTemplateOutlet=\"\n      selector;\n        context: {\n            name: `test-${item.id}`,\n            value: 42\n        }\n    \"\n/>\n\n<div></div>",
        ),
        ("<div [class]=\"\n     'a' + 'b'\n\"></div>", "<div [class]=\"\n     'ab'\n\"></div>"),
        // Chains.
        ("{{ 'a' + 'b' + 'c' }}", "{{ 'abc' }}"),
        ("{{ x.type + '' + y }}", "{{ `${x.type}${y}` }}"),
        (
            "{{ x.type + '' + $any(x).label + '' + $any(x).to }}",
            "{{ `${x.type}${$any(x).label}${$any(x).to}` }}",
        ),
        ("{{ 'a' + 'b' + 'c' + 'd' }}", "{{ 'abcd' }}"),
        (
            "{{ 'prefix-' + value + '-middle-' + value2 + '-suffix' }}",
            "{{ `prefix-${value}-middle-${value2}-suffix` }}",
        ),
        ("{{ getValue() + '-' + getOther() + '-end' }}", "{{ `${getValue()}-${getOther()}-end` }}"),
        (r#"<div *ngIf="'a' + b"></div>"#, r#"<div *ngIf="`a${b}`"></div>"#),
        (
            r#"<ng-template [ngIf]="'a' + b"><p>x</p></ng-template>"#,
            r#"<ng-template [ngIf]="`a${b}`"><p>x</p></ng-template>"#,
        ),
        ("{{ tag`a${b + 'c'}` }}", "{{ tag`a${b}c` }}"),
        // Backslashes and `${` in literal text stay literal.
        (r"{{ 'a\\b' + c }}", r"{{ `a\\b${c}` }}"),
        ("{{ '${a}' + b }}", r"{{ `\${a}${b}` }}"),
    ];

    Tester::new(PreferTemplateLiteral::NAME, PreferTemplateLiteral::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();

    // Inline templates are not checked.
    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '{{ a + \"b\" }}' }) class A {}",
    ];
    Tester::new(
        PreferTemplateLiteral::NAME,
        PreferTemplateLiteral::PLUGIN,
        inline_pass,
        Vec::<&str>::new(),
    )
    .change_rule_path_extension("ts")
    .intentionally_allow_no_fix_tests()
    .test();
}
