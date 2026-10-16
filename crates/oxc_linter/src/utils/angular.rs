//! Helpers shared by the `angular` plugin rules, ported from `@angular-eslint/utils`.

use oxc_angular_compiler::pipeline::selector::CssSelector;
use oxc_ast::{
    AstKind,
    ast::{
        Class, Decorator, Expression, ImportDeclaration, ImportDeclarationSpecifier,
        ObjectExpression, ObjectProperty, ObjectPropertyKind, Program, Statement, TSTypeName,
    },
};
use oxc_span::{GetSpan, Span};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{AstNode, context::LintContext};

pub const ANGULAR_CLASS_DECORATORS: [&str; 6] =
    ["Component", "Directive", "Injectable", "NgModule", "Pipe", "Service"];

pub const ANGULAR_LIFECYCLE_METHODS: [&str; 9] = [
    "ngAfterContentChecked",
    "ngAfterContentInit",
    "ngAfterViewChecked",
    "ngAfterViewInit",
    "ngDoBootstrap",
    "ngDoCheck",
    "ngOnChanges",
    "ngOnDestroy",
    "ngOnInit",
];

pub fn is_angular_lifecycle_method(name: &str) -> bool {
    ANGULAR_LIFECYCLE_METHODS.contains(&name)
}

/// `Foo` for `@Foo` and `@Foo(...)`.
pub fn decorator_name<'a>(decorator: &Decorator<'a>) -> Option<&'a str> {
    match &decorator.expression {
        Expression::Identifier(ident) => Some(ident.name.as_str()),
        _ => decorator_call_name(decorator),
    }
}

/// `Foo` for `@Foo(...)` only, the form `Decorator[expression.callee.name=...]` selectors match.
pub fn decorator_call_name<'a>(decorator: &Decorator<'a>) -> Option<&'a str> {
    let Expression::CallExpression(call) = &decorator.expression else { return None };
    let Expression::Identifier(callee) = &call.callee else { return None };
    Some(callee.name.as_str())
}

/// `@name(...)` on a class declaration (not a class expression).
pub fn class_decorator<'b, 'a>(class: &'b Class<'a>, name: &str) -> Option<&'b Decorator<'a>> {
    if !class.is_declaration() {
        return None;
    }
    class.decorators.iter().find(|decorator| decorator_call_name(decorator) == Some(name))
}

/// First decorator of `class` named after an Angular class decorator, in `@Foo` or `@Foo(...)` form.
pub fn angular_class_decorator<'b, 'a>(
    class: &'b Class<'a>,
) -> Option<(&'b Decorator<'a>, &'a str)> {
    class.decorators.iter().find_map(|decorator| {
        decorator_name(decorator)
            .filter(|name| ANGULAR_CLASS_DECORATORS.contains(name))
            .map(|name| (decorator, name))
    })
}

/// The object literal in `@Foo({ ... })`.
pub fn decorator_metadata<'b, 'a>(
    decorator: &'b Decorator<'a>,
) -> Option<&'b ObjectExpression<'a>> {
    let Expression::CallExpression(call) = &decorator.expression else { return None };
    match call.arguments.first()?.as_expression()? {
        Expression::ObjectExpression(object) => Some(object),
        _ => None,
    }
}

/// `name: value`, `'name': value` or `` `name`: value `` in a metadata object; computed `[name]` does not match.
pub fn metadata_property<'b, 'a>(
    metadata: &'b ObjectExpression<'a>,
    name: &str,
) -> Option<&'b ObjectProperty<'a>> {
    metadata.properties.iter().find_map(|property| match property {
        ObjectPropertyKind::ObjectProperty(property)
            if property.key.is_specific_static_name(name) =>
        {
            Some(&**property)
        }
        _ => None,
    })
}

/// Nearest ancestor decorator that sits directly on a class declaration, for selectors like
/// `ClassDeclaration > Decorator[...] Property`.
pub fn enclosing_class_decorator<'a>(
    node: &AstNode<'a>,
    ctx: &LintContext<'a>,
) -> Option<&'a Decorator<'a>> {
    let nodes = ctx.nodes();
    nodes.ancestors(node.id()).find_map(|ancestor| {
        let AstKind::Decorator(decorator) = ancestor.kind() else { return None };
        matches!(nodes.parent_kind(ancestor.id()), AstKind::Class(class) if class.is_declaration())
            .then_some(decorator)
    })
}

/// Nearest ancestor class declaration; class expressions are skipped.
pub fn nearest_class_declaration<'a>(
    node: &AstNode<'a>,
    ctx: &LintContext<'a>,
) -> Option<&'a Class<'a>> {
    ctx.nodes().ancestors(node.id()).find_map(|ancestor| match ancestor.kind() {
        AstKind::Class(class) if class.is_declaration() => Some(class),
        _ => None,
    })
}

/// `Foo` and `Bar` for `class X implements Foo, ns.Bar`.
pub fn implemented_interface_names<'b, 'a: 'b>(
    class: &'b Class<'a>,
) -> impl Iterator<Item = &'a str> + 'b {
    class.implements.iter().filter_map(|implements| match &implements.expression {
        TSTypeName::IdentifierReference(ident) => Some(ident.name.as_str()),
        TSTypeName::QualifiedName(name) => Some(name.right.name.as_str()),
        TSTypeName::ThisExpression(_) => None,
    })
}

/// A string literal's value, or a template literal's first quasi (raw) even when it has expressions.
pub fn literal_text<'a>(expr: &Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::StringLiteral(lit) => Some(lit.value.as_str()),
        Expression::TemplateLiteral(lit) => {
            lit.quasis.first().map(|quasi| quasi.value.raw.as_str())
        }
        _ => None,
    }
}

/// `"a"`, `"a" or "b"`, `"a", "b" or "c"`.
pub fn to_human_readable_text<S: AsRef<str>>(items: &[S]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| format!("\"{}\"", item.as_ref())).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

/// Span that removes an element of a comma-separated list (object property, array element,
/// argument) with its own trailing comma; the preceding comma stays so trailing-comma style holds.
pub fn list_item_removal_span(source: &str, item: Span) -> Span {
    let comma_after = next_token(source, item.end as usize, true)
        .filter(|&offset| source.as_bytes()[offset] == b',');
    if let Some(comma) = comma_after
        && let Some(next) = next_token(source, comma + 1, false)
        && !matches!(source.as_bytes()[next], b'}' | b']' | b')')
    {
        return Span::new(item.start, offset_u32(next));
    }
    let start = match prev_token_end(source, item.start as usize) {
        Some(end) if source.as_bytes()[end - 1] == b',' => offset_u32(end),
        _ => item.start,
    };
    Span::new(start, comma_after.map_or(item.end, |comma| offset_u32(comma + 1)))
}

/// Span that removes the named import `name` from `module`: the whole declaration when it is the
/// only specifier, otherwise the specifier and its adjacent comma.
pub fn named_import_removal_span(program: &Program, module: &str, name: &str) -> Option<Span> {
    let source = program.source_text;
    module_imports(program, module).find_map(|import| {
        let specifiers = import.specifiers.as_ref()?;
        let index = specifiers.iter().position(|specifier| {
            matches!(specifier, ImportDeclarationSpecifier::ImportSpecifier(s) if s.imported.name() == name)
        })?;
        let span = specifiers[index].span();
        if specifiers.len() == 1 {
            return Some(import.span);
        }
        if index == 0 {
            let comma = next_token(source, span.end as usize, true)?;
            return Some(Span::new(span.start, offset_u32(comma + 1)));
        }
        let before = prev_token_end(source, span.start as usize)?;
        (source.as_bytes()[before - 1] == b',').then(|| Span::new(offset_u32(before - 1), span.end))
    })
}

/// Text to insert after the returned span so `name` is imported from `module`; `None` when it
/// already is. Type-only imports are extended only with `allow_type_only`.
pub fn named_import_insertion(
    program: &Program,
    module: &str,
    name: &str,
    allow_type_only: bool,
) -> Option<(Span, String)> {
    let full_import = (Span::empty(0), format!("import {{ {name} }} from '{module}';\n"));
    let mut target = None;
    for import in module_imports(program, module) {
        let specifiers =
            import.specifiers.as_ref().map_or(&[][..], |specifiers| specifiers.as_slice());
        if specifiers.iter().any(|specifier| {
            matches!(specifier, ImportDeclarationSpecifier::ImportSpecifier(s) if s.imported.name() == name)
        }) {
            return None;
        }
        if import.import_kind.is_type() && !allow_type_only {
            continue;
        }
        match specifiers.last() {
            None | Some(ImportDeclarationSpecifier::ImportNamespaceSpecifier(_)) => {}
            Some(last) => target = Some(last),
        }
    }
    Some(match target {
        None => full_import,
        Some(ImportDeclarationSpecifier::ImportDefaultSpecifier(default)) => {
            (default.span, format!(", {{ {name} }}"))
        }
        Some(last) => (last.span(), format!(", {name}")),
    })
}

/// Text to insert after the returned span so `class` implements `interface`.
pub fn implements_insertion(class: &Class, interface: &str) -> (Span, String) {
    if let Some(last) = class.implements.last() {
        return (last.span, format!(", {interface}"));
    }
    let text = format!(" implements {interface}");
    if let Some(heritage) = &class.heritage {
        let span =
            heritage.type_arguments.as_ref().map_or(heritage.expression.span(), |args| args.span);
        return (span, text);
    }
    if let Some(type_parameters) = &class.type_parameters {
        return (type_parameters.span, text);
    }
    match &class.id {
        Some(id) => (id.span, text),
        None => (Span::empty(class.body.span.start), format!("implements {interface} ")),
    }
}

fn module_imports<'b, 'a>(
    program: &'b Program<'a>,
    module: &'b str,
) -> impl Iterator<Item = &'b ImportDeclaration<'a>> {
    program.body.iter().filter_map(move |statement| match statement {
        Statement::ImportDeclaration(import) if import.source.value == module => Some(&**import),
        _ => None,
    })
}

#[expect(clippy::cast_possible_truncation)]
fn offset_u32(offset: usize) -> u32 {
    offset as u32
}

// Start of the next token at or after `offset`, skipping whitespace and, with `skip_comments`, comments.
fn next_token(source: &str, mut offset: usize, skip_comments: bool) -> Option<usize> {
    loop {
        let rest = &source[offset..];
        let trimmed = rest.trim_start();
        offset += rest.len() - trimmed.len();
        if skip_comments && trimmed.starts_with("//") {
            offset += trimmed.find('\n')?;
        } else if skip_comments && trimmed.starts_with("/*") {
            offset += trimmed.find("*/")? + 2;
        } else {
            return (!trimmed.is_empty()).then_some(offset);
        }
    }
}

// End of the previous token (comments included) before `offset`.
fn prev_token_end(source: &str, offset: usize) -> Option<usize> {
    let end = source[..offset].trim_end().len();
    (end > 0).then_some(end)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SelectorType {
    Element,
    Attribute,
}

impl SelectorType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Element => "element",
            Self::Attribute => "attribute",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum SelectorStyle {
    #[serde(rename = "camelCase")]
    CamelCase,
    #[serde(rename = "kebab-case")]
    KebabCase,
}

impl SelectorStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CamelCase => "camelCase",
            Self::KebabCase => "kebab-case",
        }
    }

    pub fn matches(self, selector: &str) -> bool {
        match self {
            Self::CamelCase => {
                !selector.is_empty()
                    && selector.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'[' || b == b']')
            }
            Self::KebabCase => selector.split('-').all(|part| {
                !part.is_empty()
                    && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            }),
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T: Clone> OneOrMany<T> {
    fn to_vec(&self) -> Vec<T> {
        match self {
            Self::One(value) => vec![value.clone()],
            Self::Many(values) => values.clone(),
        }
    }
}

/// Options of `component-selector` and `directive-selector`: one config (whose `prefix`
/// defaults to `"app"`), or an array of 1-2 configs with distinct `type`s and no default prefix.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SelectorOptions {
    Single(SingleSelectorConfig),
    Multiple(Vec<SelectorConfigItem>),
}

// Upstream's default has no `style`, which disables the rule.
impl Default for SelectorOptions {
    fn default() -> Self {
        Self::Multiple(Vec::new())
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SingleSelectorConfig {
    r#type: OneOrMany<SelectorType>,
    #[serde(default = "default_selector_prefix")]
    prefix: OneOrMany<String>,
    style: SelectorStyle,
}

fn default_selector_prefix() -> OneOrMany<String> {
    OneOrMany::One("app".to_string())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectorConfigItem {
    r#type: SelectorType,
    prefix: Option<OneOrMany<String>>,
    style: SelectorStyle,
}

/// One normalized selector config; an empty `prefixes` requires no prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorConfig {
    pub kind: SelectorType,
    pub prefixes: Vec<String>,
    pub style: SelectorStyle,
}

fn normalize_prefix(prefix: Option<&OneOrMany<String>>) -> Vec<String> {
    match prefix {
        None => Vec::new(),
        Some(OneOrMany::One(prefix)) if prefix.is_empty() => Vec::new(),
        Some(prefix) => prefix.to_vec(),
    }
}

impl SelectorOptions {
    pub fn configs(&self) -> Vec<SelectorConfig> {
        match self {
            Self::Single(config) => {
                let prefixes = normalize_prefix(Some(&config.prefix));
                config
                    .r#type
                    .to_vec()
                    .into_iter()
                    .map(|kind| SelectorConfig {
                        kind,
                        prefixes: prefixes.clone(),
                        style: config.style,
                    })
                    .collect()
            }
            Self::Multiple(items) => items
                .iter()
                .map(|item| SelectorConfig {
                    kind: item.r#type,
                    prefixes: normalize_prefix(item.prefix.as_ref()),
                    style: item.style,
                })
                .collect(),
        }
    }

    /// The config for `selectors`: the only one, or with several, the one whose type matches
    /// the first selector (attribute when it has attributes, else element).
    pub fn applicable_config(&self, selectors: &[CssSelector]) -> Option<SelectorConfig> {
        let mut configs = self.configs();
        if configs.len() <= 1 {
            return configs.pop();
        }
        let actual = actual_selector_type(selectors)?;
        configs.into_iter().find(|config| config.kind == actual)
    }
}

fn actual_selector_type(selectors: &[CssSelector]) -> Option<SelectorType> {
    let first = selectors.first()?;
    if !first.attrs.is_empty() {
        return Some(SelectorType::Attribute);
    }
    first
        .element
        .as_deref()
        .is_some_and(|element| !element.is_empty() && element != "*")
        .then_some(SelectorType::Element)
}

/// Parses an Angular selector like `app-foo, [appBar]`; non-ASCII text yields no selectors.
pub fn parse_selector(text: &str) -> Vec<CssSelector> {
    // shortcut: the compiler parser slices by char index and panics on non-ASCII; selectors are ASCII in practice.
    if text.is_ascii() { CssSelector::parse(text) } else { Vec::new() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorCheck {
    pub has_expected_type: bool,
    pub has_expected_prefix: bool,
    pub has_expected_style: bool,
    pub has_selector_after_prefix: bool,
}

pub fn check_selector(selectors: &[CssSelector], config: &SelectorConfig) -> SelectorCheck {
    // Attribute values count as candidates too, as upstream concatenates `CssSelector.attrs`.
    let candidates: Vec<&str> = selectors
        .iter()
        .flat_map(|selector| match config.kind {
            SelectorType::Element => selector
                .element
                .as_deref()
                .filter(|element| !element.is_empty())
                .into_iter()
                .collect(),
            SelectorType::Attribute => {
                selector.attrs.iter().map(String::as_str).collect::<Vec<_>>()
            }
        })
        .collect();
    let prefixes = &config.prefixes;
    SelectorCheck {
        has_expected_type: !candidates.is_empty(),
        has_expected_prefix: prefixes.is_empty()
            || candidates.iter().any(|candidate| {
                prefixes.iter().any(|prefix| has_selector_prefix(candidate, prefix, config.style))
            }),
        has_expected_style: candidates.iter().any(|candidate| config.style.matches(candidate)),
        has_selector_after_prefix: prefixes.is_empty()
            || candidates.iter().any(|candidate| {
                prefixes.iter().any(|prefix| {
                    !strip_selector_prefix(candidate, prefix).unwrap_or(candidate).is_empty()
                })
            }),
    }
}

/// `selector` after an optional `[` and `prefix`; upstream's `^\[?(prefix)` regex, with `prefix` literal.
pub fn strip_selector_prefix<'s>(selector: &'s str, prefix: &str) -> Option<&'s str> {
    selector
        .strip_prefix('[')
        .and_then(|rest| rest.strip_prefix(prefix))
        .or_else(|| selector.strip_prefix(prefix))
}

/// Starts with `prefix` (empty matches all) followed by nothing, an uppercase/non-letter (camelCase)
/// or `-` (kebab-case).
pub fn has_selector_prefix(selector: &str, prefix: &str, style: SelectorStyle) -> bool {
    if prefix.is_empty() {
        return true;
    }
    match (style, strip_selector_prefix(selector, prefix).map(|rest| rest.chars().next())) {
        (_, None) => false,
        (_, Some(None)) => true,
        (SelectorStyle::CamelCase, Some(Some(c))) => !c.is_lowercase(),
        (SelectorStyle::KebabCase, Some(Some(c))) => c == '-',
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn list_item_removal() {
        let cases = [
            ("{ a: 1, b: 2, c: 3 }", "b: 2", "{ a: 1, c: 3 }"),
            ("{ a: 1, b: 2 }", "b: 2", "{ a: 1, }"),
            ("{ a: 1, b: 2, }", "b: 2", "{ a: 1, }"),
            ("{ a: 1 }", "a: 1", "{  }"),
            ("{ a: 1, /* keep */ b: 2 }", "a: 1", "{ /* keep */ b: 2 }"),
            ("{\n  a: 1,\n  b: 2,\n}", "a: 1", "{\n  b: 2,\n}"),
            ("{\n  a: 1,\n  b: 2,\n}", "b: 2", "{\n  a: 1,\n}"),
        ];
        for (source, item, expected) in cases {
            let start = source.find(item).unwrap();
            let span = list_item_removal_span(
                source,
                Span::new(offset_u32(start), offset_u32(start + item.len())),
            );
            let mut out = source.to_string();
            out.replace_range(span.start as usize..span.end as usize, "");
            assert_eq!(out, expected, "removing `{item}` from `{source}`");
        }
    }

    #[test]
    fn human_readable_text() {
        assert_eq!(to_human_readable_text(&["a"]), "\"a\"");
        assert_eq!(to_human_readable_text(&["a", "b", "c"]), "\"a\", \"b\" or \"c\"");
    }

    fn options(json: serde_json::Value) -> SelectorOptions {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn selector_options_prefix_default() {
        let single = options(serde_json::json!({ "type": "element", "style": "kebab-case" }));
        assert_eq!(single.configs()[0].prefixes, ["app"]);
        let multiple = options(serde_json::json!([
            { "type": "element", "style": "kebab-case" },
            { "type": "attribute", "prefix": ["x", "y"], "style": "camelCase" },
        ]));
        let configs = multiple.configs();
        assert_eq!(configs[0].prefixes, Vec::<String>::new());
        assert_eq!(configs[1].prefixes, ["x", "y"]);
        assert!(SelectorOptions::default().applicable_config(&parse_selector("app-x")).is_none());
    }

    #[test]
    fn selector_options_pick_by_type() {
        let options = options(serde_json::json!({
            "type": ["element", "attribute"], "prefix": "", "style": "camelCase",
        }));
        let attribute = options.applicable_config(&parse_selector("[appFoo]")).unwrap();
        assert_eq!(attribute.kind, SelectorType::Attribute);
        assert_eq!(attribute.prefixes, Vec::<String>::new());
        let element = options.applicable_config(&parse_selector("app-foo")).unwrap();
        assert_eq!(element.kind, SelectorType::Element);
    }

    fn check(
        selector: &str,
        kind: SelectorType,
        prefix: &str,
        style: SelectorStyle,
    ) -> SelectorCheck {
        let config = SelectorConfig { kind, prefixes: vec![prefix.to_string()], style };
        check_selector(&parse_selector(selector), &config)
    }

    #[test]
    fn selector_checks() {
        use SelectorStyle::{CamelCase, KebabCase};
        use SelectorType::{Attribute, Element};

        let ok = SelectorCheck {
            has_expected_type: true,
            has_expected_prefix: true,
            has_expected_style: true,
            has_selector_after_prefix: true,
        };
        assert_eq!(check("app-foo", Element, "app", KebabCase), ok);
        assert_eq!(check("[appFoo]", Attribute, "app", CamelCase), ok);
        assert_eq!(check("app-foo, [appBar]", Attribute, "app", CamelCase), ok);
        assert!(!check("[appFoo]", Element, "app", CamelCase).has_expected_type);
        assert!(!check("foo-bar", Element, "app", KebabCase).has_expected_prefix);
        assert!(!check("appfoo", Element, "app", KebabCase).has_expected_prefix);
        assert!(!check("[appfoo]", Attribute, "app", CamelCase).has_expected_prefix);
        assert!(!check("appFoo", Element, "app", KebabCase).has_expected_style);
        assert!(!check("app-foo", Attribute, "app", CamelCase).has_expected_type);
        assert!(!check("[app]", Attribute, "app", CamelCase).has_selector_after_prefix);
        assert!(check("[app-x]", Attribute, "", KebabCase).has_expected_prefix);
    }

    #[test]
    fn parse_selector_non_ascii() {
        assert!(parse_selector("ü-x").is_empty());
    }
}
