use oxc_angular_compiler::ast::{
    expression::{AngularExpression, LiteralMapKey, LiteralValue},
    r3::{R3IfBlock, R3Node},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{context::LintContext, fixer::Fix, rule::Rule, utils::AngularTemplate};

fn prefer_at_else_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Prefer using `@else` instead of a second `@if` clause.").with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferAtElse;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers `@else` over a second `@if` that tests the opposite condition of the `@if` right
    /// before it.
    ///
    /// ### Why is this bad?
    ///
    /// `@else` shows at a glance that exactly one of the two branches renders. Two `@if` blocks
    /// make the reader check that the conditions are opposites, and they can drift apart.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// @if (isLoggedIn) { Welcome }
    /// @if (!isLoggedIn) { Please log in }
    /// @if (items.length > 0) { Items } @if (items.length === 0) { None }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// @if (isLoggedIn) { Welcome } @else { Please log in }
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - When two fixes in one chain would together produce an invalid template, only the first is
    ///   applied; the second is applied on the next run.
    PreferAtElse,
    angular_template,
    style,
    conditional_fix,
    version = "next",
    short_description = "Prefer using `@else` instead of a second `@if` with the opposite condition to reduce code and make it easier to read.",
);

impl Rule for PreferAtElse {
    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            // The `@if` whose fix turns it into an `@else`.
            let mut fixed = None;
            walk_siblings(template.nodes, &mut |node, previous| {
                let (Some(current), Some(previous)) = (if_info(node), previous.and_then(if_info))
                else {
                    return;
                };
                if !can_combine(&previous, &current) {
                    return;
                }
                let span = template.span(current.node.name_span);
                // An `@else` cannot follow the `@else` the previous fix creates.
                let fixes = (fixed != Some(previous.node.name_span))
                    .then(|| fix(ctx, template, &previous, &current))
                    .flatten();
                fixed = fixes.is_some().then_some(current.node.name_span);
                match fixes {
                    Some(fixes) => ctx.diagnostic_with_fix(prefer_at_else_diagnostic(span), |f| {
                        let mut fix = f.for_multifix().new_fix_with_capacity(fixes.len());
                        for f in fixes {
                            fix.push(f);
                        }
                        fix.with_message("Replace the `@if` with `@else`")
                    }),
                    None => ctx.diagnostic(prefer_at_else_diagnostic(span)),
                }
            });
        }
    }
}

/// Calls `f` on every node with the sibling right before it. Comments, and whitespace after a
/// control flow block, are not template nodes, so they do not separate siblings.
pub(super) fn walk_siblings<'n, 'a>(
    nodes: &'n [R3Node<'a>],
    f: &mut dyn FnMut(&'n R3Node<'a>, Option<&'n R3Node<'a>>),
) {
    let mut previous = None;
    for node in nodes {
        f(node, previous);
        match node {
            R3Node::Element(n) => walk_siblings(&n.children, f),
            R3Node::Template(n) => walk_siblings(&n.children, f),
            R3Node::Content(n) => walk_siblings(&n.children, f),
            R3Node::Component(n) => walk_siblings(&n.children, f),
            R3Node::IfBlock(n) => {
                for branch in &n.branches {
                    walk_siblings(&branch.children, f);
                }
            }
            R3Node::IfBlockBranch(n) => walk_siblings(&n.children, f),
            R3Node::ForLoopBlock(n) => {
                walk_siblings(&n.children, f);
                if let Some(empty) = &n.empty {
                    walk_siblings(&empty.children, f);
                }
            }
            R3Node::ForLoopBlockEmpty(n) => walk_siblings(&n.children, f),
            R3Node::SwitchBlock(n) => {
                for group in &n.groups {
                    walk_siblings(&group.children, f);
                }
            }
            R3Node::SwitchBlockCaseGroup(n) => walk_siblings(&n.children, f),
            R3Node::DeferredBlock(n) => {
                walk_siblings(&n.children, f);
                if let Some(block) = &n.placeholder {
                    walk_siblings(&block.children, f);
                }
                if let Some(block) = &n.loading {
                    walk_siblings(&block.children, f);
                }
                if let Some(block) = &n.error {
                    walk_siblings(&block.children, f);
                }
            }
            R3Node::DeferredBlockPlaceholder(n) => walk_siblings(&n.children, f),
            R3Node::DeferredBlockLoading(n) => walk_siblings(&n.children, f),
            R3Node::DeferredBlockError(n) => walk_siblings(&n.children, f),
            _ => {}
        }
        previous = Some(node);
    }
}

/// An `@if` with an optional `@else`, and its condition split as `lhs operator rhs`.
struct IfInfo<'n, 'a> {
    node: &'n R3IfBlock<'a>,
    lhs: &'n AngularExpression<'a>,
    rhs: Option<&'n AngularExpression<'a>>,
    operator: &'static str,
}

fn if_info<'n, 'a>(node: &'n R3Node<'a>) -> Option<IfInfo<'n, 'a>> {
    let R3Node::IfBlock(block) = node else { return None };
    // An `@else if` branch cannot be combined with another `@if`.
    let [first, rest @ ..] = block.branches.as_slice() else { return None };
    if rest.len() > 1 || rest.first().is_some_and(|branch| branch.expression.is_some()) {
        return None;
    }
    let (lhs, rhs, operator) = match first.expression.as_ref()? {
        AngularExpression::Binary(e) => (&e.left, Some(&e.right), e.operation.as_str()),
        AngularExpression::PrefixNot(e) => (&e.expression, None, "!"),
        // `-a` is a binary expression in Angular, whose operator has no opposite.
        AngularExpression::Unary(_) => return None,
        e => (e, None, ""),
    };
    Some(IfInfo { node: block, lhs, rhs, operator })
}

fn opposite(operator: &str) -> Option<&'static str> {
    Some(match operator {
        "" => "!",
        "!" => "",
        "<" => ">=",
        ">" => "<=",
        "<=" => ">",
        ">=" => "<",
        "==" => "!=",
        "!=" => "==",
        "===" => "!==",
        "!==" => "===",
        _ => return None,
    })
}

fn can_combine(previous: &IfInfo, current: &IfInfo) -> bool {
    if opposite(previous.operator) == Some(current.operator)
        && are_equivalent(previous.lhs, current.lhs)
        && match (previous.rhs, current.rhs) {
            (None, None) => true,
            (Some(a), Some(b)) => are_equivalent(a, b),
            _ => false,
        }
    {
        return true;
    }
    // A length cannot be negative, so "is empty" and "is not empty" are opposites.
    [
        (empty_length_read(previous), non_empty_length_read(current)),
        (non_empty_length_read(previous), empty_length_read(current)),
    ]
    .into_iter()
    .any(|pair| matches!(pair, (Some(a), Some(b)) if are_equivalent(a, b)))
}

/// The `x.length` of `x.length === 0` or `0 == x.length`.
fn empty_length_read<'n, 'a>(info: &IfInfo<'n, 'a>) -> Option<&'n AngularExpression<'a>> {
    let rhs = info.rhs?;
    if !matches!(info.operator, "==" | "===") {
        return None;
    }
    if is_length_read(info.lhs) && is_zero(rhs) {
        Some(info.lhs)
    } else if is_zero(info.lhs) && is_length_read(rhs) {
        Some(rhs)
    } else {
        None
    }
}

/// The `x.length` of `x.length > 0` or `0 < x.length`.
fn non_empty_length_read<'n, 'a>(info: &IfInfo<'n, 'a>) -> Option<&'n AngularExpression<'a>> {
    let rhs = info.rhs?;
    if is_length_read(info.lhs) && info.operator == ">" && is_zero(rhs) {
        Some(info.lhs)
    } else if is_zero(info.lhs) && info.operator == "<" && is_length_read(rhs) {
        Some(rhs)
    } else {
        None
    }
}

pub(super) fn is_length_read(expression: &AngularExpression) -> bool {
    matches!(expression, AngularExpression::PropertyRead(read) if read.name.as_str() == "length")
}

#[expect(clippy::float_cmp, reason = "`===` of JS numbers")]
pub(super) fn is_number(expression: &AngularExpression, value: f64) -> bool {
    matches!(expression, AngularExpression::LiteralPrimitive(literal)
        if matches!(literal.value, LiteralValue::Number(n) if n == value))
}

pub(super) fn is_zero(expression: &AngularExpression) -> bool {
    is_number(expression, 0.0)
}

/// Whether two expressions are the same, as `@angular-eslint`'s `areEquivalentASTs` compares
/// them: kinds it does not handle, such as parenthesized expressions, are never equivalent.
#[expect(clippy::float_cmp, reason = "`===` of JS numbers")]
pub(super) fn are_equivalent(a: &AngularExpression, b: &AngularExpression) -> bool {
    use AngularExpression as E;
    let all =
        |a: &[E], b: &[E]| a.len() == b.len() && a.iter().zip(b).all(|(a, b)| are_equivalent(a, b));
    match (a, b) {
        (E::ImplicitReceiver(_) | E::ThisReceiver(_), b) => {
            matches!(b, E::ImplicitReceiver(_) | E::ThisReceiver(_))
        }
        (E::PropertyRead(a), E::PropertyRead(b)) => {
            a.name == b.name && are_equivalent(&a.receiver, &b.receiver)
        }
        (E::SafePropertyRead(a), E::SafePropertyRead(b)) => {
            a.name == b.name && are_equivalent(&a.receiver, &b.receiver)
        }
        (E::Call(a), E::Call(b)) => {
            all(&a.args, &b.args) && are_equivalent(&a.receiver, &b.receiver)
        }
        (E::SafeCall(a), E::SafeCall(b)) => {
            all(&a.args, &b.args) && are_equivalent(&a.receiver, &b.receiver)
        }
        (E::KeyedRead(a), E::KeyedRead(b)) => {
            are_equivalent(&a.key, &b.key) && are_equivalent(&a.receiver, &b.receiver)
        }
        (E::SafeKeyedRead(a), E::SafeKeyedRead(b)) => {
            are_equivalent(&a.key, &b.key) && are_equivalent(&a.receiver, &b.receiver)
        }
        (E::NonNullAssert(a), E::NonNullAssert(b)) => are_equivalent(&a.expression, &b.expression),
        (E::PrefixNot(a), E::PrefixNot(b)) => are_equivalent(&a.expression, &b.expression),
        (E::Unary(a), E::Unary(b)) => a.operator == b.operator && are_equivalent(&a.expr, &b.expr),
        (E::Binary(a), E::Binary(b)) => {
            a.operation == b.operation
                && are_equivalent(&a.left, &b.left)
                && are_equivalent(&a.right, &b.right)
        }
        (E::Conditional(a), E::Conditional(b)) => {
            are_equivalent(&a.condition, &b.condition)
                && are_equivalent(&a.true_exp, &b.true_exp)
                && are_equivalent(&a.false_exp, &b.false_exp)
        }
        (E::LiteralPrimitive(a), E::LiteralPrimitive(b)) => match (&a.value, &b.value) {
            (LiteralValue::String(a), LiteralValue::String(b)) => a == b,
            (LiteralValue::Number(a), LiteralValue::Number(b)) => a == b,
            (LiteralValue::Boolean(a), LiteralValue::Boolean(b)) => a == b,
            (LiteralValue::Null, LiteralValue::Null)
            | (LiteralValue::Undefined, LiteralValue::Undefined) => true,
            _ => false,
        },
        (E::LiteralArray(a), E::LiteralArray(b)) => all(&a.expressions, &b.expressions),
        (E::LiteralMap(a), E::LiteralMap(b)) => {
            a.keys.len() == b.keys.len()
                && a.keys.iter().zip(&b.keys).all(|keys| match keys {
                    (LiteralMapKey::Spread(_), LiteralMapKey::Spread(_)) => true,
                    (LiteralMapKey::Property(a), LiteralMapKey::Property(b)) => a.key == b.key,
                    _ => false,
                })
                && all(&a.values, &b.values)
        }
        (E::BindingPipe(a), E::BindingPipe(b)) => {
            a.name == b.name && are_equivalent(&a.exp, &b.exp) && all(&a.args, &b.args)
        }
        (E::Interpolation(a), E::Interpolation(b)) => {
            a.strings.iter().eq(b.strings.iter()) && all(&a.expressions, &b.expressions)
        }
        (E::Chain(a), E::Chain(b)) => all(&a.expressions, &b.expressions),
        (E::TypeofExpression(a), E::TypeofExpression(b)) => {
            are_equivalent(&a.expression, &b.expression)
        }
        _ => false,
    }
}

/// Text of the linted source between two template offsets.
pub(super) fn template_text<'a>(
    ctx: &LintContext<'a>,
    template: &AngularTemplate,
    start: u32,
    end: u32,
) -> Option<&'a str> {
    let span = template.span(Span::new(start, end));
    ctx.source_text().get(span.start as usize..span.end as usize)
}

/// Content projection through control flow only works with one root node per branch, so
/// merging several roots into a branch must not be fixed automatically.
fn would_prevent_content_projection<'n, 'a: 'n>(
    ctx: &LintContext,
    template: &AngularTemplate,
    nodes: impl Iterator<Item = &'n R3Node<'a>>,
) -> bool {
    let mut roots = 0;
    let mut projectable = false;
    for node in nodes {
        if let R3Node::Text(text) = node
            && template_text(ctx, template, text.source_span.start, text.source_span.end)
                .is_some_and(|text| text.trim().is_empty())
        {
            continue;
        }
        roots += 1;
        projectable |= matches!(node, R3Node::Element(_) | R3Node::Template(_));
    }
    roots > 1 && projectable
}

fn fix(
    ctx: &LintContext,
    template: &AngularTemplate,
    previous: &IfInfo,
    current: &IfInfo,
) -> Option<Vec<Fix>> {
    let previous_if = &previous.node.branches[0];
    let current_if = &current.node.branches[0];
    let previous_else = previous.node.branches.get(1);
    let current_else = current.node.branches.get(1);
    // The alias would not exist in the `@else` branch of the previous `@if`.
    if current_if.expression_alias.is_some() {
        return None;
    }
    if previous_else.is_some_and(|previous_else| {
        would_prevent_content_projection(
            ctx,
            template,
            previous_else.children.iter().chain(&current_if.children),
        )
    }) || current_else.is_some_and(|current_else| {
        would_prevent_content_projection(
            ctx,
            template,
            previous_if.children.iter().chain(&current_else.children),
        )
    }) {
        return None;
    }

    let span = |start: u32, end: u32| template.span(Span::new(start, end));
    let mut fixes = vec![];
    if let Some(previous_else) = previous_else {
        // Both the previous `@else` and the current `@if` render, so the current `@if` content
        // moves to the end of the previous `@else`.
        let content = template_text(
            ctx,
            template,
            current_if.start_source_span.end,
            current_if.source_span.end.checked_sub(1)?,
        )?;
        let at = previous_else.source_span.end.checked_sub(1)?;
        fixes.push(Fix::new(content.to_string(), span(at, at)));
        fixes.push(Fix::delete(template.span(current_if.source_span)));
    } else {
        fixes.push(Fix::new(
            "@else {",
            span(current_if.source_span.start, current_if.start_source_span.end),
        ));
    }
    if let (Some(current_else), Some(current_if_end)) = (current_else, current_if.end_source_span) {
        // The current `@else` renders together with the previous `@if`.
        let content = template_text(
            ctx,
            template,
            current_else.start_source_span.end,
            current_else.source_span.end.checked_sub(1)?,
        )?;
        let at = previous_if.source_span.end.checked_sub(1)?;
        fixes.push(Fix::new(content.to_string(), span(at, at)));
        fixes.push(Fix::delete(span(current_if_end.end, current_else.source_span.end)));
    }
    Some(fixes)
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "@if (a) {}",
        "
            @if (a) {
            } @else {
            }
          ",
        "
            @if (a) {}
            @if (b) {}
          ",
        "
            @if (a) {}
            @if (!b) {}
          ",
        "
            @if (a.b) {}
            @if (!c.b) {}
          ",
        "
            @if (a === true) {}
            @if (a === false) {}
          ",
        "
            @if (a === b) {}
            @if (a != b) {}
          ",
        "
            @if (a > 0) {}
            @if (a < 0) {}
          ",
        "
            @if (a > 1) {}
            @if (a <= 0) {}
          ",
        "
            @if (a >= b) {}
            @if (a <= b) {}
          ",
        "
            @if (a.length > 1) {}
            @if (a.length <= 0) {}
          ",
        "
            @if (a) {}
            text
            @if (!a) {}
          ",
        "
            @if (a) {}
            <element/>
            @if (!a) {}
          ",
        "
            @if (a) {}
            @if (b) {}
            @if (!a) {}
          ",
        "
            @if (a) {}
            @else if (b) {}
            @if (!a) {}
          ",
        // Lengths of different collections are not opposites.
        "@if (a.length === 0) {} @if (b.length > 0) {}",
        "@if (-a) {} @if (!-a) {}",
        "@if ((a)) {} @if (!(a)) {}",
        "<div *ngIf=\"x\"></div><div *ngIf=\"!x\"></div>",
        "<div>@if (a) {}</div>@if (!a) {}",
        "<ng-template>@if (a) {}</ng-template>@if (!a) {}",
    ];

    let pairs = [
        ("a", "!a"),
        ("a > b", "a <= b"),
        ("a >= b", "a < b"),
        ("a === b", "a !== b"),
        ("a == b", "a != b"),
        ("a.length > 0", "a.length === 0"),
        ("0 < a.length", "a.length === 0"),
        ("0 < a.length", "0 === a.length"),
        ("a.length > 0", "0 === a.length"),
        ("a.length > 0", "a.length == 0"),
    ];

    let mut fail: Vec<String> = pairs
        .iter()
        .flat_map(|(a, b)| [(a, b), (b, a)])
        .map(|(first, second)| format!("@if ({first}) {{}}\n@if ({second}) {{}}"))
        .collect();
    fail.extend(
        [
            "
          @if (a) {
            1
          } @else {
            2
          }
          @if (!a) {
            3
          }
        ",
            "
          @if (a) {
            1
          }
          @if (!a) {
            2
          } @else {
            3
          }
        ",
            "
          @if (a) {
            1
          } @else {
            2
          }
          @if (!a) {
            3
          } @else {
            4
          }
        ",
            "
          <my-component>
            @if (a) {
              <button>1</button>
            } @else {
              <button>2</button>
            }
            @if (!a) {
              <button>3</button>
            }
          </my-component>
        ",
            "
          <my-component>
            @if (a) {
              <button>1</button>
            }
            @if (!a) {
              <button>2</button>
            } @else {
              <button>3</button>
            }
          </my-component>
        ",
            "
          @if (a) {
            <button>1</button>
          }
          @if (!a) {
            <button>2</button>
          }
          @if (!a) {
            <button>3</button>
          }
        ",
            "
          @if (a) {}

          @if (!a) {}
        ",
            "
          @if (a) {}
          &nbsp;
          @if (!a) {}
        ",
            "
          @if (a) {}
          <!-- comment -->
          @if (!a) {}
        ",
            "
          @if (a; as alias) {}
          @if (!a) {}
        ",
            "
          @if (a) {}
          @if (!a; as alias) {}
        ",
            r#"<div *ngIf="x">@if (a) {} @if (!a) {}</div>"#,
            "<ng-template>@if (a) {} @if (!a) {}</ng-template>",
            "@for (x of xs; track x) { @if (a) {} @if (!a) {} } @empty { @if (b) {} @if (!b) {} }",
            "@if (a) {} @if (!a) {} @if (a) {}",
        ]
        .map(String::from),
    );

    let mut fix: Vec<(String, String)> = pairs
        .iter()
        .flat_map(|(a, b)| [(a, b), (b, a)])
        .map(|(first, second)| {
            (
                format!("@if ({first}) {{}}\n@if ({second}) {{}}"),
                format!("@if ({first}) {{}}\n@else {{}}"),
            )
        })
        .collect();
    fix.extend(
        [
            (
                "@if (a) {\n  1\n} @else {\n  2\n}\n@if (!a) {\n  3\n}",
                "@if (a) {\n  1\n} @else {\n  2\n\n  3\n}\n",
            ),
            (
                "@if (a) {\n  1\n}\n@if (!a) {\n  2\n} @else {\n  3\n}",
                "@if (a) {\n  1\n\n  3\n}\n@else {\n  2\n}",
            ),
            (
                "@if (a) {\n  1\n} @else {\n  2\n}\n@if (!a) {\n  3\n} @else {\n  4\n}",
                "@if (a) {\n  1\n\n  4\n} @else {\n  2\n\n  3\n}\n",
            ),
            (
                "@if (a) {\n  <button>1</button>\n}\n@if (!a) {\n  <button>2</button>\n}\n@if (!a) {\n  <button>3</button>\n}",
                "@if (a) {\n  <button>1</button>\n}\n@else {\n  <button>2</button>\n}\n@if (!a) {\n  <button>3</button>\n}",
            ),
            ("@if (a) {}\n\n@if (!a) {}", "@if (a) {}\n\n@else {}"),
            ("@if (a) {}\n&nbsp;\n@if (!a) {}", "@if (a) {}\n&nbsp;\n@else {}"),
            ("@if (a) {}\n<!-- comment -->\n@if (!a) {}", "@if (a) {}\n<!-- comment -->\n@else {}"),
            ("@if (a; as alias) {}\n@if (!a) {}", "@if (a; as alias) {}\n@else {}"),
            // The third `@if` cannot become a second `@else`.
            ("@if (a) {} @if (!a) {} @if (a) {}", "@if (a) {} @else {} @if (a) {}"),
            ("@if (a) {} @if (!a; as x) {} @if (a) {}", "@if (a) {} @if (!a; as x) {} @else {}"),
        ]
        .map(|(a, b)| (a.to_string(), b.to_string())),
    );

    Tester::new(
        PreferAtElse::NAME,
        PreferAtElse::PLUGIN,
        pass.into_iter().map(String::from).collect(),
        fail,
    )
    .change_rule_path_extension("html")
    .expect_fix(fix)
    .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '@if (a) {} @else {}' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: `\n  @if (a) {}\n  @if (!a) {}\n` }) class A {}",
    ];
    let inline_fix = vec![(
        "import { Component } from '@angular/core'; @Component({ template: '@if (a) { x } @if (!a) { y }' }) class A {}",
        "import { Component } from '@angular/core'; @Component({ template: '@if (a) { x } @else { y }' }) class A {}",
    )];
    Tester::new(PreferAtElse::NAME, PreferAtElse::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .expect_fix(inline_fix)
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
