use oxc_ast::{
    AstKind,
    ast::{
        Argument, CallExpression, Decorator, Expression, MethodDefinition, MethodDefinitionKind,
        MethodDefinitionType, ObjectProperty, PropertyDefinition, PropertyDefinitionType,
        PropertyKey,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{decorator_call_name, enclosing_class_decorator, literal_text, metadata_property},
};

fn no_output_on_prefix_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Output bindings, including aliases, should not be named \"on\", nor prefixed with it",
    )
    .with_help("See https://angular.dev/guide/components/outputs#choosing-event-names")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoOutputOnPrefix;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows output bindings, including aliases, named `on` or prefixed with `on` followed
    /// by a character that is not a lowercase letter, such as `onChange` or `on-change`.
    ///
    /// ### Why is this bad?
    ///
    /// Angular binds events with `(event)` or `on-event`, so an `onChange` output reads as
    /// `(onChange)` or the redundant `on-onChange` in templates.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({})
    /// class Test {
    ///   @Output() onChange = new EventEmitter<string>();
    ///   onClose = output();
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class Test {
    ///   @Output() change = new EventEmitter<string>();
    ///   close = output();
    /// }
    /// ```
    NoOutputOnPrefix,
    angular,
    style,
    version = "next",
    short_description = "Disallow output bindings named or prefixed with `on`.",
);

impl Rule for NoOutputOnPrefix {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::ObjectProperty(property) => check_outputs_metadata(node, property, ctx),
            AstKind::PropertyDefinition(property) => check_output_property(property, ctx),
            AstKind::MethodDefinition(method) => check_output_getter(method, ctx),
            _ => {}
        }
    }
}

fn check_name(span: Span, text: &str, ctx: &LintContext) {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    // `'name: alias'` in the `outputs` metadata; other names have no colon.
    let mut names = text.split(':');
    if names.next().is_some_and(has_on_prefix) || names.next().is_some_and(has_on_prefix) {
        ctx.diagnostic(no_output_on_prefix_diagnostic(span));
    }
}

fn has_on_prefix(name: &str) -> bool {
    name.strip_prefix("on").is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_lowercase()))
}

fn check_outputs_metadata<'a>(
    node: &AstNode<'a>,
    property: &ObjectProperty<'a>,
    ctx: &LintContext<'a>,
) {
    let Expression::ArrayExpression(array) = &property.value else { return };
    if !property.key.is_specific_static_name("outputs") {
        return;
    }
    let Some(decorator) = enclosing_class_decorator(node, ctx) else { return };
    if !matches!(decorator_call_name(decorator), Some("Component" | "Directive")) {
        return;
    }
    for element in &array.elements {
        if let Some(text) = element.as_expression().and_then(literal_text) {
            check_name(element.span(), text, ctx);
        }
    }
}

// The name and alias of an `@Output()` property or an `output()` field.
fn check_output_property(property: &PropertyDefinition, ctx: &LintContext) {
    if property.r#type != PropertyDefinitionType::PropertyDefinition {
        return;
    }
    let decorator = output_decorator(&property.decorators);
    let call = match &property.value {
        Some(Expression::CallExpression(call)) if call.callee.is_specific_id("output") => {
            Some(call)
        }
        _ => None,
    };
    if let Some(decorator) = decorator {
        check_decorator_aliases(decorator, ctx);
    }
    if !property.computed
        && (decorator.is_some() || call.is_some())
        && let Some(name) = key_name(&property.key)
    {
        check_name(property.key.span(), name, ctx);
    }
    if let Some(call) = call {
        check_object_aliases(call, ctx);
    }
}

fn check_output_getter(method: &MethodDefinition, ctx: &LintContext) {
    if method.kind != MethodDefinitionKind::Get
        || method.r#type != MethodDefinitionType::MethodDefinition
    {
        return;
    }
    let Some(decorator) = output_decorator(&method.decorators) else { return };
    check_decorator_aliases(decorator, ctx);
    if !method.computed
        && let Some(name) = key_name(&method.key)
    {
        check_name(method.key.span(), name, ctx);
    }
}

fn output_decorator<'b, 'a>(decorators: &'b [Decorator<'a>]) -> Option<&'b Decorator<'a>> {
    decorators.iter().find(|decorator| decorator_call_name(decorator) == Some("Output"))
}

fn key_name<'a>(key: &PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some(ident.name.as_str()),
        PropertyKey::StringLiteral(lit) => Some(lit.value.as_str()),
        _ => None,
    }
}

// `@Output('alias')` and `@Output({ alias: 'alias' })`.
fn check_decorator_aliases(decorator: &Decorator, ctx: &LintContext) {
    let Expression::CallExpression(call) = &decorator.expression else { return };
    for argument in &call.arguments {
        if let Some(text) = argument.as_expression().and_then(literal_text) {
            check_name(argument.span(), text, ctx);
        }
    }
    check_object_aliases(call, ctx);
}

// `{ alias: 'alias' }` passed as an argument.
fn check_object_aliases(call: &CallExpression, ctx: &LintContext) {
    for argument in &call.arguments {
        let Argument::ObjectExpression(object) = argument else { continue };
        if let Some(alias) = metadata_property(object, "alias")
            && let Some(text) = literal_text(&alias.value)
        {
            check_name(alias.value.span(), text, ctx);
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "class Test {}",
        r"
    @Page({
      outputs: ['on', onChange, `onLine`, 'on: on2', 'offline: on', ...onCheck, onOutput()],
    })
    class Test {}
    ",
        r"
    @Component()
    class Test {
      on = new EventEmitter();
    }
    ",
        r"
    @Directive()
    class Test {
      @Output() buttonChange = new EventEmitter<'on'>();
    }
    ",
        r"
    @Directive()
    class Test {
      buttonChange = output<'on'>();
    }
    ",
        r"
    @Component()
    class Test {
      @Output() On = new EventEmitter<{ on: onType }>();
    }
    ",
        r"
    @Component()
    class Test {
      On = output<{ on: onType }>();
    }
    ",
        r"
    @Directive()
    class Test {
      @Output(`one`) ontype = new EventEmitter<{ bar: string, on: boolean }>();
    }
    ",
        r"
    @Directive()
    class Test {
      ontype = output<{ bar: string, on: boolean }>({ alias: `one` });
    }
    ",
        r"
    @Component()
    class Test {
      @Output('oneProp') common = new EventEmitter<ComplextOn>();
    }
    ",
        r"
    @Component()
    class Test {
      common = output<ComplextOn>({ alias: 'oneProp' });
    }
    ",
        r"
    @Directive()
    class Test<On> {
      @Output() ON = new EventEmitter<On>();
    }
    ",
        r"
    @Directive()
    class Test<On> {
      ON = output<On>();
    }
    ",
        r"
    const on = 'on';
    @Component()
    class Test {
      @Output(on) touchMove: EventEmitter<{ action: 'on' | 'off' }> = new EventEmitter<{ action: 'on' | 'off' }>();
    }
    ",
        r"
    const on = 'on';
    @Component()
    class Test {
      touchMove = output<{ action: 'on' | 'off' }>({ alias: on });
    }
    ",
        r"
    const test = 'on';
    const on = 'on';
    @Directive()
    class Test {
      @Output(test) [on]: EventEmitter<OnTest>;
    }
    ",
        r"
    const test = 'on';
    const on = 'on';
    @Directive()
    class Test {
      [on] = output<OnTest>({ alias: test });
    }
    ",
        r"
    @Component({
      selector: 'foo',
      'outputs': [`test: foo`]
    })
    class Test {}
    ",
        r"
    @Directive({
      selector: 'foo',
      ['outputs']: [`test: foo`]
    })
    class Test {}
    ",
        r"
    @Component({
      'selector': 'foo',
      [`outputs`]: [`test: foo`]
    })
    class Test {}
    ",
        r"
    @Directive({
      selector: 'foo',
    })
    class Test {
      @Output() get 'getter'() {}
    }
    ",
    ];

    let fail = vec![
        r"
        @Component({
          outputs: ['on']
        })
        class Test {}
      ",
        r"
        @Directive({
          inputs: [onCredit],
          'outputs': [onLevel, `test: on`, onFunction()],
        })
        class Test {}
      ",
        r"
        @Component({
          ['outputs']: ['onTest: test', ...onArray],
        })
        class Test {}
      ",
        r"
        @Directive({
          [`outputs`]: ['onTest: test', ...onArray],
        })
        class Test {}
      ",
        r"
        @Component()
        class Test {
          @Output() on: EventEmitter<any> = new EventEmitter<{}>();
        }
      ",
        r"
        @Component()
        class Test {
          on = output();
        }
      ",
        r"
        @Directive()
        class Test {
          @Output() @Custom('on') 'onPrefix' = new EventEmitter<void>();
        }
      ",
        r"
        @Directive()
        class Test {
          'onPrefix' = output();
        }
      ",
        r"
        @Component()
        class Test {
          @Custom() @Output(`on`) _on = getOutput();
        }
      ",
        r"
        @Component()
        class Test {
          _on = output({ alias: `on` });
        }
      ",
        r"
        @Directive()
        class Test {
          @Output('onPrefix') _on = (this.subject$ as Subject<{on: boolean}>).pipe();
        }
      ",
        r"
        @Directive()
        class Test {
          _on = output({ alias: 'onPrefix' });
        }
      ",
        r"
        @Component()
        class Test {
          @Output('getter') get 'on-getter'() {}
        }
      ",
        r"
        @Directive()
        class Test {
          @Output(`onGetter`) get getter() {}
        }
      ",
        r"
        @Injectable()
        class Test {
          @Output('on') onPrefix = this.getOutput();
        }
      ",
        r"
        @Injectable()
        class Test {
          onPrefix = output({ alias: 'on' });
        }
      ",
    ];

    Tester::new(NoOutputOnPrefix::NAME, NoOutputOnPrefix::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
