use oxc_ast::{
    AstKind,
    ast::{
        Decorator, Expression, MethodDefinitionKind, MethodDefinitionType, PropertyDefinitionType,
        PropertyKey,
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
    context::{ContextHost, LintContext},
    rule::{DefaultRuleConfig, Rule},
    utils::{
        decorator_call_name, decorator_metadata, enclosing_class_decorator, literal_text,
        metadata_property, to_human_readable_text,
    },
};

fn no_input_prefix_diagnostic(span: Span, prefixes: &[CompactStr]) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Input bindings, including aliases, should not be named, nor prefixed by {}",
        to_human_readable_text(prefixes)
    ))
    .with_help("Name inputs like HTML attributes, e.g. `disabled` rather than `isDisabled`.")
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct NoInputPrefix(Box<NoInputPrefixConfig>);

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct NoInputPrefixConfig {
    /// Prefixes that input names and aliases must not use, such as `on` or `is`.
    prefixes: Vec<CompactStr>,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows input bindings, including aliases, that are named or prefixed by one of the
    /// configured `prefixes`. A name is prefixed when the prefix is followed by a character that
    /// is not a lowercase letter, so `isDisabled` has the `is` prefix but `issue` does not.
    /// No prefix is disallowed until `prefixes` is configured.
    ///
    /// ### Why is this bad?
    ///
    /// HTML attributes do not use prefixes like `is` or `has`, so `[disabled]="true"` reads more
    /// naturally in a template than `[isDisabled]="true"`.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule with `{ "prefixes": ["is"] }`:
    /// ```ts
    /// @Component({})
    /// class Test {
    ///   @Input() isDisabled: boolean;
    ///   @Input('isOpen') open: boolean;
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule with `{ "prefixes": ["is"] }`:
    /// ```ts
    /// @Component({})
    /// class Test {
    ///   @Input() disabled: boolean;
    ///   @Input() issue: string;
    /// }
    /// ```
    NoInputPrefix,
    angular,
    restriction,
    config = NoInputPrefixConfig,
    version = "next",
    short_description = "Disallow input bindings named or prefixed by the configured prefixes.",
);

impl Rule for NoInputPrefix {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn should_run(&self, _ctx: &ContextHost) -> bool {
        !self.0.prefixes.is_empty()
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::ObjectProperty(property) => {
                let Expression::ArrayExpression(array) = &property.value else { return };
                if !property.key.is_specific_static_name("inputs") {
                    return;
                }
                let Some(decorator) = enclosing_class_decorator(node, ctx) else { return };
                if !matches!(decorator_call_name(decorator), Some("Component" | "Directive")) {
                    return;
                }
                for element in &array.elements {
                    let Some(text) = element.as_expression().and_then(literal_text) else {
                        continue;
                    };
                    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                    // `'name: alias'`
                    let mut names = text.split(':');
                    if names.next().is_some_and(|name| self.is_prefixed(name))
                        || names.next().is_some_and(|alias| self.is_prefixed(alias))
                    {
                        self.report(element.span(), ctx);
                    }
                }
            }
            AstKind::PropertyDefinition(property)
                if property.r#type == PropertyDefinitionType::PropertyDefinition
                    && !property.computed =>
            {
                self.check_member(&property.decorators, &property.key, ctx);
            }
            AstKind::MethodDefinition(method)
                if method.kind == MethodDefinitionKind::Set
                    && method.r#type == MethodDefinitionType::MethodDefinition
                    && !method.computed =>
            {
                self.check_member(&method.decorators, &method.key, ctx);
            }
            _ => {}
        }
    }
}

impl NoInputPrefix {
    fn check_member(&self, decorators: &[Decorator], key: &PropertyKey, ctx: &LintContext) {
        let Some(input) =
            decorators.iter().find(|decorator| decorator_call_name(decorator) == Some("Input"))
        else {
            return;
        };
        let Some(name) = key_name(key) else { return };
        if self.is_prefixed(name) {
            self.report(key.span(), ctx);
        }
        if let Some(alias) = decorator_alias(input)
            && literal_text(alias).is_some_and(|alias_name| self.is_prefixed(alias_name))
        {
            self.report(alias.span(), ctx);
        }
    }

    fn is_prefixed(&self, name: &str) -> bool {
        self.0.prefixes.iter().any(|prefix| {
            name.strip_prefix(prefix.as_str())
                .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_lowercase()))
        })
    }

    fn report(&self, span: Span, ctx: &LintContext) {
        ctx.diagnostic(no_input_prefix_diagnostic(span, &self.0.prefixes));
    }
}

fn key_name<'a>(key: &PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some(ident.name.as_str()),
        PropertyKey::StringLiteral(lit) => Some(lit.value.as_str()),
        _ => None,
    }
}

// `@Input({ alias: 'name' })`, otherwise `@Input('name')`.
fn decorator_alias<'b, 'a>(decorator: &'b Decorator<'a>) -> Option<&'b Expression<'a>> {
    if let Some(alias) = decorator_metadata(decorator).and_then(|m| metadata_property(m, "alias")) {
        return Some(&alias.value);
    }
    let Expression::CallExpression(call) = &decorator.expression else { return None };
    call.arguments.first()?.as_expression()
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        ("class Test {}", Some(serde_json::json!([{"prefixes":["on"]}]))),
        (
            r"
      @Page({
        inputs: ['on', onChange, `onLine`, 'on: on2', 'offline: on', ...onCheck, onInput()],
      })
      class Test {}
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      @Component()
      class Test {
        on = new EventEmitter();
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      @Directive()
      class Test {
        @Input() buttonChange = new EventEmitter<'on'>();
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      @Component()
      class Test {
        @Input() On = new EventEmitter<{ on: onType }>();
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      @Directive()
      class Test {
        @Input(`one`) ontype = new EventEmitter<{ bar: string, on: boolean }>();
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      @Directive()
      class Test {
        @Input({ alias: `one` }) ontype = new EventEmitter<{ bar: string, on: boolean }>();
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      @Component()
      class Test {
        @Input('oneProp') common = new EventEmitter<ComplextOn>();
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      @Component()
      class Test {
        @Input({ alias: 'oneProp' }) common = new EventEmitter<ComplextOn>();
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      @Directive()
      class Test<On> {
        @Input() ON = new EventEmitter<On>();
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      const on = 'on';
      @Component()
      class Test {
        @Input(on) touchMove: EventEmitter<{ action: 'on' | 'off' }> = new EventEmitter<{ action: 'on' | 'off' }>();
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      const test = 'on';
      const on = 'on';
      @Directive()
      class Test {
        @Input(test) [on]: EventEmitter<OnTest>;
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      @Component()
      class Test {
        @Input() notOn: string = 'on';
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
    @Component({
      selector: 'foo',
      'inputs': [`test: foo`]
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'foo',
      ['inputs']: [`test: foo`]
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Component({
      selector: 'foo',
      [`inputs`]: [`test: foo`]
    })
    class Test {}
  ",
            None,
        ),
        (
            r"
    @Directive({
      selector: 'foo',
    })
    class Test {
      @Input() set 'setter'(value) {}
    }
  ",
            None,
        ),
    ];

    let fail = vec![
        (
            r"
      @Component({
        inputs: ['on']
      })
      class Test {}
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
      @Directive({
        outputs: [onCredit],
        'inputs': [onLevel, `test: on`, onFunction()],
      })
      class Test {}
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Component({
          ['inputs']: ['onTest: test', ...onArray],
        })
        class Test {}
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Directive({
          [`inputs`]: ['onTest: test', ...onArray],
        })
        class Test {}
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Component()
        class Test {
          @Input() on: EventEmitter<any> = new EventEmitter<{}>();
        }
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Directive() 
        class Test {
          @Input() @Custom('on') 'onPrefix' = new EventEmitter<void>();
        }
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Component()
        class Test {
          @Custom() @Input(`on`) _on = getInput();
        }
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Component()
        class Test {
          @Custom() @Input({ required: true, alias: `on` }) _on = getInput();
        }
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Directive()
        class Test {
          @Input('onPrefix') _on = (this.subject$ as Subject<{on: boolean}>).pipe();
        }
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Directive()
        class Test {
          @Input({ alias: 'onPrefix', required: true }) _on = (this.subject$ as Subject<{on: boolean}>).pipe();
        }
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Component()
        class Test {
          @Input('setter') set 'on-setter'(value) {}
        }
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Directive()
        class Test {
          @Input(`onSetter`) set setter(value) {}
        }
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Injectable()
        class Test {
          @Input('on') isPrefix = this.getInput();
        }
      ",
            Some(serde_json::json!([{"prefixes":["on","is","should"]}])),
        ),
        (
            r"
      @Component()
      class Test {
        @Input() on: string = 'on';
      }
    ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
        (
            r"
        @Injectable()
        class Test {
          @Input('on') isPrefix = `on`;
        }
      ",
            Some(serde_json::json!([{"prefixes":["on"]}])),
        ),
    ];

    Tester::new(NoInputPrefix::NAME, NoInputPrefix::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
