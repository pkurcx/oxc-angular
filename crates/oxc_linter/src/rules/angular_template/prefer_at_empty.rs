use std::cell::Cell;

use oxc_angular_compiler::ast::{
    expression::AngularExpression,
    r3::{R3ForLoopBlock, R3IfBlock, R3IfBlockBranch, R3Node},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use super::prefer_at_else::{are_equivalent, is_zero, template_text, walk_siblings};
use crate::{context::LintContext, fixer::Fix, rule::Rule, utils::AngularTemplate};

fn prefer_at_empty_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Prefer using `@for (...) {...} @empty {...}`.").with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferAtEmpty;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers the `@empty` block of `@for` over an `@if` or `@else` block that tests whether the
    /// same collection is empty.
    ///
    /// ### Why is this bad?
    ///
    /// `@empty` states the intent directly, without repeating the collection in a length check or
    /// nesting the loop in an `@if`.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// @for (item of items; track item.id) { {{ item }} }
    /// @if (items.length === 0) { No items }
    ///
    /// @if (items.length > 0) {
    ///   @for (item of items; track item.id) { {{ item }} }
    /// } @else {
    ///   No items
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// @for (item of items; track item.id) { {{ item }} } @empty { No items }
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - When two fixes would together create two `@empty` blocks, only the first is applied; the
    ///   second is applied on the next run.
    PreferAtEmpty,
    angular_template,
    style,
    conditional_fix,
    version = "next",
    short_description = "Prefer using `@empty` with `@for` loops instead of a separate `@if` or `@else` block to reduce code and make it easier to read.",
);

impl Rule for PreferAtEmpty {
    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            let checker = Checker { ctx, template, new_empty: Cell::new(None) };
            walk_siblings(template.nodes, &mut |node, previous| {
                let previous = previous.and_then(node_info);
                let previous = previous.as_ref();
                match node_info(node) {
                    Some(NodeInfo::For(block, collection)) => {
                        checker.check_for(block, collection, previous);
                    }
                    Some(NodeInfo::If(block, IfKind::Empty, collection)) => {
                        checker.check_if_empty(block, collection, previous);
                    }
                    Some(NodeInfo::If(block, IfKind::EmptyElse, collection)) => {
                        checker.check_if_empty_else(block, collection);
                    }
                    Some(NodeInfo::If(block, IfKind::NotEmpty, collection)) => {
                        checker.check_if_not_empty(block, collection, previous);
                    }
                    Some(NodeInfo::If(block, IfKind::NotEmptyElse, collection)) => {
                        checker.check_if_not_empty_else(block, collection);
                    }
                    None => {}
                }
            });
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum IfKind {
    /// `@if (x.length === 0) {}`
    Empty,
    /// `@if (x.length === 0) {} @else {}`
    EmptyElse,
    /// `@if (x.length > 0) {}`
    NotEmpty,
    /// `@if (x.length > 0) {}`, followed by `@else if` or `@else` branches.
    NotEmptyElse,
}

enum NodeInfo<'n, 'a> {
    For(&'n R3ForLoopBlock<'a>, &'n AngularExpression<'a>),
    If(&'n R3IfBlock<'a>, IfKind, &'n AngularExpression<'a>),
}

fn node_info<'n, 'a>(node: &'n R3Node<'a>) -> Option<NodeInfo<'n, 'a>> {
    match node {
        R3Node::ForLoopBlock(block) => Some(NodeInfo::For(block, &block.expression.ast)),
        R3Node::IfBlock(block) => {
            let condition = block.branches.first()?.expression.as_ref()?;
            if let Some(collection) = not_empty_test_collection(condition) {
                let kind =
                    if block.branches.len() == 1 { IfKind::NotEmpty } else { IfKind::NotEmptyElse };
                return Some(NodeInfo::If(block, kind, collection));
            }
            // With an `@else if`, the `@if` content could not move to `@empty`.
            let collection = empty_test_collection(condition)?;
            match block.branches.as_slice() {
                [_] => Some(NodeInfo::If(block, IfKind::Empty, collection)),
                [_, other] if other.expression.is_none() => {
                    Some(NodeInfo::If(block, IfKind::EmptyElse, collection))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// `x` of `x.length`.
fn length_receiver<'n, 'a>(
    expression: &'n AngularExpression<'a>,
) -> Option<&'n AngularExpression<'a>> {
    match expression {
        AngularExpression::PropertyRead(read) if read.name.as_str() == "length" => {
            Some(&read.receiver)
        }
        _ => None,
    }
}

/// `x` of `x.length`, `x.length > 0`, `x.length !== 0`, `0 < x.length`...
fn not_empty_test_collection<'n, 'a>(
    condition: &'n AngularExpression<'a>,
) -> Option<&'n AngularExpression<'a>> {
    if let Some(collection) = length_receiver(condition) {
        return Some(collection);
    }
    let AngularExpression::Binary(binary) = condition else { return None };
    let operator = binary.operation.as_str();
    if let Some(collection) = length_receiver(&binary.left) {
        (matches!(operator, "!==" | ">" | "!=") && is_zero(&binary.right)).then_some(collection)
    } else if is_zero(&binary.left) {
        if matches!(operator, "!==" | "<" | "!=") { length_receiver(&binary.right) } else { None }
    } else {
        None
    }
}

/// `x` of `!x.length`, `x.length === 0`, `0 == x.length`...
fn empty_test_collection<'n, 'a>(
    condition: &'n AngularExpression<'a>,
) -> Option<&'n AngularExpression<'a>> {
    match condition {
        AngularExpression::PrefixNot(not) => length_receiver(&not.expression),
        AngularExpression::Binary(binary) => {
            if !matches!(binary.operation.as_str(), "===" | "==") {
                return None;
            }
            if let Some(collection) = length_receiver(&binary.left) {
                is_zero(&binary.right).then_some(collection)
            } else if is_zero(&binary.left) {
                length_receiver(&binary.right)
            } else {
                None
            }
        }
        _ => None,
    }
}

struct Checker<'c, 'a> {
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
    /// The `@for` that a fix gives a new `@empty` block.
    new_empty: Cell<Option<Span>>,
}

impl<'a> Checker<'_, 'a> {
    fn span(&self, start: u32, end: u32) -> Span {
        self.template.span(Span::new(start, end))
    }

    fn text(&self, start: u32, end: u32) -> Option<&'a str> {
        template_text(self.ctx, self.template, start, end)
    }

    fn report(&self, name_span: Span, fixes: Option<Vec<Fix>>) {
        let diagnostic = prefer_at_empty_diagnostic(self.template.span(name_span));
        match fixes {
            Some(fixes) => self.ctx.diagnostic_with_fix(diagnostic, |fixer| {
                let mut fix = fixer.for_multifix().new_fix_with_capacity(fixes.len());
                for f in fixes {
                    fix.push(f);
                }
                fix.with_message("Move the content into an `@empty` block")
            }),
            None => self.ctx.diagnostic(diagnostic),
        }
    }

    /// The only `@for` among the children of a branch, ignoring whitespace.
    fn only_for_block<'n>(
        &self,
        branch: &'n R3IfBlockBranch<'a>,
    ) -> Option<&'n R3ForLoopBlock<'a>> {
        let mut for_block = None;
        for child in &branch.children {
            match child {
                R3Node::ForLoopBlock(block) if for_block.is_none() => for_block = Some(&**block),
                // `&nbsp;` is decoded in the text value, so the raw source is checked.
                R3Node::Text(text)
                    if self
                        .text(text.source_span.start, text.source_span.end)
                        .is_some_and(|text| text.trim().is_empty()) => {}
                _ => return None,
            }
        }
        for_block
    }

    /// `@if (x.length === 0) {...} @for (item of x) {}`
    fn check_for(
        &self,
        for_block: &R3ForLoopBlock<'a>,
        collection: &AngularExpression<'a>,
        previous: Option<&NodeInfo<'_, 'a>>,
    ) {
        let Some(NodeInfo::If(if_block, IfKind::Empty, previous_collection)) = previous else {
            return;
        };
        if !are_equivalent(collection, previous_collection) {
            return;
        }
        let branch = &if_block.branches[0];
        let fixes = branch.end_source_span.and_then(|branch_end| {
            let mut fixes = vec![Fix::delete(self.template.span(if_block.source_span))];
            if let Some(empty) = &for_block.empty {
                // The `@if` content renders before the existing `@empty` content.
                let content = self.text(branch.start_source_span.end, branch_end.end - 1)?;
                let at = empty.start_source_span.end;
                fixes.push(Fix::new(content.to_string(), self.span(at, at)));
            } else {
                let content = self.text(branch.start_source_span.end, branch_end.end)?;
                let at = for_block.source_span.end;
                fixes.push(Fix::new(format!(" @empty {{{content}"), self.span(at, at)));
                self.new_empty.set(Some(for_block.source_span));
            }
            Some(fixes)
        });
        self.report(if_block.name_span, fixes);
    }

    /// `@for (item of x) {} @if (x.length === 0) {...}`, or
    /// `@if (x.length > 0) { @for (item of x) {} } @if (x.length === 0) {...}`
    fn check_if_empty(
        &self,
        if_block: &R3IfBlock<'a>,
        collection: &AngularExpression<'a>,
        previous: Option<&NodeInfo<'_, 'a>>,
    ) {
        match previous {
            Some(NodeInfo::For(for_block, previous_collection))
                if are_equivalent(collection, previous_collection) =>
            {
                let start = self.template.span(if_block.start_source_span);
                let fixes = match for_block.empty.as_ref().and_then(|e| e.end_source_span) {
                    // The `@if` content renders after the existing `@empty` content.
                    Some(empty_end) => {
                        Some(vec![Fix::delete(self.template.span(empty_end)), Fix::delete(start)])
                    }
                    // A second `@empty` would not parse.
                    None if self.new_empty.get() == Some(for_block.source_span) => None,
                    None => Some(vec![Fix::new("@empty {", start)]),
                };
                self.report(if_block.name_span, fixes);
            }
            Some(NodeInfo::If(previous_block, IfKind::NotEmpty, previous_collection))
                if are_equivalent(collection, previous_collection) =>
            {
                let Some(for_block) = self.only_for_block(&previous_block.branches[0]) else {
                    return;
                };
                if !are_equivalent(collection, &for_block.expression.ast) {
                    return;
                }
                let fixes = previous_block.end_source_span.and_then(|previous_end| {
                    let content =
                        self.text(if_block.start_source_span.end, if_block.source_span.end)?;
                    let at = for_block.source_span.end;
                    Some(vec![
                        Fix::delete(self.template.span(previous_block.start_source_span)),
                        Fix::delete(self.template.span(previous_end)),
                        Fix::new(format!(" @empty {{{content}"), self.span(at, at)),
                        Fix::delete(self.template.span(if_block.source_span)),
                    ])
                });
                self.report(previous_block.name_span, fixes);
            }
            _ => {}
        }
    }

    /// `@if (x.length === 0) {...} @else { @for (item of x) {} }`
    fn check_if_empty_else(&self, if_block: &R3IfBlock<'a>, collection: &AngularExpression<'a>) {
        let else_branch = &if_block.branches[1];
        let Some(for_block) = self.only_for_block(else_branch) else { return };
        if !are_equivalent(collection, &for_block.expression.ast) {
            return;
        }
        let fixes = if_block.branches[0].end_source_span.and_then(|if_branch_end| {
            let mut fixes = vec![Fix::delete(
                self.span(if_block.source_span.start, else_branch.start_source_span.end),
            )];
            let content = self.text(if_block.start_source_span.end, if_branch_end.start)?;
            let existing = for_block.empty.as_ref().and_then(|e| Some((e, e.end_source_span?)));
            if let Some((empty, empty_end)) = existing {
                // Inside the `@else`, the existing `@empty` never renders, so it is replaced.
                fixes.push(Fix::new(
                    content.to_string(),
                    self.span(empty.start_source_span.end, empty_end.start),
                ));
                if let Some(end) = if_block.end_source_span {
                    fixes.push(Fix::delete(self.template.span(end)));
                }
            } else {
                // The `}` closing the `@else` closes the new `@empty`.
                let at = for_block.source_span.end;
                fixes.push(Fix::new(format!(" @empty {{{content}"), self.span(at, at)));
            }
            Some(fixes)
        });
        self.report(if_block.name_span, fixes);
    }

    /// `@if (x.length === 0) {...} @if (x.length > 0) { @for (item of x) {} }`
    fn check_if_not_empty(
        &self,
        if_block: &R3IfBlock<'a>,
        collection: &AngularExpression<'a>,
        previous: Option<&NodeInfo<'_, 'a>>,
    ) {
        let Some(NodeInfo::If(previous_block, IfKind::Empty, previous_collection)) = previous
        else {
            return;
        };
        if !are_equivalent(collection, previous_collection) {
            return;
        }
        let Some(for_block) = self.only_for_block(&if_block.branches[0]) else { return };
        if !are_equivalent(collection, &for_block.expression.ast) {
            return;
        }
        // The `}` closing the current `@if` closes the new `@empty`.
        let fixes = self
            .text(previous_block.start_source_span.end, previous_block.source_span.end - 1)
            .map(|content| {
                let at = for_block.source_span.end;
                vec![
                    Fix::delete(self.template.span(previous_block.source_span)),
                    Fix::delete(self.template.span(if_block.start_source_span)),
                    Fix::new(format!(" @empty {{{content}"), self.span(at, at)),
                ]
            });
        self.report(if_block.name_span, fixes);
    }

    /// `@if (x.length > 0) { @for (item of x) {} } @else {...}`
    fn check_if_not_empty_else(
        &self,
        if_block: &R3IfBlock<'a>,
        collection: &AngularExpression<'a>,
    ) {
        let Some(for_block) = self.only_for_block(&if_block.branches[0]) else { return };
        if !are_equivalent(collection, &for_block.expression.ast) {
            return;
        }
        let fixes = match (if_block.branches[0].end_source_span, if_block.end_source_span) {
            (Some(if_branch_end), Some(if_end)) => {
                let mut fixes = vec![];
                // Inside the `@if`, the existing `@empty` never renders.
                if let Some(empty) = &for_block.empty {
                    fixes.push(Fix::delete(self.template.span(empty.source_span)));
                }
                fixes.push(Fix::delete(self.template.span(if_block.start_source_span)));
                let else_branch = &if_block.branches[1];
                if else_branch.expression.is_some() {
                    // The `@else if` becomes an `@if` inside the new `@empty`.
                    fixes.push(Fix::new(
                        "@empty { @if ",
                        self.span(if_branch_end.end - 1, else_branch.name_span.end),
                    ));
                    fixes.push(Fix::new("}", self.span(if_end.end, if_end.end)));
                } else {
                    fixes.push(Fix::new(
                        "@empty {",
                        self.span(if_branch_end.end - 1, else_branch.start_source_span.end),
                    ));
                }
                Some(fixes)
            }
            _ => None,
        };
        self.report(if_block.name_span, fixes);
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "@for (item of items; track $index) {}",
        "@for (item of items; track $index) {} @empty {}",
        "
          @if (alpha.length > 0) {
            @for (item of beta; track $index) {}
          } @else {
            No items
          }
        ",
        "
          @if (items.length > 0) {
            Items:
            @for (item of items; track $index) {}
          } @else {
            No items
          }
        ",
        "
          @if (items.length === 0) {
            No items
          } @else {
            Items:
            @for (item of items; track $index) {}
          }
        ",
        "
          @if (items.length > 0) {
            @for (item of items; track $index) {}
            Total: {{ $count }}
          } @else {
            No items
          }
        ",
        "
          @if (items.length === 0) {
            No items
          } @else {
            @for (item of items; track $index) {}
            Total: {{ $count }}
          }
        ",
        "
          @if (items.length > 0) {
            @for (item of items; track $index) {}
          }
          @if (items.length > 0) {
            Not empty
          }
        ",
        "
          @if (items.length > 0) {
            @for (item of items; track $index) {}
          }
          ---
          @if (items.length === 0) {
            Not empty
          }
        ",
        "
          @for (item of items; track $index) {}
          ---
          @if (items.length === 0) {
            Not empty
          }
        ",
        "
          @for (item of items; track $index) {}
          @if (items.length > 0) {
            Not empty
          }
        ",
        "
          @if (items.length > 0) {
            Not empty
          }
          @for (item of items; track $index) {}
        ",
        "
          @if (beta.length > 0) {
            @for (item of alpha; track $index) {}
            @for (item of beta; track $index) {}
          } @else {
            Empty
          }
        ",
        r#"<li *ngFor="let item of items"></li><p *ngIf="items.length === 0">No items</p>"#,
        "<div>@for (item of items; track $index) {}</div>@if (items.length === 0) {}",
    ];

    let empty_conditions = [
        "items.length === 0",
        "items.length == 0",
        "0 === items.length",
        "0 == items.length",
        "!items.length",
    ];
    let not_empty_conditions = [
        "items.length > 0",
        "items.length !== 0",
        "items.length != 0",
        "0 < items.length",
        "0 !== items.length",
        "0 != items.length",
        "items.length",
    ];
    let for_then_if =
        |c: &str| format!("@for (item of items; track $index) {{}}\n@if ({c}) {{\n  No items\n}}");
    let if_then_for =
        |c: &str| format!("@if ({c}) {{\n  No items\n}}\n@for (item of items; track $index) {{}}");
    let for_in_if = |c: &str| {
        format!(
            "@if ({c}) {{\n  @for (item of items; track $index) {{}}\n}} @else {{\n  No items\n}}"
        )
    };
    let for_in_else = |c: &str| {
        format!(
            "@if ({c}) {{\n  No items\n}} @else {{\n  @for (item of items; track $index) {{}}\n}}"
        )
    };

    let others = [
        (
            "@if (items.length > 0) {\n  @for (item of items; track $index) {}\n}\n@if (items.length === 0) {\n  No items\n}",
            "\n  @for (item of items; track $index) {} @empty {\n  No items\n}\n\n",
        ),
        (
            "@if (items.length === 0) {\n  No items\n}\n@if (items.length > 0) {\n  @for (item of items; track $index) {}\n}",
            "\n\n  @for (item of items; track $index) {} @empty {\n  No items\n\n}",
        ),
        (
            "@if (items.length > 0) {\n  @for (item of items; track $index) {}\n} @else if (condition) {\n Condition is true\n} @else {\n Condition is false\n}",
            "\n  @for (item of items; track $index) {}\n@empty { @if (condition) {\n Condition is true\n} @else {\n Condition is false\n}}",
        ),
        (
            "@for (item of items; track $index) {}\n<!-- comment -->\n@if (items.length === 0) {\n  No items\n}",
            "@for (item of items; track $index) {}\n<!-- comment -->\n@empty {\n  No items\n}",
        ),
        (
            "<!-- The Angular compiler will ignore the space between the two blocks. -->\n@for (item of items; track $index) {}\n&nbsp;\n@if (items.length === 0) {\n  Not empty\n}",
            "<!-- The Angular compiler will ignore the space between the two blocks. -->\n@for (item of items; track $index) {}\n&nbsp;\n@empty {\n  Not empty\n}",
        ),
        (
            "@if (items.length === 0) {\n  Not empty\n}\n@for (item of items; track $index) {}\n@empty {\n  Existing\n}",
            "\n@for (item of items; track $index) {}\n@empty {\n  Not empty\n\n  Existing\n}",
        ),
        (
            "@for (item of items; track $index) {}\n@empty {\n  Existing\n}\n@if (items.length === 0) {\n  Not empty\n}",
            "@for (item of items; track $index) {}\n@empty {\n  Existing\n\n\n  Not empty\n}",
        ),
        (
            "@if (items.length > 0) {\n  @for (item of items; track $index) {}\n  @empty {\n    Existing\n  }\n} @else {\n  Empty\n}",
            "\n  @for (item of items; track $index) {}\n  \n@empty {\n  Empty\n}",
        ),
        (
            "@if (items.length === 0) {\n  Empty\n} @else {\n  @for (item of items; track $index) {}\n  @empty {\n    Existing\n  }\n}",
            "\n  @for (item of items; track $index) {}\n  @empty {\n  Empty\n}\n",
        ),
        (
            "@if (items.length === 0) {\n  Empty\n} @else {\n  @for (item of items; track $index) {}\n}",
            "\n  @for (item of items; track $index) {} @empty {\n  Empty\n\n}",
        ),
        (
            "@if (items.length > 0) {\n  <!-- before -->\n  @for (item of items; track $index) {}\n  <!-- after -->\n} @else {\n  Empty\n}",
            "\n  <!-- before -->\n  @for (item of items; track $index) {}\n  <!-- after -->\n@empty {\n  Empty\n}",
        ),
        (
            "@if (items.length === 0) {\n  Empty\n} @else {\n  <!-- before -->\n  @for (item of items; track $index) {}\n  <!-- after -->\n}",
            "\n  <!-- before -->\n  @for (item of items; track $index) {} @empty {\n  Empty\n\n  <!-- after -->\n}",
        ),
        (
            "@for (evt of eventQueue(); track evt) {\n  {{ evt }}\n}\n@if (eventQueue().length === 0) {\n  Awaiting app bootstrap...\n}",
            "@for (evt of eventQueue(); track evt) {\n  {{ evt }}\n}\n@empty {\n  Awaiting app bootstrap...\n}",
        ),
        (
            r#"<div *ngIf="x">@for (item of items; track $index) {} @if (!items.length) { None }</div>"#,
            r#"<div *ngIf="x">@for (item of items; track $index) {} @empty { None }</div>"#,
        ),
        (
            "<ng-template>@if (items.length) { @for (item of items; track $index) {} } @else { None }</ng-template>",
            "<ng-template> @for (item of items; track $index) {} @empty { None }</ng-template>",
        ),
        // Only the first `@if` moves into the new `@empty`; a second `@empty` would not parse.
        (
            "@if (x.length === 0) {A} @for (i of x; track i) {} @if (x.length === 0) {B}",
            " @for (i of x; track i) {} @empty {A} @if (x.length === 0) {B}",
        ),
    ];

    let mut fail: Vec<String> = vec![];
    let mut fix: Vec<(String, String)> = vec![];
    for c in empty_conditions {
        fail.push(for_then_if(c));
        fix.push((
            for_then_if(c),
            "@for (item of items; track $index) {}\n@empty {\n  No items\n}".into(),
        ));
        fail.push(if_then_for(c));
        fix.push((
            if_then_for(c),
            "\n@for (item of items; track $index) {} @empty {\n  No items\n}".into(),
        ));
        fail.push(for_in_else(c));
        fix.push((
            for_in_else(c),
            "\n  @for (item of items; track $index) {} @empty {\n  No items\n\n}".into(),
        ));
    }
    for c in not_empty_conditions {
        fail.push(for_in_if(c));
        fix.push((
            for_in_if(c),
            "\n  @for (item of items; track $index) {}\n@empty {\n  No items\n}".into(),
        ));
    }
    for (source, output) in others {
        fail.push(source.into());
        fix.push((source.into(), output.into()));
    }

    Tester::new(
        PreferAtEmpty::NAME,
        PreferAtEmpty::PLUGIN,
        pass.into_iter().map(String::from).collect(),
        fail,
    )
    .change_rule_path_extension("html")
    .expect_fix(fix)
    .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '@for (x of xs; track x) {} @empty {}' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: `\n  @for (x of xs; track x) {}\n  @if (!xs.length) { None }\n` }) class A {}",
    ];
    let inline_fix = vec![(
        "import { Component } from '@angular/core'; @Component({ template: '@for (x of xs; track x) {} @if (!xs.length) { None }' }) class A {}",
        "import { Component } from '@angular/core'; @Component({ template: '@for (x of xs; track x) {} @empty { None }' }) class A {}",
    )];
    Tester::new(PreferAtEmpty::NAME, PreferAtEmpty::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .expect_fix(inline_fix)
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
