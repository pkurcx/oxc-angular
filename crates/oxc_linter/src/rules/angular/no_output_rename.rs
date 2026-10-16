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

use crate::{
    AstNode,
    context::LintContext,
    fixer::RuleFixer,
    rule::Rule,
    utils::{
        class_decorator, decorator_call_name, decorator_metadata, enclosing_class_decorator,
        literal_text, metadata_property, nearest_class_declaration,
    },
};

fn no_output_rename_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Output bindings should not be aliased")
        .with_help("See https://angular.dev/guide/components/outputs#choosing-event-names")
        .with_label(span)
}

const REMOVE_ALIAS: &str = "Remove alias name";
const USE_ALIAS: &str = "Remove alias name and use it as the original name";

#[derive(Debug, Default, Clone)]
pub struct NoOutputRename;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows aliasing output bindings with `@Output('alias')`, `output({ alias })` or
    /// `'name: alias'` in the `outputs` metadata.
    ///
    /// An alias is allowed when the enclosing `@Component` or `@Directive` selector makes it the
    /// natural name: the selector itself, or the selector followed by the capitalized property
    /// name. The `outputs` of a `hostDirectives` entry are not checked.
    ///
    /// ### Why is this bad?
    ///
    /// An aliased output has one name in the class and another in templates, which makes the
    /// component harder to read and refactor.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({ selector: 'app-user' })
    /// class UserComponent {
    ///   @Output('userDeleted') deleted = new EventEmitter<void>();
    ///   saved = output({ alias: 'userSaved' });
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({ selector: 'app-user' })
    /// class UserComponent {
    ///   @Output() deleted = new EventEmitter<void>();
    ///   saved = output();
    /// }
    /// ```
    NoOutputRename,
    angular,
    style,
    fix_suggestion,
    version = "next",
    short_description = "Disallow aliasing output bindings.",
);

impl Rule for NoOutputRename {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::ObjectProperty(property) => check_outputs_metadata(node, property, ctx),
            AstKind::PropertyDefinition(property)
                if property.r#type == PropertyDefinitionType::PropertyDefinition =>
            {
                let signal = match &property.value {
                    Some(Expression::CallExpression(call))
                        if call.callee.is_specific_id("output") =>
                    {
                        Some(&**call)
                    }
                    _ => None,
                };
                check_member(node, &property.key, &property.decorators, signal, ctx);
            }
            AstKind::MethodDefinition(method)
                if method.kind == MethodDefinitionKind::Get
                    && method.r#type == MethodDefinitionType::MethodDefinition =>
            {
                check_member(node, &method.key, &method.decorators, None, ctx);
            }
            _ => {}
        }
    }
}

fn check_member<'a>(
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
        let removal = alias_removal_span(ctx.source_text(), literal.span(), alias_property);
        if alias == property_name {
            ctx.diagnostic_with_fix(no_output_rename_diagnostic(literal.span()), |fixer| {
                fixer.delete_range(removal)
            });
        } else if !selector_allows_alias(node, ctx, property_name, alias) {
            let fixer = RuleFixer::new(FixKind::Suggestion, ctx);
            let new_key =
                if alias.contains('-') { format!("'{alias}'") } else { alias.to_string() };
            ctx.diagnostic_with_suggestions(
                no_output_rename_diagnostic(literal.span()),
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
        decorators.iter().find(|decorator| decorator_call_name(decorator) == Some("Output"))
        && let Expression::CallExpression(call) = &decorator.expression
    {
        for_each_alias(call, true, &mut check);
    }
    if let Some(call) = signal {
        for_each_alias(call, false, &mut check);
    }
}

fn check_outputs_metadata<'a>(
    node: &AstNode<'a>,
    property: &ObjectProperty<'a>,
    ctx: &LintContext<'a>,
) {
    let Expression::ArrayExpression(array) = &property.value else { return };
    if !property.key.is_specific_static_name("outputs") || is_host_directive_entry(node, ctx) {
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
        let span = literal.span();
        let quote = if matches!(literal, Expression::StringLiteral(_)) { '\'' } else { '`' };
        if alias == property_name {
            ctx.diagnostic_with_fix(no_output_rename_diagnostic(span), |fixer| {
                fixer.replace(span, format!("{quote}{property_name}{quote}"))
            });
        } else if !selector_allows_alias(node, ctx, property_name, alias) {
            let fixer = RuleFixer::new(FixKind::Suggestion, ctx);
            ctx.diagnostic_with_suggestions(
                no_output_rename_diagnostic(span),
                [
                    fixer
                        .replace(span, format!("{quote}{property_name}{quote}"))
                        .with_message(REMOVE_ALIAS),
                    fixer.replace(span, format!("{quote}{alias}{quote}")).with_message(USE_ALIAS),
                ],
            );
        }
    }
}

// `'alias'` in `@Output('alias')` when `positional`, and the value of `{ alias: 'alias' }`.
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

// Upstream matches the selector text, not the parsed selector: `img[foo]` yields `imgfoo`.
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
    let composed_suffix = capitalize(property_name);
    without_brackets_and_whitespace(selector)
        .split(',')
        .any(|name| name == alias || alias.strip_prefix(name) == Some(composed_suffix.as_str()))
}

fn without_brackets_and_whitespace(text: &str) -> String {
    text.chars().filter(|c| !matches!(c, '[' | ']') && !c.is_whitespace()).collect()
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

// `hostDirectives: [{ outputs: [...] }]` exposes a host directive's outputs, which is allowed.
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

#[test]
fn test() {
    use crate::tester::{ExpectFixTestCase, Tester};

    let pass = vec![
        "class Test {}",
        r"
    @Page({
      outputs: ['play', popstate, `online`, 'obsolete: obsol', 'store: storage'],
    })
    class Test {}
    ",
        r"
    @Component()
    class Test {
      change = new EventEmitter();
    }
    ",
        r"
    @Directive()
    class Test {
      @Output() buttonChange = new EventEmitter<'change'>();
    }
    ",
        r"
    @Directive()
    class Test {
      buttonChange = output<'change'>();
    }
    ",
        r"
    @Component({
      outputs,
    })
    class Test {}
    ",
        r"
    @Directive({
      outputs: [...test],
    })
    class Test {}
    ",
        r"
    @Component({
      outputs: func(),
    })
    class Test {}
    ",
        r"
    @Directive({
      outputs: [func(), 'a'],
    })
    class Test {}
    ",
        r"
    @Component({})
    class Test {
      @Output() get getter() {}
    }
    ",
        r"
    const change = 'change';
    @Component()
    class Test {
      @Output(change) touchMove: EventEmitter<{ action: 'click' | 'close' }> = new EventEmitter<{ action: 'click' | 'close' }>();
    }
    ",
        r"
    const change = 'change';
    @Component()
    class Test {
      touchMove = output<{ action: 'click' | 'close' }>({ alias: change });
    }
    ",
        r"
    const blur = 'blur';
    const click = 'click';
    @Directive()
    class Test {
      @Output(blur) [click]: EventEmitter<Blur>;
    }
    ",
        r"
    const blur = 'blur';
    const click = 'click';
    @Directive()
    class Test {
      [click] = output({ alias: blur });
    }
    ",
        r"
    @Component({
      selector: 'foo[bar]'
    })
    class Test {
      @Output() bar: string;
    }
    ",
        r"
    @Component({
      selector: 'foo[bar]'
    })
    class Test {
      bar = output();
    }
    ",
        r"
    @Directive({
      'selector': 'foo',
      'outputs': [`test: foo`]
    })
    class Test {}
    ",
        r"
    @Component({
      'selector': 'foo',
      ['outputs']: [`test: foo`]
    })
    class Test {}
    ",
        r"
    @Directive({
      'selector': 'foo',
      [`outputs`]: [`test: foo`]
    })
    class Test {}
    ",
        r"
    @Component({
      selector: '[foo], test',
    })
    class Test {
      @Output('foo') label: string;
    }
    ",
        r"
    @Component({
      selector: '[foo], test',
    })
    class Test {
      label = output({ alias: 'foo' });
    }
    ",
        r"
    @Component({
      selector: 'foo',
      hostDirectives: [{
        directive: CdkMenuItem,
        outputs: ['cdkMenuItemTriggered: triggered'],
      }]
    })
    class Test {}
  ",
        r"
    @Component({
      selector: 'foo',
      'hostDirectives': [{
        directive: CdkMenuItem,
        outputs: ['cdkMenuItemTriggered: triggered'],
      }]
    })
    class Test {}
  ",
        r"
    @Component({
      selector: 'foo',
      ['hostDirectives']: [{
        directive: CdkMenuItem,
        outputs: ['cdkMenuItemTriggered: triggered'],
      }]
    })
    class Test {}
  ",
        r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      @Output('fooMyColor') myColor: string;
    }
    ",
        r"
    @Directive({
      selector: 'foo'
    })
    class Test {
      myColor = output({ alias: 'fooMyColor' });
    }
    ",
    ];

    let fail = vec![
        r"
        @Component({
          outputs: ['a: b']
        })
        class Test {}
      ",
        r"
        @Directive({
          inputs: ['abort'],
          'outputs': [boundary, `test: copy`],
        })
        class Test {}
      ",
        r"
        @Component({
          ['outputs']: ['orientation: orientation'],
        })
        class Test {}
      ",
        r"
        @Directive({
          [`outputs`]: ['orientation: orientation'],
        })
        class Test {}
      ",
        r"
        @Component()
        class Test {
          @Custom() @Output(`change`) _change = getOutput();
        }
      ",
        r"
        @Component()
        class Test {
          _change = output({ alias: `change` });
        }
      ",
        r"
        @Directive()
        class Test {
          @Output('change') change = (this.subject$ as Subject<{blur: boolean}>).pipe();
        }
      ",
        r"
        @Directive()
        class Test {
          change = output({ alias: 'change' });
        }
      ",
        r"
        @Component()
        class Test {
          @Output(`devicechange`) get getter() {}

          @Output() test: string;
        }
      ",
        r"
        @Directive({
          selector: 'foo'
        })
        class Test {
          @Output('fooColor') colors: string;
        }
      ",
        r"
        @Directive({
          selector: 'foo'
        })
        class Test {
          colors = output({ alias: 'fooColor' });
        }
      ",
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          @Output('foocolor') color: string;
        }
      ",
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ alias: 'foocolor' });
        }
      ",
        r"
        @Directive({
          selector: 'kebab-case',
        })
        class TestDirective {}

        @Injectable()
        class Test {
          @Output('kebab-case') blur = this.getOutput();
        }
      ",
        r"
        @Directive({
          selector: 'kebab-case',
        })
        class TestDirective {}

        @Injectable()
        class Test {
          blur = output({ alias: 'kebab-case' });
        }
      ",
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ alias: 'test', after: 'it' });
        }
      ",
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ before: 'it', alias: 'test', });
        }
      ",
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ before: 'it', alias: 'test' });
        }
      ",
        r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ before: 'it', alias: 'test', after: 'it' });
        }
      ",
    ];

    let fix: Vec<ExpectFixTestCase> = vec![
        (
            r"
        @Component({
          outputs: ['a: b']
        })
        class Test {}
      ",
            (
                r"
        @Component({
          outputs: ['a']
        })
        class Test {}
      ",
                r"
        @Component({
          outputs: ['b']
        })
        class Test {}
      ",
            ),
        )
            .into(),
        (
            r"
        @Directive({
          inputs: ['abort'],
          'outputs': [boundary, `test: copy`],
        })
        class Test {}
      ",
            (
                r"
        @Directive({
          inputs: ['abort'],
          'outputs': [boundary, `test`],
        })
        class Test {}
      ",
                r"
        @Directive({
          inputs: ['abort'],
          'outputs': [boundary, `copy`],
        })
        class Test {}
      ",
            ),
        )
            .into(),
        (
            r"
        @Component({
          ['outputs']: ['orientation: orientation'],
        })
        class Test {}
      ",
            r"
        @Component({
          ['outputs']: ['orientation'],
        })
        class Test {}
      ",
        )
            .into(),
        (
            r"
        @Directive({
          [`outputs`]: ['orientation: orientation'],
        })
        class Test {}
      ",
            r"
        @Directive({
          [`outputs`]: ['orientation'],
        })
        class Test {}
      ",
        )
            .into(),
        (
            r"
        @Component()
        class Test {
          @Custom() @Output(`change`) _change = getOutput();
        }
      ",
            (
                r"
        @Component()
        class Test {
          @Custom() @Output() _change = getOutput();
        }
      ",
                r"
        @Component()
        class Test {
          @Custom() @Output() change = getOutput();
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Component()
        class Test {
          _change = output({ alias: `change` });
        }
      ",
            (
                r"
        @Component()
        class Test {
          _change = output();
        }
      ",
                r"
        @Component()
        class Test {
          change = output();
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Directive()
        class Test {
          @Output('change') change = (this.subject$ as Subject<{blur: boolean}>).pipe();
        }
      ",
            r"
        @Directive()
        class Test {
          @Output() change = (this.subject$ as Subject<{blur: boolean}>).pipe();
        }
      ",
        )
            .into(),
        (
            r"
        @Directive()
        class Test {
          change = output({ alias: 'change' });
        }
      ",
            r"
        @Directive()
        class Test {
          change = output();
        }
      ",
        )
            .into(),
        (
            r"
        @Component()
        class Test {
          @Output(`devicechange`) get getter() {}

          @Output() test: string;
        }
      ",
            (
                r"
        @Component()
        class Test {
          @Output() get getter() {}

          @Output() test: string;
        }
      ",
                r"
        @Component()
        class Test {
          @Output() get devicechange() {}

          @Output() test: string;
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
          @Output('fooColor') colors: string;
        }
      ",
            (
                r"
        @Directive({
          selector: 'foo'
        })
        class Test {
          @Output() colors: string;
        }
      ",
                r"
        @Directive({
          selector: 'foo'
        })
        class Test {
          @Output() fooColor: string;
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
          colors = output({ alias: 'fooColor' });
        }
      ",
            (
                r"
        @Directive({
          selector: 'foo'
        })
        class Test {
          colors = output();
        }
      ",
                r"
        @Directive({
          selector: 'foo'
        })
        class Test {
          fooColor = output();
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
          @Output('foocolor') color: string;
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          @Output() color: string;
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          @Output() foocolor: string;
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
          color = output({ alias: 'foocolor' });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output();
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          foocolor = output();
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Directive({
          selector: 'kebab-case',
        })
        class TestDirective {}

        @Injectable()
        class Test {
          @Output('kebab-case') blur = this.getOutput();
        }
      ",
            (
                r"
        @Directive({
          selector: 'kebab-case',
        })
        class TestDirective {}

        @Injectable()
        class Test {
          @Output() blur = this.getOutput();
        }
      ",
                r"
        @Directive({
          selector: 'kebab-case',
        })
        class TestDirective {}

        @Injectable()
        class Test {
          @Output() 'kebab-case' = this.getOutput();
        }
      ",
            ),
        )
            .into(),
        (
            r"
        @Directive({
          selector: 'kebab-case',
        })
        class TestDirective {}

        @Injectable()
        class Test {
          blur = output({ alias: 'kebab-case' });
        }
      ",
            (
                r"
        @Directive({
          selector: 'kebab-case',
        })
        class TestDirective {}

        @Injectable()
        class Test {
          blur = output();
        }
      ",
                r"
        @Directive({
          selector: 'kebab-case',
        })
        class TestDirective {}

        @Injectable()
        class Test {
          'kebab-case' = output();
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
          color = output({ alias: 'test', after: 'it' });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ after: 'it' });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = output({ after: 'it' });
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
          color = output({ before: 'it', alias: 'test', });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ before: 'it', });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = output({ before: 'it', });
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
          color = output({ before: 'it', alias: 'test' });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ before: 'it' });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = output({ before: 'it' });
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
          color = output({ before: 'it', alias: 'test', after: 'it' });
        }
      ",
            (
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          color = output({ before: 'it', after: 'it' });
        }
      ",
                r"
        @Component({
          'selector': 'foo'
        })
        class Test {
          test = output({ before: 'it', after: 'it' });
        }
      ",
            ),
        )
            .into(),
    ];

    Tester::new(NoOutputRename::NAME, NoOutputRename::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
