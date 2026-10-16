use oxc_ast::{
    AstKind,
    ast::{
        Argument, CallExpression, Decorator, Expression, MethodDefinitionKind,
        MethodDefinitionType, ObjectExpression, ObjectProperty, ObjectPropertyKind,
        PropertyDefinitionType, PropertyKey,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use oxc_str::CompactStr;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    fixer::RuleFixer,
    rule::{DefaultRuleConfig, Rule},
    utils::{
        class_decorator, decorator_call_name, decorator_metadata, enclosing_class_decorator,
        literal_text, metadata_property, nearest_class_declaration,
    },
};

fn no_input_rename_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Input bindings should not be aliased")
        .with_help("See https://angular.dev/guide/components/inputs#choosing-input-names")
        .with_label(span)
}

const REMOVE_ALIAS: &str = "Remove alias name";
const USE_ALIAS: &str = "Remove alias name and use it as the original name";

#[derive(Debug, Default, Clone, Deserialize)]
pub struct NoInputRename(Box<NoInputRenameConfig>);

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct NoInputRenameConfig {
    /// Aliases that are always allowed.
    allowed_names: Vec<CompactStr>,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows aliasing input bindings with `@Input('alias')`, `@Input({ alias })`,
    /// `input(value, { alias })`, `input.required({ alias })` or `'name: alias'` in the `inputs`
    /// metadata.
    ///
    /// An alias is allowed when it is listed in `allowedNames`, when it is an ARIA attribute
    /// whose camelCase form is the property name (`'aria-label'` for `ariaLabel`), or when the
    /// enclosing `@Component` or `@Directive` selector makes it the natural name: the selector
    /// itself, its attribute name, or the selector followed by the capitalized property name.
    /// The `inputs` of a `hostDirectives` entry are not checked.
    ///
    /// ### Why is this bad?
    ///
    /// An aliased input has one name in the class and another in templates, which makes the
    /// component harder to read and refactor.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({ selector: 'app-user' })
    /// class UserComponent {
    ///   @Input('userName') name: string;
    ///   label = input('', { alias: 'title' });
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({ selector: 'app-user' })
    /// class UserComponent {
    ///   @Input() name: string;
    ///   label = input('');
    ///   @Input('aria-label') ariaLabel: string;
    /// }
    /// ```
    NoInputRename,
    angular,
    style,
    fix_suggestion,
    config = NoInputRenameConfig,
    version = "next",
    short_description = "Disallow aliasing input bindings.",
);

impl Rule for NoInputRename {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::ObjectProperty(property) => self.check_inputs_metadata(node, property, ctx),
            AstKind::PropertyDefinition(property)
                if property.r#type == PropertyDefinitionType::PropertyDefinition =>
            {
                let signal = match &property.value {
                    Some(Expression::CallExpression(call))
                        if call.callee.is_specific_id("input")
                            || call.callee.is_specific_member_access("input", "required") =>
                    {
                        Some(&**call)
                    }
                    _ => None,
                };
                self.check_member(node, &property.key, &property.decorators, signal, ctx);
            }
            AstKind::MethodDefinition(method)
                if method.kind == MethodDefinitionKind::Set
                    && method.r#type == MethodDefinitionType::MethodDefinition =>
            {
                self.check_member(node, &method.key, &method.decorators, None, ctx);
            }
            _ => {}
        }
    }
}

impl NoInputRename {
    fn check_member<'a>(
        &self,
        node: &AstNode<'a>,
        key: &PropertyKey<'a>,
        decorators: &'a [Decorator<'a>],
        signal: Option<&'a CallExpression<'a>>,
        ctx: &LintContext<'a>,
    ) {
        let property_name = match key {
            PropertyKey::StaticIdentifier(ident) => ident.name.as_str(),
            PropertyKey::Identifier(ident) => ident.name.as_str(),
            _ => return,
        };
        let mut check = |literal: &'a Expression<'a>, alias_property: Option<AliasProperty<'a>>| {
            let Some(alias) = static_text(literal) else { return };
            if self.is_allowed_name(alias, property_name) {
                return;
            }
            let removal = alias_removal_span(ctx.source_text(), literal.span(), alias_property);
            if alias == property_name {
                ctx.diagnostic_with_fix(no_input_rename_diagnostic(literal.span()), |fixer| {
                    fixer.delete_range(removal)
                });
            } else if !selector_allows_alias(node, ctx, property_name, alias) {
                let fixer = RuleFixer::new(FixKind::Suggestion, ctx);
                let new_key =
                    if alias.contains('-') { format!("'{alias}'") } else { alias.to_string() };
                ctx.diagnostic_with_suggestions(
                    no_input_rename_diagnostic(literal.span()),
                    [
                        fixer.delete_range(removal).with_message(REMOVE_ALIAS),
                        fixer
                            .delete_range(removal)
                            .extend(fixer.replace(key.span(), new_key))
                            .with_message(USE_ALIAS),
                    ],
                );
            }
        };
        if let Some(decorator) =
            decorators.iter().find(|decorator| decorator_call_name(decorator) == Some("Input"))
            && let Expression::CallExpression(call) = &decorator.expression
        {
            for_each_alias(call, true, &mut check);
        }
        if let Some(call) = signal {
            for_each_alias(call, false, &mut check);
        }
    }

    fn check_inputs_metadata<'a>(
        &self,
        node: &AstNode<'a>,
        property: &ObjectProperty<'a>,
        ctx: &LintContext<'a>,
    ) {
        let Expression::ArrayExpression(array) = &property.value else { return };
        if !property.key.is_specific_static_name("inputs") || is_host_directive_entry(node, ctx) {
            return;
        }
        let Some(decorator) = enclosing_class_decorator(node, ctx) else { return };
        if !matches!(decorator_call_name(decorator), Some("Component" | "Directive")) {
            return;
        }
        for element in &array.elements {
            let Some(literal) = element.as_expression() else { continue };
            let Some(text) = static_text(literal) else { continue };
            let text = without_brackets_and_whitespace(text);
            // `'name: alias'`
            let mut names = text.split(':');
            let property_name = names.next().unwrap_or_default();
            let Some(alias) = names.next().filter(|alias| !alias.is_empty()) else { continue };
            if self.is_allowed_name(alias, property_name) {
                continue;
            }
            let span = literal.span();
            let quote = if matches!(literal, Expression::StringLiteral(_)) { '\'' } else { '`' };
            if alias == property_name {
                ctx.diagnostic_with_fix(no_input_rename_diagnostic(span), |fixer| {
                    fixer.replace(span, format!("{quote}{property_name}{quote}"))
                });
            } else if !selector_allows_alias(node, ctx, property_name, alias) {
                let fixer = RuleFixer::new(FixKind::Suggestion, ctx);
                ctx.diagnostic_with_suggestions(
                    no_input_rename_diagnostic(span),
                    [
                        fixer
                            .replace(span, format!("{quote}{property_name}{quote}"))
                            .with_message(REMOVE_ALIAS),
                        fixer
                            .replace(span, format!("{quote}{alias}{quote}"))
                            .with_message(USE_ALIAS),
                    ],
                );
            }
        }
    }

    fn is_allowed_name(&self, alias: &str, property_name: &str) -> bool {
        self.0.allowed_names.iter().any(|name| name.as_str() == alias)
            || (ARIA_ATTRIBUTES.contains(&alias)
                && alias.strip_prefix("aria-").is_some_and(|rest| {
                    property_name.strip_prefix("aria") == Some(capitalize(rest).as_str())
                }))
    }
}

// `'alias'` in `@Input('alias')` when `positional`, and the value of `{ alias: 'alias' }`.
fn for_each_alias<'a>(
    call: &'a CallExpression<'a>,
    positional: bool,
    check: &mut impl FnMut(&'a Expression<'a>, Option<AliasProperty<'a>>),
) {
    for argument in &call.arguments {
        if let Argument::ObjectExpression(object) = argument {
            for property in &object.properties {
                if let ObjectPropertyKind::ObjectProperty(property) = property
                    && property.key.is_specific_id("alias")
                {
                    check(&property.value, Some((&**property, &**object)));
                }
            }
        } else if positional && let Some(expr) = argument.as_expression() {
            check(expr, None);
        }
    }
}

// An `alias:` property and the object that holds it.
type AliasProperty<'a> = (&'a ObjectProperty<'a>, &'a ObjectExpression<'a>);

// Templates with substitutions have no static name to compare or rewrite.
fn static_text<'a>(expr: &Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::TemplateLiteral(template) if !template.expressions.is_empty() => None,
        _ => literal_text(expr),
    }
}

// A positional alias goes alone. An `alias:` property takes its whole object (and the comma
// before it) when it is the only property, otherwise one adjacent comma.
fn alias_removal_span(
    source: &str,
    literal: Span,
    alias_property: Option<AliasProperty<'_>>,
) -> Span {
    let Some((property, object)) = alias_property else { return literal };
    if object.properties.len() == 1 {
        let start = comma_before(source, object.span.start).unwrap_or(object.span.start);
        return Span::new(start, object.span.end);
    }
    let next = object
        .properties
        .iter()
        .map(GetSpan::span)
        .skip_while(|span| *span != property.span)
        .nth(1);
    if let Some(next) = next {
        Span::new(property.span.start, next.start)
    } else {
        let start = comma_before(source, property.span.start).unwrap_or(property.span.start);
        Span::new(start, property.span.end)
    }
}

fn comma_before(source: &str, offset: u32) -> Option<u32> {
    let before = source[..offset as usize].trim_end().strip_suffix(',')?;
    u32::try_from(before.len()).ok()
}

// Upstream matches the selector text, not the parsed selector: `img[fooDirective]` yields the
// name `imgfooDirective` and the attribute name `fooDirective`.
fn selector_allows_alias<'a>(
    node: &AstNode<'a>,
    ctx: &LintContext<'a>,
    property_name: &str,
    alias: &str,
) -> bool {
    let Some(selector) = nearest_class_declaration(node, ctx)
        .and_then(|class| {
            class_decorator(class, "Component").or_else(|| class_decorator(class, "Directive"))
        })
        .and_then(decorator_metadata)
        .and_then(|metadata| metadata_property(metadata, "selector"))
        .and_then(|property| literal_text(&property.value))
    else {
        return false;
    };
    let attribute_name =
        selector.split_once('[').and_then(|(_, rest)| rest.split_once(']')).map(|(name, _)| name);
    let composed_suffix = capitalize(property_name);
    without_brackets_and_whitespace(selector).split(',').any(|name| {
        name == alias
            || attribute_name == Some(alias)
            || alias.strip_prefix(name) == Some(composed_suffix.as_str())
    })
}

fn without_brackets_and_whitespace(text: &str) -> String {
    text.chars().filter(|c| !matches!(c, '[' | ']') && !c.is_whitespace()).collect()
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

// `hostDirectives: [{ inputs: [...] }]` exposes a host directive's inputs, which is allowed.
fn is_host_directive_entry(node: &AstNode, ctx: &LintContext) -> bool {
    let mut ancestors = ctx.nodes().ancestor_kinds(node.id());
    matches!(
        (ancestors.next(), ancestors.next(), ancestors.next()),
        (
            Some(AstKind::ObjectExpression(_)),
            Some(AstKind::ArrayExpression(_)),
            Some(AstKind::ObjectProperty(property)),
        ) if is_host_directives_key(&property.key)
    )
}

fn is_host_directives_key(key: &PropertyKey) -> bool {
    match key {
        PropertyKey::StaticIdentifier(ident) => ident.name.as_str() == "hostDirectives",
        PropertyKey::Identifier(ident) => ident.name.as_str() == "hostDirectives",
        PropertyKey::StringLiteral(lit) => lit.value.as_str() == "hostDirectives",
        _ => false,
    }
}

const ARIA_ATTRIBUTES: &[&str] = &[
    "aria-activedescendant",
    "aria-atomic",
    "aria-autocomplete",
    "aria-busy",
    "aria-checked",
    "aria-colcount",
    "aria-colindex",
    "aria-colspan",
    "aria-controls",
    "aria-current",
    "aria-describedby",
    "aria-details",
    "aria-disabled",
    "aria-dragged",
    "aria-dropeffect",
    "aria-errormessage",
    "aria-expanded",
    "aria-flowto",
    "aria-haspopup",
    "aria-hidden",
    "aria-invalid",
    "aria-label",
    "aria-labelledby",
    "aria-level",
    "aria-live",
    "aria-modal",
    "aria-multiline",
    "aria-multiselectable",
    "aria-orientation",
    "aria-owns",
    "aria-placeholder",
    "aria-posinset",
    "aria-pressed",
    "aria-readonly",
    "aria-relevant",
    "aria-required",
    "aria-rowcount",
    "aria-rowindex",
    "aria-rowspan",
    "aria-selected",
    "aria-setsize",
    "aria-sort",
    "aria-valuemax",
    "aria-valuemin",
    "aria-valuenow",
    "aria-valuetext",
];

#[test]
fn test() {
    use crate::tester::{ExpectFixTestCase, Tester};

    let pass = vec![
        ("class Test {}", None),
        (
            r"
    @Page({
      inputs: ['play', popstate, `online`, 'obsolete: obsol', 'store: storage'],
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Component()
    class Test {
      change = new EventEmitter();
    }
  ",
            None,
        ),
        (
            r"
    @Directive()
    class Test {
      @Input() buttonChange = new EventEmitter<'change'>();
    }
  ",
            None,
        ),
        (
            r"
    @Directive()
    class Test {
      buttonChange = input(1);
    }
  ",
            None,
        ),
        (
            r"
    @Directive()
    class Test {
      buttonChange = input.required<number>();
    }
  ",
            None,
        ),
        (
            r"
    @Component({
      inputs,
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Directive({
      inputs: [...test],
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Component({
      inputs: func(),
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Directive({
      inputs: [func(), 'a'],
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: 'qx-menuitem',
      hostDirectives: [{
        directive: CdkMenuItem,
        inputs: ['cdkMenuItemDisabled: disabled'],
      }]
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: 'qx-menuitem',
      'hostDirectives': [{
        directive: CdkMenuItem,
        inputs: ['cdkMenuItemDisabled: disabled'],
      }]
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: 'qx-menuitem',
      ['hostDirectives']: [{
        directive: CdkMenuItem,
        inputs: ['cdkMenuItemDisabled: disabled'],
      }]
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Component({})
    class Test {
      @Input() set setter(setter: string) {}
    }
  ",
            None,
        ),
        (
            r"
      @Component({
        inputs: ['foo: aria-wrong']
      })
      class Test {
        @Input('aria-wrong') set setter(setter: string) {}
        func = input(1, { alias: 'aria-wrong' });
        required = input.required<number>({ alias: 'aria-wrong' });
      }
      ",
            Some(serde_json::json!([{"allowedNames":["aria-wrong"]}])),
        ),
        (
            r"
    const change = 'change';
    @Component()
    class Test {
      @Input(change) touchMove: EventEmitter<{ action: 'click' | 'close' }> = new EventEmitter<{ action: 'click' | 'close' }>();
    }
  ",
            None,
        ),
        (
            r"
    const change = 'change';
    @Component()
    class Test {
      touchMove = input(1, { alias: change });
    }
  ",
            None,
        ),
        (
            r"
    const change = 'change';
    @Component()
    class Test {
      touchMove = input.required<number>({ alias: change });
    }
  ",
            None,
        ),
        (
            r"
    const blur = 'blur';
    const click = 'click';
    @Directive()
    class Test {
      @Input(blur) [click]: EventEmitter<Blur>;
    }
  ",
            None,
        ),
        (
            r"
    const blur = 'blur';
    const click = 'click';
    @Directive()
    class Test {
      [click] = input(1, { alias: blur });
    }
  ",
            None,
        ),
        (
            r"
    const blur = 'blur';
    const click = 'click';
    @Directive()
    class Test {
      [click] = input.required<number>({ alias: blur });
    }
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: 'foo[bar]'
    })
    class Test {
      @Input() bar: string;
    }
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: 'foo[bar]'
    })
    class Test {
      bar = input(1);
    }
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: 'foo[bar]'
    })
    class Test {
      bar = input.required<number>();
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      'selector': 'foo',
      'inputs': [`test: foo`]
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Component({
      'selector': 'foo',
      ['inputs']: [`test: foo`]
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Directive({
      'selector': 'foo',
      [`inputs`]: [`test: foo`]
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: '[foo], test',
    })
    class Test {
      @Input('foo') label: string;
    }
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: '[foo], test',
    })
    class Test {
      label = input(1, { alias: 'foo' });
    }
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: '[foo], test',
    })
    class Test {
      label = input.required<number>(1, { alias: 'foo' });
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      @Input('aria-label') ariaLabel: string;
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      ariaLabel = input(1, { alias: 'aria-label' });
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      ariaLabel = input.required<number>('aria-label');
    }
  ",
            None,
        ),
        (
            r"
      @Component({
        inputs: ['foo: allowedName']
      })
      class Test {
        @Input() bar: string;
      }
      ",
            Some(serde_json::json!([{"allowedNames":["allowedName"]}])),
        ),
        (
            r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      @Input('fooMyColor') myColor: string;
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      myColor = input(1, { alias: 'fooMyColor' });
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      myColor = input.required<number>('fooMyColor');
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      @Input foo: Foo;
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      foo = input(1);
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      foo = input.required<number>();
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      @Input('fooDirective') foo: Foo;
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      foo = input(1, { alias: 'fooDirective' });
    }
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'img[fooDirective]'
    })
    class Test {
      foo = input.required<number>({ alias: 'fooDirective' });
    }
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: 'foo'
    })
    class Test {
      @Input({ required: true }) name: string;
    }
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: 'foo'
    })
    class Test {
      @Input({ transform: (val) => val ?? '', required: true }) foo!: string;
    }
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: 'foo'
    })
    class Test {
      foo = input.required<string>({ transform: (val) => val ?? '' });
    }
  ",
            None,
        ),
    ];

    let fail = vec![
        (
            r"
      @Component({
        inputs: ['a: b']
      })
      class Test {}
    ",
            None,
        ),
        (
            r"
      @Directive({
        outputs: ['abort'],
        'inputs': [boundary, `test: copy`, 'check: check'],
      })
      class Test {}
    ",
            Some(serde_json::json!([{"allowedNames":["check","test"]}])),
        ),
        (
            r"
      @Component({
        ['inputs']: ['orientation: orientation'],
      })
      class Test {}
    ",
            None,
        ),
        (
            r"
      @Directive({
        [`inputs`]: ['orientation: orientation'],
      })
      class Test {}
    ",
            None,
        ),
        (
            r"
      @Component()
      class Test {
        @Custom() @Input(`change`) _change = getInput();
      }
    ",
            None,
        ),
        (
            r"
      @Component()
      class Test {
        _change = input(1, { alias: `change` });
      }
    ",
            None,
        ),
        (
            r"
      @Component()
      class Test {
        _change = input.required<number>({ alias: `change` });
      }
    ",
            None,
        ),
        (
            r"
      @Directive()
      class Test {
        @Input('change') change = (this.subject$ as Subject<{blur: boolean}>).pipe();
      }
    ",
            None,
        ),
        (
            r"
      @Directive()
      class Test {
        change = input(1, { alias: 'change' });
      }
    ",
            None,
        ),
        (
            r"
      @Directive()
      class Test {
        change = input.required<number>({ alias: 'change' });
      }
    ",
            None,
        ),
        (
            r"
      @Directive()
      class Test {
        @Input({ alias: 'change' }) change = (this.subject$ as Subject<{blur: boolean}>).pipe();
      }
    ",
            None,
        ),
        (
            r"
      @Component()
      class Test {
        @Input(`devicechange`) set setter(setter: string) {}

        @Input('allowedName') test: string;
      }
    ",
            Some(serde_json::json!([{"allowedNames":["allowedName"]}])),
        ),
        (
            r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        @Input('aria-invalid') ariaBusy: string;
      }
    ",
            None,
        ),
        (
            r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        ariaBusy = input(1, { alias: 'aria-invalid' });
      }
    ",
            None,
        ),
        (
            r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        ariaBusy = input.required<number>({ alias: 'aria-invalid' });
      }
    ",
            None,
        ),
        (
            r"
      @Component({
        selector: 'foo'
      })
      class Test {
        @Input('fooColor') colors: string;
      }
    ",
            None,
        ),
        (
            r"
      @Component({
        selector: 'foo'
      })
      class Test {
        colors = input(1, { alias: 'fooColor' });
      }
    ",
            None,
        ),
        (
            r"
      @Component({
        selector: 'foo'
      })
      class Test {
        colors = input.required<number>({ alias: 'fooColor' });
      }
    ",
            None,
        ),
        (
            r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        @Input('foocolor') color: string;
      }
    ",
            None,
        ),
        (
            r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        color = input(1, { alias: 'foocolor' });
      }
    ",
            None,
        ),
        (
            r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        color = input.required<number>({ alias: 'foocolor' });
      }
    ",
            None,
        ),
        (
            r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        @Input('click') blur = this.getInput();
      }
    ",
            None,
        ),
        (
            r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        blur = input(1, { alias: 'click' });
      }
    ",
            None,
        ),
        (
            r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        blur = input.required<number>({ alias: 'click' });
      }
    ",
            None,
        ),
        (
            r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        @Input('notFooDirective') foo: Foo;
      }
    ",
            None,
        ),
        (
            r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        foo = input({ alias: 'notFooDirective' });
      }
    ",
            None,
        ),
        (
            r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        foo = input.required<number>({ alias: 'notFooDirective' });
      }
    ",
            None,
        ),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { alias: 'test', after: 'it' });
        }
      ",
            None,
        ),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ alias: 'test', after: 'it' });
        }
      ",
            None,
        ),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', alias: 'test', });
        }
      ",
            None,
        ),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', alias: 'test', });
        }
      ",
            None,
        ),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', alias: 'test' });
        }
      ",
            None,
        ),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', alias: 'test' });
        }
      ",
            None,
        ),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', alias: 'test', after: 'it' });
        }
      ",
            None,
        ),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', alias: 'test', after: 'it' });
        }
      ",
            None,
        ),
    ];

    let fix: Vec<ExpectFixTestCase> = vec![
        (
            r"
      @Component({
        inputs: ['a: b']
      })
      class Test {}
    ",
            (
                r"
      @Component({
        inputs: ['a']
      })
      class Test {}
    ",
                r"
      @Component({
        inputs: ['b']
      })
      class Test {}
    ",
            ),
        )
            .into(),
        (
            r"
      @Directive({
        outputs: ['abort'],
        'inputs': [boundary, `test: copy`, 'check: check'],
      })
      class Test {}
    ",
            (
                r"
      @Directive({
        outputs: ['abort'],
        'inputs': [boundary, `test`, 'check: check'],
      })
      class Test {}
    ",
                r"
      @Directive({
        outputs: ['abort'],
        'inputs': [boundary, `copy`, 'check: check'],
      })
      class Test {}
    ",
            ),
            Some(serde_json::json!([{"allowedNames":["check","test"]}])),
        )
            .into(),
        (
            r"
      @Component({
        ['inputs']: ['orientation: orientation'],
      })
      class Test {}
    ",
            r"
      @Component({
        ['inputs']: ['orientation'],
      })
      class Test {}
    ",
        )
            .into(),
        (
            r"
      @Directive({
        [`inputs`]: ['orientation: orientation'],
      })
      class Test {}
    ",
            r"
      @Directive({
        [`inputs`]: ['orientation'],
      })
      class Test {}
    ",
        )
            .into(),
        (
            r"
      @Component()
      class Test {
        @Custom() @Input(`change`) _change = getInput();
      }
    ",
            (
                r"
      @Component()
      class Test {
        @Custom() @Input() _change = getInput();
      }
    ",
                r"
      @Component()
      class Test {
        @Custom() @Input() change = getInput();
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Component()
      class Test {
        _change = input(1, { alias: `change` });
      }
    ",
            (
                r"
      @Component()
      class Test {
        _change = input(1);
      }
    ",
                r"
      @Component()
      class Test {
        change = input(1);
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Component()
      class Test {
        _change = input.required<number>({ alias: `change` });
      }
    ",
            (
                r"
      @Component()
      class Test {
        _change = input.required<number>();
      }
    ",
                r"
      @Component()
      class Test {
        change = input.required<number>();
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Directive()
      class Test {
        @Input('change') change = (this.subject$ as Subject<{blur: boolean}>).pipe();
      }
    ",
            r"
      @Directive()
      class Test {
        @Input() change = (this.subject$ as Subject<{blur: boolean}>).pipe();
      }
    ",
        )
            .into(),
        (
            r"
      @Directive()
      class Test {
        change = input(1, { alias: 'change' });
      }
    ",
            r"
      @Directive()
      class Test {
        change = input(1);
      }
    ",
        )
            .into(),
        (
            r"
      @Directive()
      class Test {
        change = input.required<number>({ alias: 'change' });
      }
    ",
            r"
      @Directive()
      class Test {
        change = input.required<number>();
      }
    ",
        )
            .into(),
        (
            r"
      @Directive()
      class Test {
        @Input({ alias: 'change' }) change = (this.subject$ as Subject<{blur: boolean}>).pipe();
      }
    ",
            r"
      @Directive()
      class Test {
        @Input() change = (this.subject$ as Subject<{blur: boolean}>).pipe();
      }
    ",
        )
            .into(),
        (
            r"
      @Component()
      class Test {
        @Input(`devicechange`) set setter(setter: string) {}

        @Input('allowedName') test: string;
      }
    ",
            (
                r"
      @Component()
      class Test {
        @Input() set setter(setter: string) {}

        @Input('allowedName') test: string;
      }
    ",
                r"
      @Component()
      class Test {
        @Input() set devicechange(setter: string) {}

        @Input('allowedName') test: string;
      }
    ",
            ),
            Some(serde_json::json!([{"allowedNames":["allowedName"]}])),
        )
            .into(),
        (
            r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        @Input('aria-invalid') ariaBusy: string;
      }
    ",
            (
                r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        @Input() ariaBusy: string;
      }
    ",
                r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        @Input() 'aria-invalid': string;
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        ariaBusy = input(1, { alias: 'aria-invalid' });
      }
    ",
            (
                r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        ariaBusy = input(1);
      }
    ",
                r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        'aria-invalid' = input(1);
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        ariaBusy = input.required<number>({ alias: 'aria-invalid' });
      }
    ",
            (
                r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        ariaBusy = input.required<number>();
      }
    ",
                r"
      @Directive({
        selector: 'foo'
      })
      class Test {
        'aria-invalid' = input.required<number>();
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Component({
        selector: 'foo'
      })
      class Test {
        @Input('fooColor') colors: string;
      }
    ",
            (
                r"
      @Component({
        selector: 'foo'
      })
      class Test {
        @Input() colors: string;
      }
    ",
                r"
      @Component({
        selector: 'foo'
      })
      class Test {
        @Input() fooColor: string;
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Component({
        selector: 'foo'
      })
      class Test {
        colors = input(1, { alias: 'fooColor' });
      }
    ",
            (
                r"
      @Component({
        selector: 'foo'
      })
      class Test {
        colors = input(1);
      }
    ",
                r"
      @Component({
        selector: 'foo'
      })
      class Test {
        fooColor = input(1);
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Component({
        selector: 'foo'
      })
      class Test {
        colors = input.required<number>({ alias: 'fooColor' });
      }
    ",
            (
                r"
      @Component({
        selector: 'foo'
      })
      class Test {
        colors = input.required<number>();
      }
    ",
                r"
      @Component({
        selector: 'foo'
      })
      class Test {
        fooColor = input.required<number>();
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        @Input('foocolor') color: string;
      }
    ",
            (
                r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        @Input() color: string;
      }
    ",
                r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        @Input() foocolor: string;
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        color = input(1, { alias: 'foocolor' });
      }
    ",
            (
                r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        color = input(1);
      }
    ",
                r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        foocolor = input(1);
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        color = input.required<number>({ alias: 'foocolor' });
      }
    ",
            (
                r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        color = input.required<number>();
      }
    ",
                r"
      @Directive({
        'selector': 'foo'
      })
      class Test {
        foocolor = input.required<number>();
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        @Input('click') blur = this.getInput();
      }
    ",
            (
                r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        @Input() blur = this.getInput();
      }
    ",
                r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        @Input() click = this.getInput();
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        blur = input(1, { alias: 'click' });
      }
    ",
            (
                r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        blur = input(1);
      }
    ",
                r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        click = input(1);
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        blur = input.required<number>({ alias: 'click' });
      }
    ",
            (
                r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        blur = input.required<number>();
      }
    ",
                r"
      @Component({
        selector: 'click',
      })
      class TestComponent {}

      @Injectable()
      class Test {
        click = input.required<number>();
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        @Input('notFooDirective') foo: Foo;
      }
    ",
            (
                r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        @Input() foo: Foo;
      }
    ",
                r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        @Input() notFooDirective: Foo;
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        foo = input({ alias: 'notFooDirective' });
      }
    ",
            (
                r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        foo = input();
      }
    ",
                r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        notFooDirective = input();
      }
    ",
            ),
        )
            .into(),
        (
            r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        foo = input.required<number>({ alias: 'notFooDirective' });
      }
    ",
            (
                r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        foo = input.required<number>();
      }
    ",
                r"
      @Directive({
        selector: 'img[fooDirective]',
      })
      class Test {
        notFooDirective = input.required<number>();
      }
    ",
            ),
        )
            .into(),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { alias: 'test', after: 'it' });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { after: 'it' });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = input(1, { after: 'it' });
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ alias: 'test', after: 'it' });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ after: 'it' });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = input.required<number>({ after: 'it' });
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', alias: 'test', });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = input(1, { before: 'it', });
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', alias: 'test', });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = input.required<number>({ before: 'it', });
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', alias: 'test' });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it' });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = input(1, { before: 'it' });
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', alias: 'test' });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it' });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = input.required<number>({ before: 'it' });
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', alias: 'test', after: 'it' });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input(1, { before: 'it', after: 'it' });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = input(1, { before: 'it', after: 'it' });
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', alias: 'test', after: 'it' });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = input.required<number>({ before: 'it', after: 'it' });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = input.required<number>({ before: 'it', after: 'it' });
        }
      ",
            ),
        )
            .into(),
    ];

    Tester::new(NoInputRename::NAME, NoInputRename::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
