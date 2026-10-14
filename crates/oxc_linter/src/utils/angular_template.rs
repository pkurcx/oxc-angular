//! Angular templates (`.html` files and inline `@Component({ template })`), parsed with
//! `oxc_angular_compiler` into the R3 AST that `@angular-eslint/template` rules inspect.

use cow_utils::CowUtils;
use oxc_allocator::Allocator;
use oxc_angular_compiler::{
    ast::{
        expression::{AbsoluteSourceSpan, AngularExpression, LiteralValue},
        r3::{
            R3BoundAttribute, R3BoundEvent, R3BoundText, R3DeferredBlock, R3Element,
            R3ForLoopBlock, R3IfBlockBranch, R3LetDeclaration, R3Node, R3SwitchBlock,
            R3SwitchBlockCase, R3Template, R3TemplateAttr, R3Visitor, visit_all,
        },
    },
    parser::{ParseTemplateOptions, html::HtmlParser},
    transform::html_to_r3::{HtmlToR3Transform, TransformOptions},
};
use oxc_ast::{
    AstKind, Comment, CommentKind,
    ast::{Expression, ObjectPropertyKind},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_span::Span;

use crate::{
    AllowWarnDeny,
    context::{ContextHost, LintContext, imports_angular},
    disable_directives::{
        DisableDirectives, DisableDirectivesBuilder, create_unused_directives_diagnostics,
    },
    fixer::{Message, PossibleFixes},
};

/// A parsed template and where it sits in the linted source text.
pub struct AngularTemplate<'a> {
    pub nodes: &'a [R3Node<'a>],
    pub offset: u32,
    end: u32,
    /// The template's `<!-- eslint-disable -->` directives, which apply only to its own rules,
    /// as in `@angular-eslint` where an inline template is linted as a file of its own.
    pub(crate) directives: DisableDirectives,
}

impl AngularTemplate<'_> {
    pub(crate) fn contains(&self, span: Span) -> bool {
        Span::new(self.offset, self.end).contains_inclusive(span)
    }

    /// Span of a template node in the linted source text.
    pub fn span(&self, span: Span) -> Span {
        Span::new(span.start + self.offset, span.end + self.offset)
    }

    /// Span of a template expression in the linted source text.
    pub fn expression_span(&self, span: AbsoluteSourceSpan) -> Span {
        Span::new(span.start + self.offset, span.end + self.offset)
    }
}

/// The parsed templates of a file, and the parse errors of those that failed.
#[derive(Default)]
pub struct AngularTemplates<'a> {
    pub templates: Vec<AngularTemplate<'a>>,
    pub errors: Vec<OxcDiagnostic>,
}

/// Parses the templates of the current file: the whole `.html` file, or each inline
/// `template` of an `@angular/core` `@Component` in a script.
/// Templates that fail to parse are skipped, as `@angular-eslint` does, and their errors kept.
pub fn build_angular_templates<'a>(host: &ContextHost<'a>) -> AngularTemplates<'a> {
    let sources = match host.current_sub_host().angular_template_text() {
        Some(text) => vec![(text, 0)],
        None => inline_templates(host),
    };
    let respect_eslint = host.current_sub_host().respect_eslint_disable_directives;
    let mut result = AngularTemplates::default();
    for (text, offset) in sources {
        match parse(host.allocator(), text) {
            Ok(nodes) => {
                #[expect(clippy::cast_possible_truncation)]
                let end = offset + text.len() as u32;
                // Built on the text up to the template's end, so an unclosed `eslint-disable`
                // stops there.
                let directives = DisableDirectivesBuilder::new()
                    .with_respect_eslint_disable_directives(respect_eslint)
                    .build(&host.source_text()[..end as usize], &html_comments(text, offset));
                result.templates.push(AngularTemplate { nodes, offset, end, directives });
            }
            Err(errors) => result.errors.extend(errors.into_iter().map(|(message, start)| {
                OxcDiagnostic::error(format!("Angular template: {message}"))
                    .with_label(Span::empty(start + offset))
            })),
        }
    }
    result
}

/// Reports the template parse errors and unused template directives of a file once its rules
/// have run. Templates are only parsed when an `angular-template` rule runs.
pub fn report_angular_template_diagnostics(
    host: &ContextHost,
    unused_directives: Option<AllowWarnDeny>,
) {
    let Some(templates) = host.angular_templates.get() else { return };
    for error in &templates.errors {
        host.push_diagnostic(Message::new(error.clone(), PossibleFixes::None));
    }
    let Some(severity) = unused_directives else { return };
    for template in &templates.templates {
        // Without fixes: a fix sized for `//` comments would leave `-->` behind.
        for diagnostic in create_unused_directives_diagnostics(&template.directives, severity) {
            host.push_diagnostic(Message::new(diagnostic, PossibleFixes::None));
        }
    }
}

/// `<!-- -->` comments of a template, as comments of the source text it sits at `offset` in.
/// The span ends before `-->`, which makes `content_span()` the text between the delimiters.
fn html_comments(text: &str, offset: u32) -> Vec<Comment> {
    let mut comments = vec![];
    let mut rest = 0;
    while let Some(start) = text[rest..].find("<!--").map(|i| rest + i) {
        let Some(end) = text[start + 4..].find("-->").map(|i| start + 4 + i) else { break };
        #[expect(clippy::cast_possible_truncation)]
        comments.push(Comment::new(
            offset + start as u32,
            offset + end as u32,
            CommentKind::HtmlOpen,
        ));
        rest = end + 3;
    }
    comments
}

fn parse<'a>(
    allocator: &'a Allocator,
    text: &'a str,
) -> Result<&'a [R3Node<'a>], Vec<(String, u32)>> {
    // Same options as `@angular-eslint/template-parser`, which keeps whitespace and comments.
    let options = ParseTemplateOptions {
        preserve_whitespaces: true,
        preserve_line_endings: true,
        collect_comment_nodes: true,
        enable_block_syntax: true,
        enable_let_syntax: true,
        tokenize_expansion_forms: true,
        ..ParseTemplateOptions::default()
    };
    let html = HtmlParser::with_options(allocator, text, "", &options).parse();
    if !html.errors.is_empty() {
        return Err(html.errors.into_iter().map(|e| (e.msg, e.span.start.offset)).collect());
    }
    let transform_options = TransformOptions { collect_comment_nodes: true, angular_version: None };
    let r3 = HtmlToR3Transform::new(allocator, text, transform_options).transform(&html.nodes);
    if !r3.errors.is_empty() {
        return Err(r3.errors.into_iter().map(|e| (e.msg, e.span.start.offset)).collect());
    }
    Ok(allocator.alloc(r3.nodes).as_slice())
}

/// Raw text of each inline template, with its offset in the script.
/// shortcut: templates with escape sequences or `${}` are skipped, since spans must map back to
/// the source unchanged; decode them with an offset map if users miss their diagnostics.
fn inline_templates<'a>(host: &ContextHost<'a>) -> Vec<(&'a str, u32)> {
    if !imports_angular(host.module_record()) {
        return vec![];
    }
    let source_text = host.source_text();
    host.semantic()
        .nodes()
        .iter()
        .filter_map(|node| {
            let AstKind::Decorator(decorator) = node.kind() else { return None };
            let Expression::CallExpression(call) = &decorator.expression else { return None };
            if !call.callee.is_specific_id("Component") {
                return None;
            }
            let Some(Expression::ObjectExpression(object)) =
                call.arguments.first().and_then(|arg| arg.as_expression())
            else {
                return None;
            };
            object.properties.iter().find_map(|property| {
                let ObjectPropertyKind::ObjectProperty(property) = property else { return None };
                if property.key.static_name().as_deref() != Some("template") {
                    return None;
                }
                let span = match &property.value {
                    Expression::StringLiteral(literal) => literal.span,
                    Expression::TemplateLiteral(literal) if literal.expressions.is_empty() => {
                        literal.span
                    }
                    _ => return None,
                };
                let start = span.start + 1;
                let text = &source_text[start as usize..span.end as usize - 1];
                (!text.contains('\\')).then_some((text, start))
            })
        })
        .collect()
}

/// Calls `f` on every expression of a template and on each of its subexpressions, with its
/// parent expression: bindings, events, interpolations, structural directive attributes,
/// block parameters, and `@defer` `when` triggers.
pub fn for_each_expression<'a>(nodes: &[R3Node<'a>], f: &mut ExpressionCallback<'_, 'a>) {
    visit_all(&mut ExpressionVisitor { f }, nodes);
}

pub type ExpressionCallback<'f, 'a> =
    dyn FnMut(&AngularExpression<'a>, Option<&AngularExpression<'a>>) + 'f;

/// Visits an element's parts in the order `@angular-eslint/template-parser` traverses them.
pub fn visit_element_in_eslint_order<'a, V: R3Visitor<'a> + ?Sized>(
    visitor: &mut V,
    element: &R3Element<'a>,
) {
    visit_all(visitor, &element.children);
    element.inputs.iter().for_each(|input| visitor.visit_bound_attribute(input));
    element.outputs.iter().for_each(|output| visitor.visit_bound_event(output));
    element.attributes.iter().for_each(|attr| visitor.visit_text_attribute(attr));
}

/// Visits a template's parts in the order `@angular-eslint/template-parser` traverses them,
/// which skips its attributes and outputs. A structural directive's template holds copies of
/// the wrapped element's inputs, which are visited on the element instead.
pub fn visit_template_in_eslint_order<'a, V: R3Visitor<'a> + ?Sized>(
    visitor: &mut V,
    template: &R3Template<'a>,
) {
    for attr in &template.template_attrs {
        match attr {
            R3TemplateAttr::Bound(attr) => visitor.visit_bound_attribute(attr),
            R3TemplateAttr::Text(attr) => visitor.visit_text_attribute(attr),
        }
    }
    visit_all(visitor, &template.children);
    if template.template_attrs.is_empty() {
        template.inputs.iter().for_each(|input| visitor.visit_bound_attribute(input));
    }
}

/// Calls `f` on `expression` and on each of its subexpressions, parents first.
pub fn walk_expression<'a>(
    expression: &AngularExpression<'a>,
    parent: Option<&AngularExpression<'a>>,
    f: &mut ExpressionCallback<'_, 'a>,
) {
    f(expression, parent);
    let mut walk = |e: &AngularExpression<'a>| walk_expression(e, Some(expression), f);
    match expression {
        AngularExpression::Empty(_)
        | AngularExpression::ImplicitReceiver(_)
        | AngularExpression::ThisReceiver(_)
        | AngularExpression::LiteralPrimitive(_)
        | AngularExpression::RegularExpressionLiteral(_) => {}
        AngularExpression::Chain(e) => e.expressions.iter().for_each(walk),
        AngularExpression::Conditional(e) => {
            walk(&e.condition);
            walk(&e.true_exp);
            walk(&e.false_exp);
        }
        AngularExpression::PropertyRead(e) => walk(&e.receiver),
        AngularExpression::SafePropertyRead(e) => walk(&e.receiver),
        AngularExpression::KeyedRead(e) => {
            walk(&e.receiver);
            walk(&e.key);
        }
        AngularExpression::SafeKeyedRead(e) => {
            walk(&e.receiver);
            walk(&e.key);
        }
        AngularExpression::BindingPipe(e) => {
            walk(&e.exp);
            e.args.iter().for_each(walk);
        }
        AngularExpression::LiteralArray(e) => e.expressions.iter().for_each(walk),
        AngularExpression::LiteralMap(e) => e.values.iter().for_each(walk),
        AngularExpression::Interpolation(e) => e.expressions.iter().for_each(walk),
        AngularExpression::Binary(e) => {
            walk(&e.left);
            walk(&e.right);
        }
        AngularExpression::Unary(e) => walk(&e.expr),
        AngularExpression::PrefixNot(e) => walk(&e.expression),
        AngularExpression::TypeofExpression(e) => walk(&e.expression),
        AngularExpression::VoidExpression(e) => walk(&e.expression),
        AngularExpression::NonNullAssert(e) => walk(&e.expression),
        AngularExpression::Call(e) => {
            walk(&e.receiver);
            e.args.iter().for_each(walk);
        }
        AngularExpression::SafeCall(e) => {
            walk(&e.receiver);
            e.args.iter().for_each(walk);
        }
        AngularExpression::TaggedTemplateLiteral(e) => {
            walk(&e.tag);
            e.template.expressions.iter().for_each(walk);
        }
        AngularExpression::TemplateLiteral(e) => e.expressions.iter().for_each(walk),
        AngularExpression::ParenthesizedExpression(e) => walk(&e.expression),
        AngularExpression::SpreadElement(e) => walk(&e.expression),
        AngularExpression::ArrowFunction(e) => walk(&e.body),
    }
}

struct ExpressionVisitor<'f, 'g, 'a> {
    f: &'f mut ExpressionCallback<'g, 'a>,
}

impl<'a> ExpressionVisitor<'_, '_, 'a> {
    fn walk(&mut self, expression: &AngularExpression<'a>) {
        walk_expression(expression, None, self.f);
    }
}

impl<'a> R3Visitor<'a> for ExpressionVisitor<'_, '_, 'a> {
    fn visit_bound_text(&mut self, text: &R3BoundText<'a>) {
        self.walk(&text.value);
    }

    fn visit_bound_attribute(&mut self, attr: &R3BoundAttribute<'a>) {
        self.walk(&attr.value);
    }

    fn visit_bound_event(&mut self, event: &R3BoundEvent<'a>) {
        self.walk(&event.handler);
    }

    fn visit_element(&mut self, element: &R3Element<'a>) {
        visit_element_in_eslint_order(self, element);
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        visit_template_in_eslint_order(self, template);
    }

    fn visit_if_block_branch(&mut self, branch: &R3IfBlockBranch<'a>) {
        if let Some(expression) = &branch.expression {
            self.walk(expression);
        }
        visit_all(self, &branch.children);
    }

    fn visit_for_loop_block(&mut self, block: &R3ForLoopBlock<'a>) {
        self.walk(&block.expression.ast);
        self.walk(&block.track_by.ast);
        visit_all(self, &block.children);
        if let Some(empty) = &block.empty {
            self.visit_for_loop_block_empty(empty);
        }
    }

    fn visit_switch_block(&mut self, block: &R3SwitchBlock<'a>) {
        self.walk(&block.expression);
        for group in &block.groups {
            self.visit_switch_block_case_group(group);
        }
    }

    fn visit_switch_block_case(&mut self, case: &R3SwitchBlockCase<'a>) {
        if let Some(expression) = &case.expression {
            self.walk(expression);
        }
    }

    fn visit_let_declaration(&mut self, declaration: &R3LetDeclaration<'a>) {
        self.walk(&declaration.value);
    }

    fn visit_deferred_block(&mut self, block: &R3DeferredBlock<'a>) {
        for triggers in [&block.hydrate_triggers, &block.triggers, &block.prefetch_triggers] {
            if let Some(when) = &triggers.when {
                self.walk(&when.value);
            }
        }
        visit_all(self, &block.children);
        if let Some(placeholder) = &block.placeholder {
            self.visit_deferred_block_placeholder(placeholder);
        }
        if let Some(loading) = &block.loading {
            self.visit_deferred_block_loading(loading);
        }
        if let Some(error) = &block.error {
            self.visit_deferred_block_error(error);
        }
    }
}

/// Native elements as `@angular-eslint` lists them: `aria-query` 5.3.2's `dom` keys and the
/// elements it adds.
pub const ANGULAR_DOM_ELEMENTS: phf::Set<&'static str> = phf::phf_set![
    ":math:math",
    ":svg:svg",
    "a",
    "abbr",
    "acronym",
    "address",
    "applet",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "base",
    "bdi",
    "bdo",
    "big",
    "blink",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "content",
    "data",
    "datalist",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "font",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "keygen",
    "label",
    "legend",
    "li",
    "link",
    "main",
    "map",
    "mark",
    "marquee",
    "menu",
    "menuitem",
    "meta",
    "meter",
    "nav",
    "noembed",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "param",
    "picture",
    "pre",
    "progress",
    "q",
    "rb",
    "rp",
    "rt",
    "rtc",
    "ruby",
    "s",
    "samp",
    "script",
    "section",
    "select",
    "selectedcontent",
    "slot",
    "small",
    "source",
    "spacer",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "tt",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
    "xmp",
];

/// Whether `name` is a native element, as `@angular-eslint` matches them: lower case, or also
/// upper case where a rule allows it.
pub fn is_angular_dom_element(name: &str, allow_upper_case: bool) -> bool {
    ANGULAR_DOM_ELEMENTS.contains(name)
        || (allow_upper_case
            && name.bytes().all(|b| !b.is_ascii_lowercase())
            && ANGULAR_DOM_ELEMENTS.contains(name.cow_to_ascii_lowercase().as_ref()))
}

/// The value of an element's attribute or input, as `@angular-eslint`'s `getAttributeValue`
/// reads it.
pub enum AngularAttributeValue<'e, 'a> {
    Text(&'a str),
    Literal(&'e LiteralValue<'a>),
    /// Any other bound expression.
    Other,
}

impl AngularAttributeValue<'_, '_> {
    /// The value as JavaScript's `String(value)` would print it, for the primitive cases.
    pub fn as_string(&self) -> Option<String> {
        match self {
            Self::Text(text) => Some((*text).to_string()),
            Self::Literal(LiteralValue::String(value)) => Some(value.to_string()),
            Self::Literal(LiteralValue::Boolean(value)) => Some(value.to_string()),
            Self::Literal(LiteralValue::Number(value)) => Some(value.to_string()),
            Self::Literal(LiteralValue::Null) => Some("null".to_string()),
            Self::Literal(LiteralValue::Undefined) => Some("undefined".to_string()),
            _ => None,
        }
    }

    /// The value if it is a string, as in `typeof value === 'string'`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Literal(LiteralValue::String(value)) => Some(value.as_str()),
            _ => None,
        }
    }
}

/// The name an attribute or input is written with: `aria-label` for `[attr.aria-label]`,
/// `style.display` for `[style.display]`.
pub fn angular_original_attribute_name<'a>(
    ctx: &LintContext<'a>,
    template: &AngularTemplate,
    key_span: Span,
) -> &'a str {
    let name = angular_binding_name(ctx, template, key_span);
    name.strip_prefix("attr.").unwrap_or(name)
}

/// The key of a binding without its syntax: `attr.x` for `[attr.x]`, `click` for `(click)`.
pub fn angular_binding_name<'a>(
    ctx: &LintContext<'a>,
    template: &AngularTemplate,
    key_span: Span,
) -> &'a str {
    let key = ctx.source_range(template.span(key_span));
    let key = key.trim_start_matches(['[', '(', '#', '*']).trim_end_matches([']', ')']);
    ["bindon-", "bind-", "on-", "ref-"]
        .iter()
        .find_map(|prefix| key.strip_prefix(prefix))
        .unwrap_or(key)
}

pub fn angular_attribute_value<'e, 'a>(
    ctx: &LintContext<'a>,
    template: &AngularTemplate,
    element: &'e R3Element<'a>,
    name: &str,
) -> Option<AngularAttributeValue<'e, 'a>> {
    if let Some(attr) = element.attributes.iter().find(|attr| attr.name == name) {
        return Some(AngularAttributeValue::Text(attr.value.as_str()));
    }
    let input = element
        .inputs
        .iter()
        .find(|input| angular_original_attribute_name(ctx, template, input.key_span) == name)?;
    Some(match &input.value {
        AngularExpression::LiteralPrimitive(literal) => {
            AngularAttributeValue::Literal(&literal.value)
        }
        _ => AngularAttributeValue::Other,
    })
}

/// Whether an element's content is hidden from screen readers by its own attributes or styles;
/// callers also check its ancestors.
pub fn is_angular_element_hidden<'a>(
    ctx: &LintContext<'a>,
    template: &AngularTemplate,
    element: &R3Element<'a>,
) -> bool {
    let value = |name: &str| angular_attribute_value(ctx, template, element, name);
    let string = |name: &str| value(name).and_then(|v| v.as_string());
    let is_html_truthy = |name: &str| string(name).is_some_and(|v| v.is_empty() || v == "true");
    let hidden_style =
        value("style").as_ref().and_then(AngularAttributeValue::as_str).is_some_and(|style| {
            let style: String =
                style.chars().filter(|c| !c.is_whitespace() && *c != '\'' && *c != '"').collect();
            style.contains("display:none") || style.contains("visibility:hidden")
        });
    let literal_str = |name: &str| value(name).and_then(|v| v.as_str().map(str::to_string));
    hidden_style
        || literal_str("style.display").as_deref() == Some("none")
        || string("style.display.none").as_deref() == Some("true")
        || literal_str("style.visibility").as_deref() == Some("hidden")
        || string("style.visibility.hidden").as_deref() == Some("true")
        || is_html_truthy("aria-hidden")
        || is_html_truthy("hidden")
        || (element.name.eq_ignore_ascii_case("input")
            && literal_str("type").is_some_and(|t| t.eq_ignore_ascii_case("hidden")))
}

/// Every element of a template, parents first.
pub fn angular_elements<'a>(nodes: &'a [R3Node<'a>]) -> Vec<&'a R3Element<'a>> {
    fn collect<'a>(nodes: &'a [R3Node<'a>], out: &mut Vec<&'a R3Element<'a>>) {
        for node in nodes {
            match node {
                R3Node::Element(element) => {
                    out.push(element);
                    collect(&element.children, out);
                }
                R3Node::Template(template) => collect(&template.children, out),
                R3Node::Content(content) => collect(&content.children, out),
                R3Node::Component(component) => collect(&component.children, out),
                R3Node::IfBlock(block) => {
                    block.branches.iter().for_each(|branch| collect(&branch.children, out));
                }
                R3Node::IfBlockBranch(branch) => collect(&branch.children, out),
                R3Node::ForLoopBlock(block) => {
                    collect(&block.children, out);
                    if let Some(empty) = &block.empty {
                        collect(&empty.children, out);
                    }
                }
                R3Node::ForLoopBlockEmpty(empty) => collect(&empty.children, out),
                R3Node::SwitchBlock(block) => {
                    block.groups.iter().for_each(|group| collect(&group.children, out));
                }
                R3Node::SwitchBlockCaseGroup(group) => collect(&group.children, out),
                R3Node::DeferredBlock(block) => {
                    collect(&block.children, out);
                    block.placeholder.iter().for_each(|b| collect(&b.children, out));
                    block.loading.iter().for_each(|b| collect(&b.children, out));
                    block.error.iter().for_each(|b| collect(&b.children, out));
                }
                R3Node::DeferredBlockPlaceholder(b) => collect(&b.children, out),
                R3Node::DeferredBlockLoading(b) => collect(&b.children, out),
                R3Node::DeferredBlockError(b) => collect(&b.children, out),
                _ => {}
            }
        }
    }
    let mut out = vec![];
    collect(nodes, &mut out);
    out
}
