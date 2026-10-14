use oxc_angular_compiler::ast::{
    expression::{AngularExpression, PropertyRead},
    r3::{R3ForLoopBlock, R3Node, R3Variable},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::prefer_at_else::{are_equivalent, is_number, is_zero, walk_siblings};
use crate::{
    context::LintContext,
    fixer::Fix,
    rule::{DefaultRuleConfig, Rule},
    utils::{AngularTemplate, for_each_expression, walk_expression},
};

fn prefer_contextual_variable_diagnostic(name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Use the '{name}' contextual variable instead of aliasing it."))
        .with_label(span)
}

fn simplification_diagnostic(variable: &str, expression: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Use '{variable}' instead of '{expression}'.")).with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PreferContextualForVariables(Box<PreferContextualForVariablesConfig>);

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct PreferContextualForVariablesConfig {
    /// Alias names allowed for each contextual variable.
    allowed_aliases: AllowedAliases,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct AllowedAliases {
    /// Aliases for `$count` that are allowed to be used.
    #[serde(rename = "$count")]
    count: Vec<String>,
    /// Aliases for `$index` that are allowed to be used.
    #[serde(rename = "$index")]
    index: Vec<String>,
    /// Aliases for `$first` that are allowed to be used.
    #[serde(rename = "$first")]
    first: Vec<String>,
    /// Aliases for `$last` that are allowed to be used.
    #[serde(rename = "$last")]
    last: Vec<String>,
    /// Aliases for `$even` that are allowed to be used.
    #[serde(rename = "$even")]
    even: Vec<String>,
    /// Aliases for `$odd` that are allowed to be used.
    #[serde(rename = "$odd")]
    odd: Vec<String>,
}

impl AllowedAliases {
    fn get(&self, variable: &str) -> &[String] {
        match variable {
            "$count" => &self.count,
            "$index" => &self.index,
            "$first" => &self.first,
            "$last" => &self.last,
            "$even" => &self.even,
            "$odd" => &self.odd,
            _ => &[],
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that the contextual variables of `@for` (`$index`, `$count`, `$first`, `$last`,
    /// `$even`, `$odd`) are used directly instead of through an alias, and instead of
    /// expressions that compute them, such as `$index === 0` or `items.length`.
    ///
    /// Aliases stay allowed in nested loops, where they are the only way to reach the outer
    /// loop's variables.
    ///
    /// ### Why is this bad?
    ///
    /// The contextual variables name their meaning: `$first` reads as "first item", where
    /// `i === 0` has to be translated.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// @for (item of items; track item.id; let i = $index) {
    ///   @if (i === 0) { first }
    ///   {{ i + 1 }} of {{ items.length }}
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// @for (item of items; track item.id) {
    ///   @if ($first) { first }
    ///   {{ $index + 1 }} of {{ $count }}
    /// }
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - The fix renames only plain reads of an alias, never `obj.i` or `this.i`.
    /// - Expressions inside `@empty` are not checked against the loop, as its variables are not in
    ///   scope there.
    /// - The fix also renames alias reads in `<ng-template>` outputs.
    /// - An alias is reported without a fix when the loop contains an ICU message, since reads
    ///   inside it are not renamed.
    PreferContextualForVariables,
    angular_template,
    style,
    fix,
    config = PreferContextualForVariables,
    version = "next",
    short_description = "Ensures that contextual variables are used in @for blocks where possible instead of aliasing them.",
);

impl Rule for PreferContextualForVariables {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            self.run_on_template(ctx, template);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Count,
    First,
    Last,
    Even,
    Odd,
}

impl Kind {
    fn variable(self) -> &'static str {
        match self {
            Self::Count => "$count",
            Self::First => "$first",
            Self::Last => "$last",
            Self::Even => "$even",
            Self::Odd => "$odd",
        }
    }
}

struct Simplification {
    kind: Kind,
    span: Span,
    replacement: &'static str,
}

struct LoopState<'n, 'a> {
    block: &'n R3ForLoopBlock<'a>,
    /// Aliased contextual variables, with the spans of the reads of each alias.
    aliases: Vec<(&'n R3Variable<'a>, Vec<Span>)>,
    simplifications: Vec<Simplification>,
}

impl LoopState<'_, '_> {
    fn record(&mut self, expression: &AngularExpression, kind: Kind, replacement: &'static str) {
        let span = expression.source_span().to_span();
        self.simplifications.push(Simplification { kind, span, replacement });
    }
}

impl PreferContextualForVariables {
    fn run_on_template(&self, ctx: &LintContext, template: &AngularTemplate) {
        let mut loops = vec![];
        let mut conditions = vec![];
        let mut handlers = vec![];
        let mut icus = vec![];
        walk_siblings(template.nodes, &mut |node, _| match node {
            R3Node::ForLoopBlock(block) => loops.push(LoopState {
                block,
                // Only aliases have a value span.
                aliases: block
                    .context_variables
                    .iter()
                    .filter(|variable| variable.value_span.is_some())
                    .map(|variable| (variable, vec![]))
                    .collect(),
                simplifications: vec![],
            }),
            R3Node::IfBlock(block) => {
                conditions.extend(block.branches.iter().filter_map(|b| b.expression.as_ref()));
            }
            R3Node::Template(t) if t.template_attrs.is_empty() => {
                handlers.extend(t.outputs.iter().map(|output| &output.handler));
            }
            R3Node::Icu(icu) => icus.push(icu.source_span),
            _ => {}
        });
        if loops.is_empty() {
            return;
        }

        // `loops` is in document order, so the innermost loop around a span is the last one
        // that contains it. The loop variables are not in scope in `@empty`, unlike
        // `@angular-eslint` assumes.
        let innermost = |loops: &[LoopState], span: Span| {
            loops.iter().rposition(|l| {
                Span::new(l.block.source_span.start, l.block.main_block_span.end)
                    .contains_inclusive(span)
            })
        };

        for_each_expression(template.nodes, &mut |expression, _| {
            let Some(index) = innermost(&loops, expression.source_span().to_span()) else {
                return;
            };
            record_expression(&mut loops[index], expression);
        });
        // `@angular-eslint` skips `<ng-template>` outputs, but a fix must still rename the alias.
        for handler in handlers {
            walk_expression(handler, None, &mut |expression, _| {
                if let AngularExpression::PropertyRead(read) = expression
                    && let Some(index) = innermost(&loops, read.source_span.to_span())
                {
                    record_reference(&mut loops[index], read);
                }
            });
        }
        // A condition of `$index % 2` is used as a truthy value.
        for condition in conditions {
            if let Some(index) = innermost(&loops, condition.source_span().to_span())
                && is_index_mod_two(condition)
            {
                loops[index].record(condition, Kind::Odd, "$odd");
            }
        }

        for (i, state) in loops.iter().enumerate() {
            let span = state.block.source_span;
            // Aliases are needed to reach the variables of an outer loop from an inner one.
            let nested = loops.iter().enumerate().any(|(j, other)| {
                let other = other.block.source_span;
                j != i
                    && [(other, span), (span, other)].iter().any(|(a, b)| a.contains_inclusive(*b))
            });
            if !nested {
                // Alias reads inside an ICU are not visited, so renaming them is not possible.
                let has_icu = icus.iter().any(|icu| span.contains_inclusive(*icu));
                self.report_aliases(ctx, template, state, has_icu);
            }
            report_simplifications(ctx, template, state);
        }
    }

    fn report_aliases(
        &self,
        ctx: &LintContext,
        template: &AngularTemplate,
        state: &LoopState,
        has_icu: bool,
    ) {
        let count = state.aliases.len();
        for (index, (variable, references)) in state.aliases.iter().enumerate() {
            let allowed = self.0.allowed_aliases.get(variable.value.as_str());
            if allowed.iter().any(|alias| alias.as_str() == variable.name.as_str()) {
                continue;
            }
            let diagnostic = prefer_contextual_variable_diagnostic(
                &variable.value,
                template.span(variable.source_span),
            );
            let Some(removed) = variable_range_to_remove(ctx, template, variable, index, count)
                .filter(|_| !has_icu)
            else {
                ctx.diagnostic(diagnostic);
                continue;
            };
            ctx.diagnostic_with_fix(diagnostic, |fixer| {
                let mut fix = fixer.for_multifix().new_fix_with_capacity(references.len() + 1);
                fix.push(Fix::delete(removed));
                for reference in references {
                    fix.push(Fix::new(variable.value.to_string(), template.span(*reference)));
                }
                fix.with_message(format!("Use `{}` instead of the alias", variable.value))
            });
        }
    }
}

fn report_simplifications(ctx: &LintContext, template: &AngularTemplate, state: &LoopState) {
    for kind in [Kind::Count, Kind::First, Kind::Last, Kind::Even, Kind::Odd] {
        for simplification in state.simplifications.iter().filter(|s| s.kind == kind) {
            let span = template.span(simplification.span);
            let Some(expression) = ctx.source_text().get(span.start as usize..span.end as usize)
            else {
                continue;
            };
            ctx.diagnostic_with_fix(
                simplification_diagnostic(kind.variable(), expression, span),
                |fixer| fixer.replace(span, simplification.replacement),
            );
        }
    }
}

fn record_expression<'a>(state: &mut LoopState<'_, 'a>, expression: &AngularExpression<'a>) {
    use AngularExpression as E;
    match expression {
        E::PropertyRead(read) => {
            record_reference(state, read);
            if read.name.as_str() == "length"
                && are_equivalent(&read.receiver, &state.block.expression.ast)
            {
                state.record(expression, Kind::Count, "$count");
            }
        }
        E::Binary(binary) => {
            record_binary(
                state,
                expression,
                &binary.left,
                &binary.right,
                binary.operation.as_str(),
            );
        }
        E::PrefixNot(not) => {
            if is_contextual(&not.expression, "$odd") || is_index_mod_two(&not.expression) {
                state.record(expression, Kind::Even, "$even");
            } else if is_contextual(&not.expression, "$even") {
                state.record(expression, Kind::Odd, "$odd");
            }
        }
        E::Conditional(conditional) if is_index_mod_two(&conditional.condition) => {
            state.record(&conditional.condition, Kind::Odd, "$odd");
        }
        _ => {}
    }
}

fn record_reference(state: &mut LoopState, read: &PropertyRead) {
    // Only implicit reads refer to the alias; `obj.i` and `this.i` do not.
    if matches!(read.receiver, AngularExpression::ImplicitReceiver(_)) {
        let span = read.source_span.to_span();
        if let Some((_, references)) =
            state.aliases.iter_mut().find(|(variable, _)| variable.name == read.name)
            && !references.contains(&span)
        {
            references.push(span);
        }
    }
}

fn record_binary<'a>(
    state: &mut LoopState<'_, 'a>,
    node: &AngularExpression<'a>,
    left: &AngularExpression<'a>,
    right: &AngularExpression<'a>,
    operator: &str,
) {
    let inequality = matches!(operator, "!==" | "!=");
    let logical = matches!(operator, "&&" | "||");
    // Records `equal` for `===`/`==`, and `unequal` for `!==`/`!=` or `other`.
    let mut check = |equal: (Kind, &'static str), other: &str, unequal: (Kind, &'static str)| {
        if matches!(operator, "===" | "==") {
            state.record(node, equal.0, equal.1);
        } else if inequality || operator == other {
            state.record(node, unequal.0, unequal.1);
        }
    };
    let first = (Kind::First, "$first");
    let not_first = (Kind::First, "!$first");
    let last = (Kind::Last, "$last");
    let not_last = (Kind::Last, "!$last");
    let even = (Kind::Even, "$even");
    let odd = (Kind::Odd, "$odd");
    if is_index(left) {
        if is_zero(right) {
            check(first, ">", not_first);
        } else if is_count_minus_one(right) {
            check(last, "<", not_last);
        }
    } else if is_zero(left) {
        if is_index(right) {
            check(first, "<", not_first);
        } else if is_index_mod_two(right) {
            check(even, "<", odd);
        }
    } else if is_number(left, 1.0) {
        if is_index_mod_two(right) {
            check(odd, ">", even);
        }
    } else if is_contextual(left, "$count") {
        if is_index_plus_one(right) {
            check(last, ">", not_last);
        }
    } else if is_index_plus_one(left) {
        if is_contextual(right, "$count") {
            check(last, "<", not_last);
        }
    } else if is_count_minus_one(left) {
        if is_index(right) {
            check(last, ">", not_last);
        }
    } else if is_index_mod_two(left) {
        if is_zero(right) {
            check(even, ">", odd);
        } else if is_number(right, 1.0) {
            check(odd, "<", even);
        } else if logical {
            // `$index % 2` is a number, so it is only replaced where it is used as a truthy
            // value, such as an operand of `&&`.
            state.record(left, Kind::Odd, "$odd");
        }
    }
    if logical && is_index_mod_two(right) {
        state.record(right, Kind::Odd, "$odd");
    }
}

/// An implicit read of a contextual variable: `this.$index` is a component property.
fn is_contextual(expression: &AngularExpression, name: &str) -> bool {
    matches!(expression, AngularExpression::PropertyRead(read)
        if read.name.as_str() == name && matches!(read.receiver, AngularExpression::ImplicitReceiver(_)))
}

fn is_index(expression: &AngularExpression) -> bool {
    is_contextual(expression, "$index")
}

fn unwrap_parenthesized<'n, 'a>(
    expression: &'n AngularExpression<'a>,
) -> &'n AngularExpression<'a> {
    match expression {
        AngularExpression::ParenthesizedExpression(e) => &e.expression,
        e => e,
    }
}

/// `$index + 1` or `1 + $index`.
fn is_index_plus_one(expression: &AngularExpression) -> bool {
    let AngularExpression::Binary(binary) = unwrap_parenthesized(expression) else { return false };
    binary.operation.as_str() == "+"
        && if is_index(&binary.left) {
            is_number(&binary.right, 1.0)
        } else {
            is_index(&binary.right) && is_number(&binary.left, 1.0)
        }
}

/// `$index % 2`.
fn is_index_mod_two(expression: &AngularExpression) -> bool {
    let AngularExpression::Binary(binary) = unwrap_parenthesized(expression) else { return false };
    binary.operation.as_str() == "%" && is_index(&binary.left) && is_number(&binary.right, 2.0)
}

/// `$count - 1` or `1 - $count`.
fn is_count_minus_one(expression: &AngularExpression) -> bool {
    let AngularExpression::Binary(binary) = unwrap_parenthesized(expression) else { return false };
    binary.operation.as_str() == "-"
        && if is_contextual(&binary.left, "$count") {
            is_number(&binary.right, 1.0)
        } else {
            is_contextual(&binary.right, "$count") && is_number(&binary.left, 1.0)
        }
}

/// The source range that removes an alias from the `@for` parameters, along with its `let`
/// keyword when no other alias shares it, or with the comma that separates it from the others.
fn variable_range_to_remove(
    ctx: &LintContext,
    template: &AngularTemplate,
    variable: &R3Variable,
    index: usize,
    count: usize,
) -> Option<Span> {
    let text = ctx.source_text();
    let span = template.span(variable.source_span);
    let (mut start, mut end) = (span.start as usize, span.end as usize);
    let before = text.get(..start)?;
    let after = text.get(end..)?;
    let let_index = before.rfind("let");
    let has_own_let = let_index.is_some_and(|let_index| {
        !before[let_index + 3..].contains(',') && !after.trim_start().starts_with(',')
    });
    if count == 1 || has_own_let {
        if let Some(let_index) = let_index {
            // Also removes the `;` before the `let`.
            start = before[..let_index].rfind(';').unwrap_or(let_index);
        }
    } else if index == 0 {
        // Keeps the `let`, and removes the comma and whitespace after the alias.
        if let Some(comma) = after.find(',') {
            end = text.len() - after[comma + 1..].trim_start().len();
        }
    } else {
        start = before.rfind(',').unwrap_or(start);
    }
    #[expect(clippy::cast_possible_truncation)]
    Some(Span::new(start as u32, end as u32))
}

#[test]
fn test() {
    use crate::tester::Tester;
    use serde_json::json;

    const VARIABLES: [&str; 6] = ["$index", "$count", "$first", "$last", "$even", "$odd"];

    let mut pass: Vec<(String, Option<serde_json::Value>)> = vec![
        ("@for (item of items; track item.id) {}".into(), None),
        ("@for (item of items; track $index) {}".into(), None),
    ];
    for variable in VARIABLES {
        pass.push((
            format!("@for (item of items; track item.id; let beta = {variable}) {{}}"),
            Some(json!([{ "allowedAliases": { variable: ["alpha", "beta", "gamma"] } }])),
        ));
    }
    for variable in VARIABLES {
        pass.push((
            format!(
                "@for (a of outer; track a.id; let outerVar = {variable}) {{\n  @for (b of inner; track b.id) {{}}\n}}"
            ),
            None,
        ));
        pass.push((
            format!(
                "@for (a of outer; track a.id) {{\n  @for (b of inner; track b.id; let innerVar = {variable}) {{}}\n}}"
            ),
            None,
        ));
        pass.push((
            format!(
                "@for (a of outer; track a.id; let outerVar = {variable}) {{\n  @for (b of inner; track b.id; let innerVar = {variable}) {{}}\n}}"
            ),
            None,
        ));
    }
    pass.extend(
        [
            "@for (item of items; track item.id) {}\n{{ items.length }}",
            "@for (a of outer; track a.id) {\n  @for (b of inner; track b.id) {\n    {{ outer.length }}\n  }\n}",
            "@for (a of outer(); track a.id) {\n  @for (b of inner(); track b.id) {\n    {{ outer().length }}\n  }\n}",
            "@for (item of items; track item.id) {\n  {{ this.$index === 0 }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index === this.$count - 1 }}\n}",
            "@for (item of items; track item.id) {}\n{{ $index % 2 === 0 }}",
            "@for (item of items; track item.id) {}\n{{ $index % 2 !== 0 }}",
            "@for (item of items; track item.id) {}\n{{ !$even }}",
            "@for (item of items; track item.id) {}\n{{ !$odd }}",
            "@for (item of items; track item.id) {\n  @if (this.$index % 2 === 0) {}\n}",
            "@for (item of items; track item.id) {\n  @if (this.$index % 2 !== 0) {}\n}",
            "@for (item of items; track item.id) {\n  {{ !this.$even }}\n}",
            "@for (item of items; track item.id) {\n  {{ !this.$odd }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 === foo }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 !== foo }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 >= foo }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 > foo }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 <= foo }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 < foo }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 + foo }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 - foo }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 * foo }}\n}",
            "@for (item of items; track item.id) {\n  {{ $index % 2 / foo }}\n}",
            r#"<li *ngFor="let item of items; let i = index">{{ i }}</li>"#,
        ]
        .map(|source| (source.to_string(), None)),
    );

    let mut fail: Vec<(String, Option<serde_json::Value>)> = vec![];
    let mut fix: Vec<(String, String, Option<serde_json::Value>)> = vec![];
    let mut fail_and_fix = |source: String, output: String, config: Option<serde_json::Value>| {
        fail.push((source.clone(), config.clone()));
        fix.push((source, output, config));
    };

    for variable in VARIABLES {
        for alias in [&variable[1..], "foo"] {
            fail_and_fix(
                format!("@for (item of items; track item.id; let {alias} = {variable}) {{}}"),
                "@for (item of items; track item.id) {}".into(),
                None,
            );
        }
        fail_and_fix(
            format!("@for (item of items; track item.id; let bar = {variable}) {{}}"),
            "@for (item of items; track item.id) {}".into(),
            Some(json!([{ "allowedAliases": { variable: ["foo"] } }])),
        );
    }
    let allow_a_b = json!([{ "allowedAliases": { "$index": ["a"], "$count": ["b"] } }]);
    fail_and_fix(
        "@for (item of items; track item.id; let x = $even, a = $index, b = $count) {,}".into(),
        "@for (item of items; track item.id; let a = $index, b = $count) {,}".into(),
        Some(allow_a_b.clone()),
    );
    fail_and_fix(
        "@for (item of items; track item.id; let a = $index, x = $even, b = $count) {,}".into(),
        "@for (item of items; track item.id; let a = $index, b = $count) {,}".into(),
        Some(allow_a_b.clone()),
    );
    fail_and_fix(
        "@for (item of items; track item.id; let a = $index, b = $count, x = $even) {,}".into(),
        "@for (item of items; track item.id; let a = $index, b = $count) {,}".into(),
        Some(allow_a_b),
    );
    let allow_b = json!([{ "allowedAliases": { "$count": ["b"] } }]);
    fail_and_fix(
        "@for (item of items; track item.id; let a = $index, b = $count, x = $even) {,}".into(),
        "@for (item of items; track item.id; let b = $count) {,}".into(),
        Some(allow_b.clone()),
    );
    // Overlapping fixes: the second one applies on the next pass.
    fail_and_fix(
        "@for (item of items; track item.id; let a = $index, x = $even, b = $count) {,}".into(),
        "@for (item of items; track item.id; let x = $even, b = $count) {,}".into(),
        Some(allow_b),
    );
    fail_and_fix(
        "@for (item of items; track item.id; let a = $index, b = $even, c = $count) {,}".into(),
        "@for (item of items; track item.id; let b = $even) {,}".into(),
        None,
    );
    for variable in VARIABLES {
        fail_and_fix(
            format!(
                "@for (item of items; track item.id; let foo = {variable}) {{\n  {{{{ foo }}}}\n  {{{{ foo + 1 }}}}\n  <my-component [value]=\"foo\"/>\n  <div [attr.title]=\"foo\"></div>\n  @if (foo) {{}}\n  @switch (foo) {{}}\n  @let bar = foo + 1;\n}}"
            ),
            format!(
                "@for (item of items; track item.id) {{\n  {{{{ {variable} }}}}\n  {{{{ {variable} + 1 }}}}\n  <my-component [value]=\"{variable}\"/>\n  <div [attr.title]=\"{variable}\"></div>\n  @if ({variable}) {{}}\n  @switch ({variable}) {{}}\n  @let bar = {variable} + 1;\n}}"
            ),
            None,
        );
    }
    for source in ["items", "items()"] {
        fail_and_fix(
            format!(
                "@for (item of {source}; track item.id) {{\n  {{{{ $index + 1 }}}} of {{{{ {source}.length }}}}\n}}"
            ),
            format!(
                "@for (item of {source}; track item.id) {{\n  {{{{ $index + 1 }}}} of {{{{ $count }}}}\n}}"
            ),
            None,
        );
    }
    let in_if = |expression: &str| {
        format!("@for (item of items; track item.id) {{\n  @if ({expression}) {{\n    x\n  }}\n}}")
    };
    let cases: [(&[&str], &str); 6] = [
        (&["$index === 0", "$index == 0", "0 === $index", "0 == $index"], "$first"),
        (
            &[
                "$index > 0",
                "$index !== 0",
                "$index != 0",
                "0 < $index",
                "0 !== $index",
                "0 != $index",
            ],
            "!$first",
        ),
        (
            &[
                "$index + 1 === $count",
                "$index + 1 == $count",
                "$index === $count - 1",
                "$index == $count - 1",
                "$count === $index + 1",
                "$count == $index + 1",
                "$count - 1 === $index",
                "$count - 1 == $index",
            ],
            "$last",
        ),
        (
            &[
                "$index + 1 < $count",
                "$index + 1 !== $count",
                "$index < $count - 1",
                "$index !== $count - 1",
                "$count > $index + 1",
                "$count !== $index + 1",
                "$count - 1 > $index",
                "$count - 1 !== $index",
            ],
            "!$last",
        ),
        (
            &[
                "!($index % 2)",
                "$index % 2 === 0",
                "$index % 2 == 0",
                "$index % 2 !== 1",
                "$index % 2 != 1",
                "$index % 2 < 1",
                "0 === $index % 2",
                "0 == $index % 2",
                "1 !== $index % 2",
                "1 > $index % 2",
                "!$odd",
            ],
            "$even",
        ),
        (
            &[
                "$index % 2 === 1",
                "$index % 2 == 1",
                "$index % 2 !== 0",
                "$index % 2 != 0",
                "$index % 2 > 0",
                "1 === $index % 2",
                "1 == $index % 2",
                "0 !== $index % 2",
                "0 != $index % 2",
                "0 < $index % 2",
                "!$even",
            ],
            "$odd",
        ),
    ];
    for (expressions, replacement) in cases {
        for expression in expressions {
            fail_and_fix(in_if(expression), in_if(replacement), None);
        }
    }
    // Only the inner `!` is reported; the outer one simplifies on the next pass.
    fail_and_fix(in_if("!!($index % 2)"), in_if("!$even"), None);
    for (pre, post) in
        [("{{ foo && ", " }}"), ("{{ ", " && foo }}"), ("{{ ", " ? 1 : 2 }}"), ("@if (", ") {}")]
    {
        fail_and_fix(
            format!("@for (item of items; track item.id) {{\n  {pre}$index % 2{post}\n}}"),
            format!("@for (item of items; track item.id) {{\n  {pre}$odd{post}\n}}"),
            None,
        );
    }
    // Overlapping fixes: the second alias is removed on the next pass.
    fail_and_fix(
        "@for (example of examples; track example; let index = $index; let last = $last) {\n  {{ index }}\n  {{ last }}\n}".into(),
        "@for (example of examples; track example; let last = $last) {\n  {{ $index }}\n  {{ last }}\n}".into(),
        None,
    );
    fail_and_fix(
        "@for (a of outer; track a.id) {\n  @if ($index === 0) {\n    first outer\n  }\n  @for (b of inner; track b.id) {\n    @if ($index === 0) {\n      first inner\n    }\n  }\n}".into(),
        "@for (a of outer; track a.id) {\n  @if ($first) {\n    first outer\n  }\n  @for (b of inner; track b.id) {\n    @if ($first) {\n      first inner\n    }\n  }\n}".into(),
        None,
    );
    // Not the alias: a property of another object, or of the component.
    fail_and_fix(
        "@for (item of items; track item.id; let i = $index) {\n  {{ i }} {{ obj.i }} {{ this.i }}\n}".into(),
        "@for (item of items; track item.id) {\n  {{ $index }} {{ obj.i }} {{ this.i }}\n}".into(),
        None,
    );
    fail_and_fix(
        "@for (item of items; track i; let i = $index) { {{ items.length }} }".into(),
        "@for (item of items; track $index) { {{ $count }} }".into(),
        None,
    );
    fail_and_fix(
        r#"<div *ngIf="show">@for (item of items; track item.id) { <span *ngIf="$index === 0"></span> }</div>"#.into(),
        r#"<div *ngIf="show">@for (item of items; track item.id) { <span *ngIf="$first"></span> }</div>"#.into(),
        None,
    );
    fail_and_fix(
        "<ng-template>@for (item of items; track item.id; let i = $index) { {{ i }} }</ng-template>".into(),
        "<ng-template>@for (item of items; track item.id) { {{ $index }} }</ng-template>".into(),
        None,
    );
    // `@empty` reads the component, not the loop variables.
    fail_and_fix(
        "@for (item of items; track item.id; let i = $index) { {{ i }} {{ items.length }} } @empty { {{ i }} {{ items.length }} }".into(),
        "@for (item of items; track item.id) { {{ $index }} {{ $count }} } @empty { {{ i }} {{ items.length }} }".into(),
        None,
    );
    fail_and_fix(
        "@for (a of outer; track a) { @for (b of inner; track b) {} @empty { {{ $index === 0 }} } }".into(),
        "@for (a of outer; track a) { @for (b of inner; track b) {} @empty { {{ $first }} } }".into(),
        None,
    );
    let icu = "@for (item of items; track item.id; let i = $index) { <span i18n>{i, plural, =0 {first} other {more}}</span> }";
    fail_and_fix(icu.into(), icu.into(), None);
    fail_and_fix(
        r#"@for (item of items; track item.id; let i = $index) { <ng-template [a]="i" (b)="f(i)"></ng-template> }"#.into(),
        r#"@for (item of items; track item.id) { <ng-template [a]="$index" (b)="f($index)"></ng-template> }"#.into(),
        None,
    );

    Tester::new(
        PreferContextualForVariables::NAME,
        PreferContextualForVariables::PLUGIN,
        pass.iter().map(|(s, c)| (s.as_str(), c.clone())).collect(),
        fail.iter().map(|(s, c)| (s.as_str(), c.clone())).collect(),
    )
    .change_rule_path_extension("html")
    .expect_fix(fix)
    .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '@for (x of xs; track x) { {{ $index }} }' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: `\n  @for (x of xs; track x; let i = $index) { {{ i }} }\n` }) class A {}",
    ];
    let inline_fix = vec![(
        "import { Component } from '@angular/core'; @Component({ template: '@for (x of xs; track x; let i = $index) { {{ i }} }' }) class A {}",
        "import { Component } from '@angular/core'; @Component({ template: '@for (x of xs; track x) { {{ $index }} }' }) class A {}",
    )];
    Tester::new(
        PreferContextualForVariables::NAME,
        PreferContextualForVariables::PLUGIN,
        inline_pass,
        inline_fail,
    )
    .change_rule_path_extension("ts")
    .expect_fix(inline_fix)
    .with_snapshot_suffix("inline")
    .test_and_snapshot();
}
