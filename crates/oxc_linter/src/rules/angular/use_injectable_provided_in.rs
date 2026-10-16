use lazy_regex::{Regex, RegexBuilder, regex};
use oxc_ast::{
    AstKind,
    ast::{Argument, Expression, ObjectProperty},
};
use oxc_ast_visit::{Visit, walk};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    fixer::RuleFixer,
    rule::{DefaultRuleConfig, Rule},
    utils::{decorator_call_name, implemented_interface_names},
};

fn use_injectable_provided_in_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The `providedIn` property is mandatory for `Injectables`.")
        .with_help("Set `providedIn` so the service can be tree-shaken.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct UseInjectableProvidedIn(Box<UseInjectableProvidedInConfig>);

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct UseInjectableProvidedInConfig {
    /// Class names to skip. `/.../` is a regular expression (for example `/Effects$/`); any other
    /// string must match the class name exactly.
    #[serde(deserialize_with = "deserialize_class_name_pattern")]
    ignore_class_name_pattern: Option<Regex>,
    /// Accept `providedIn: null`, which Angular treats like a missing `providedIn`.
    allow_provided_in_null: bool,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires `@Injectable()` classes to set the `providedIn` metadata property. Classes that
    /// implement `HttpInterceptor` are skipped.
    ///
    /// ### Why is this bad?
    ///
    /// A service with `providedIn` is tree-shakable: Angular drops it from the bundle when nothing
    /// injects it. Without `providedIn`, the service has to be listed in some `providers` array
    /// and ships even when unused. `providedIn: null` and `providedIn: undefined` behave like a
    /// missing property.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Injectable()
    /// class Test {}
    ///
    /// @Injectable({ providedIn: null })
    /// class Other {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Injectable({ providedIn: 'root' })
    /// class Test {}
    /// ```
    UseInjectableProvidedIn,
    angular,
    pedantic,
    suggestion,
    config = UseInjectableProvidedInConfig,
    version = "next",
    short_description = "Require the `providedIn` property in `@Injectable()`.",
);

const INJECTORS: [&str; 3] = ["any", "platform", "root"];

impl Rule for UseInjectableProvidedIn {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if !class.is_declaration()
            || implemented_interface_names(class).any(|name| name == "HttpInterceptor")
        {
            return;
        }
        if let (Some(pattern), Some(id)) = (&self.0.ignore_class_name_pattern, &class.id)
            && pattern.is_match(id.name.as_str())
        {
            return;
        }

        for decorator in &class.decorators {
            if decorator_call_name(decorator) != Some("Injectable") {
                continue;
            }
            let Expression::CallExpression(call) = &decorator.expression else { continue };

            let mut finder = ProvidedInFinder::default();
            finder.visit_expression(&decorator.expression);

            // `@Injectable(options)` with a non-literal argument is accepted.
            let insertion = match call.arguments.first() {
                None => Some((Span::empty(call.span.end - 1), "{", "}")),
                Some(Argument::ObjectExpression(object))
                    if !finder
                        .properties
                        .iter()
                        .any(|p| object.span.contains_inclusive(p.span)) =>
                {
                    Some(match object.properties.last() {
                        None => (Span::empty(object.span.end - 1), "", ""),
                        Some(last) => (last.span(), ", ", ""),
                    })
                }
                _ => None,
            };
            if let Some((at, before, after)) = insertion {
                let fixer = RuleFixer::new(FixKind::Suggestion, ctx);
                ctx.diagnostic_with_suggestions(
                    use_injectable_provided_in_diagnostic(decorator.span),
                    INJECTORS.map(|injector| {
                        fixer
                            .insert_text_after_range(
                                at,
                                format!("{before}providedIn: '{injector}'{after}"),
                            )
                            .with_message(suggestion_message(injector))
                    }),
                );
            }

            for property in &finder.properties {
                let flagged = match property.value {
                    ProvidedInValue::Undefined => true,
                    ProvidedInValue::Null => !self.0.allow_provided_in_null,
                    ProvidedInValue::Other => false,
                };
                if !flagged {
                    continue;
                }
                let fixer = RuleFixer::new(FixKind::Suggestion, ctx);
                ctx.diagnostic_with_suggestions(
                    use_injectable_provided_in_diagnostic(property.value_span),
                    INJECTORS.map(|injector| {
                        fixer
                            .replace(property.value_span, format!("'{injector}'"))
                            .with_message(suggestion_message(injector))
                    }),
                );
            }
        }
    }
}

fn suggestion_message(injector: &str) -> String {
    format!("Use `providedIn: '{injector}'`")
}

enum ProvidedInValue {
    Undefined,
    Null,
    Other,
}

struct ProvidedInProperty {
    span: Span,
    value_span: Span,
    value: ProvidedInValue,
}

// Collects every `providedIn` property in the decorator, nested ones included, as the upstream
// descendant selectors do.
#[derive(Default)]
struct ProvidedInFinder {
    properties: Vec<ProvidedInProperty>,
}

impl<'a> Visit<'a> for ProvidedInFinder {
    fn visit_object_property(&mut self, property: &ObjectProperty<'a>) {
        if property.key.is_specific_static_name("providedIn") {
            let value = property.value.without_parentheses();
            self.properties.push(ProvidedInProperty {
                span: property.span,
                value_span: value.span(),
                value: match value {
                    Expression::Identifier(ident) if ident.name == "undefined" => {
                        ProvidedInValue::Undefined
                    }
                    Expression::NullLiteral(_) => ProvidedInValue::Null,
                    _ => ProvidedInValue::Other,
                },
            });
        }
        walk::walk_object_property(self, property);
    }
}

// esquery reads `/source/flags` as a regular expression and anything else as an exact name.
fn deserialize_class_name_pattern<'de, D>(deserializer: D) -> Result<Option<Regex>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let pattern = String::deserialize(deserializer)?;
    let compiled = match pattern.strip_prefix('/').and_then(|rest| rest.rsplit_once('/')) {
        Some((source, flags)) => {
            RegexBuilder::new(source).case_insensitive(flags.contains('i')).build()
        }
        None => Regex::new(&format!("^{}$", regex::escape(&pattern))),
    };
    compiled.map(Some).map_err(D::Error::custom)
}

#[test]
fn test() {
    use crate::tester::{ExpectFixTestCase, Tester};

    let pass = vec![
        ("class Test {}", None),
        (
            r"
            const options = {};
            @Injectable(options)
            class Test {}
            ",
            None,
        ),
        (
            r"
            @Injectable({
              providedIn: `any`,
            })
            class Test {}
            ",
            None,
        ),
        (
            r"
            @Injectable({
              'providedIn': 'root',
            })
            class Test {}
            ",
            None,
        ),
        (
            r"
            @Injectable({
              ['providedIn']: SomeModule,
            })
            class Test {}
            ",
            None,
        ),
        (
            r"
            @Injectable({
              [`providedIn`]: providedIn(),
            })
            class Test {}
            ",
            None,
        ),
        (
            r"
            @Injectable()
            class Test implements HttpInterceptor {}
            ",
            None,
        ),
        (
            r"
            @Injectable()
            class Test implements ng.HttpInterceptor {}
            ",
            None,
        ),
        (
            r"
            @Injectable()
            class TestEffects {}
            ",
            Some(serde_json::json!([{ "ignoreClassNamePattern": "/Effects$/" }])),
        ),
        (
            r"
            @CustomInjectable()
            class Test {}
            ",
            None,
        ),
        (
            r"
            @Service()
            class Test {}
            ",
            None,
        ),
        (
            r"
            @Injectable({ providedIn: null })
            class Test {}
            ",
            Some(serde_json::json!([{ "allowProvidedInNull": true }])),
        ),
    ];

    let fail = vec![
        ("@Injectable() class Test {}", None),
        ("@Injectable({}) class Test {}", None),
        (
            "const providedIn = 'anotherProperty'; @Injectable({ [providedIn]: [] }) class Test {}",
            None,
        ),
        ("@Injectable({ providedIn: null }) class Test {}", None),
        (
            "@Injectable({ ['providedIn']: undefined }) class Test {} @Injectable() class HttpPostInterceptor implements HttpInterceptor {}",
            None,
        ),
        (
            "@Injectable({ ['providedIn']: undefined }) class Test {} @Injectable() class ProvidedInNgModule {}",
            Some(serde_json::json!([{ "ignoreClassNamePattern": "/(Effects|NgModule)$/" }])),
        ),
        (
            "@Injectable({ [`providedIn`]: undefined }) class Test {} @Injectable() class TestEffects {}",
            Some(serde_json::json!([{ "ignoreClassNamePattern": "/(Effects|NgModule)$/" }])),
        ),
    ];

    let fix: Vec<ExpectFixTestCase> = vec![
        (
            "@Injectable() class Test {}",
            (
                "@Injectable({providedIn: 'any'}) class Test {}",
                "@Injectable({providedIn: 'platform'}) class Test {}",
                "@Injectable({providedIn: 'root'}) class Test {}",
            ),
        )
            .into(),
        (
            "@Injectable({}) class Test {}",
            (
                "@Injectable({providedIn: 'any'}) class Test {}",
                "@Injectable({providedIn: 'platform'}) class Test {}",
                "@Injectable({providedIn: 'root'}) class Test {}",
            ),
        )
            .into(),
        (
            "const providedIn = 'anotherProperty'; @Injectable({ [providedIn]: [] }) class Test {}",
            (
                "const providedIn = 'anotherProperty'; @Injectable({ [providedIn]: [], providedIn: 'any' }) class Test {}",
                "const providedIn = 'anotherProperty'; @Injectable({ [providedIn]: [], providedIn: 'platform' }) class Test {}",
                "const providedIn = 'anotherProperty'; @Injectable({ [providedIn]: [], providedIn: 'root' }) class Test {}",
            ),
        )
            .into(),
        (
            "@Injectable({ providedIn: null }) class Test {}",
            (
                "@Injectable({ providedIn: 'any' }) class Test {}",
                "@Injectable({ providedIn: 'platform' }) class Test {}",
                "@Injectable({ providedIn: 'root' }) class Test {}",
            ),
        )
            .into(),
        (
            "@Injectable({ ['providedIn']: undefined }) class Test {} @Injectable() class HttpPostInterceptor implements HttpInterceptor {}",
            (
                "@Injectable({ ['providedIn']: 'any' }) class Test {} @Injectable() class HttpPostInterceptor implements HttpInterceptor {}",
                "@Injectable({ ['providedIn']: 'platform' }) class Test {} @Injectable() class HttpPostInterceptor implements HttpInterceptor {}",
                "@Injectable({ ['providedIn']: 'root' }) class Test {} @Injectable() class HttpPostInterceptor implements HttpInterceptor {}",
            ),
        )
            .into(),
        // The tester has no three-suggestion form with options, so only the first suggestion is checked.
        (
            "@Injectable({ ['providedIn']: undefined }) class Test {} @Injectable() class ProvidedInNgModule {}",
            "@Injectable({ ['providedIn']: 'any' }) class Test {} @Injectable() class ProvidedInNgModule {}",
            Some(serde_json::json!([{ "ignoreClassNamePattern": "/(Effects|NgModule)$/" }])),
        )
            .into(),
        (
            "@Injectable({ [`providedIn`]: undefined }) class Test {} @Injectable() class TestEffects {}",
            "@Injectable({ [`providedIn`]: 'any' }) class Test {} @Injectable() class TestEffects {}",
            Some(serde_json::json!([{ "ignoreClassNamePattern": "/(Effects|NgModule)$/" }])),
        )
            .into(),
    ];

    Tester::new(UseInjectableProvidedIn::NAME, UseInjectableProvidedIn::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
