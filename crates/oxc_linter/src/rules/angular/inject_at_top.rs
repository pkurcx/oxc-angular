use lazy_regex::Regex;
use oxc_ast::{
    AstKind,
    ast::{ArrowFunctionExpression, CallExpression, Class, ClassElement, Expression, Function},
};
use oxc_ast_visit::{Visit, walk};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_semantic::ScopeFlags;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::class_decorator,
};

fn inject_at_top_diagnostic(span: Span, function_name: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Move this {function_name}() to the top of the class."))
        .with_help("Class fields are initialized in the order they are written, so anything declared above this line cannot safely use the service yet.")
        .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct InjectAtTop(Box<InjectAtTopConfig>);

#[derive(Debug, Default, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct InjectAtTopConfig {
    /// Additional functions that perform injection, such as your own `injectTranslations()`
    /// helper. Each entry is a regular expression that must match the whole function name, so
    /// plain names such as `injectTranslations` work as well as patterns such as `inject[A-Z].*`.
    #[serde(deserialize_with = "deserialize_full_match_regex_vec")]
    additional_inject_functions: Vec<Regex>,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires `inject()` calls in Angular classes (`@Component`, `@Directive`, `@Injectable`,
    /// `@Pipe` and `@Service`) to be declared before any other non-static member.
    ///
    /// ### Why is this bad?
    ///
    /// Class fields are initialized in the order they are declared. A field, getter or method
    /// declared above an `inject()` field that reads the injected service sees `undefined`, and
    /// TypeScript does not catch the read when it goes through a getter or a method call.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({})
    /// class MyComponent {
    ///   protected readonly pageTitle = `Dashboard ${this.fullName}`;
    ///   private get fullName() {
    ///     return this.userService.name;
    ///   }
    ///   private readonly userService = inject(UserService);
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class MyComponent {
    ///   private readonly userService = inject(UserService);
    ///   protected readonly pageTitle = `Dashboard ${this.fullName}`;
    ///   private get fullName() {
    ///     return this.userService.name;
    ///   }
    /// }
    /// ```
    InjectAtTop,
    angular,
    pedantic,
    config = InjectAtTopConfig,
    version = "next",
    short_description = "Require `inject()` calls to be declared at the top of the class.",
);

const DI_CLASS_DECORATORS: [&str; 5] = ["Component", "Directive", "Injectable", "Pipe", "Service"];

impl Rule for InjectAtTop {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if !DI_CLASS_DECORATORS.iter().any(|name| class_decorator(class, name).is_some()) {
            return;
        }
        let mut seen_non_inject = false;
        for element in &class.body.body {
            if element.r#static() {
                continue;
            }
            if let ClassElement::PropertyDefinition(property) = element
                && let Some(value) = &property.value
                && let Some(function_name) = self.find_eager_inject(value)
            {
                if seen_non_inject {
                    ctx.diagnostic(inject_at_top_diagnostic(property.span, function_name));
                }
                continue;
            }
            seen_non_inject = true;
        }
    }
}

impl InjectAtTop {
    fn is_inject_function(&self, name: &str) -> bool {
        name == "inject" || self.0.additional_inject_functions.iter().any(|re| re.is_match(name))
    }

    fn find_eager_inject<'a>(&self, value: &Expression<'a>) -> Option<&'a str> {
        let mut finder = EagerInjectFinder { rule: self, found: None };
        finder.visit_expression(value);
        finder.found
    }
}

// Finds an inject call evaluated with the initializer: it skips function bodies and classes,
// except for immediately invoked functions.
struct EagerInjectFinder<'r, 'a> {
    rule: &'r InjectAtTop,
    found: Option<&'a str>,
}

impl<'a> Visit<'a> for EagerInjectFinder<'_, 'a> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.found.is_some() {
            return;
        }
        if let Expression::Identifier(callee) = &call.callee
            && self.rule.is_inject_function(callee.name.as_str())
        {
            self.found = Some(callee.name.as_str());
            return;
        }
        match call.callee.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow) => {
                self.visit_formal_parameters(&arrow.params);
                self.visit_arrow_function_body(&arrow.body);
            }
            Expression::FunctionExpression(function) => {
                self.visit_formal_parameters(&function.params);
                if let Some(body) = &function.body {
                    self.visit_function_body(body);
                }
            }
            _ => {
                walk::walk_call_expression(self, call);
                return;
            }
        }
        self.visit_arguments(&call.arguments);
    }

    fn visit_expression(&mut self, expr: &Expression<'a>) {
        if self.found.is_none() {
            walk::walk_expression(self, expr);
        }
    }

    fn visit_function(&mut self, _function: &Function<'a>, _flags: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _arrow: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _class: &Class<'a>) {}
}

// Anchors each pattern so it must match the whole function name, like upstream `toPattern`.
fn deserialize_full_match_regex_vec<'de, D>(deserializer: D) -> Result<Vec<Regex>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    Vec::<String>::deserialize(deserializer)?
        .iter()
        .map(|pattern| Regex::new(&format!("^(?:{pattern})$")))
        .collect::<Result<Vec<_>, _>>()
        .map_err(D::Error::custom)
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        (
            r"
            class PlainClass {
              private get userName() {
                return this.userService.name;
              }
              private readonly userService = inject(UserService);
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly userService = inject(UserService);

              protected readonly pageTitle = `Dashboard ${this.fullName}`;

              private get fullName() {
                return `${this.userService.firstName} ${this.userService.lastName}`.trim();
              }
            }
            ",
            None,
        ),
        (
            r"
            @Injectable()
            class MyService {
              private readonly http = inject(HttpClient);
              private readonly router = inject(Router);
              private readonly config = inject(CONFIG_TOKEN);

              doThing() {}
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private value = 42;
              private get doubled() {
                return this.value * 2;
              }
            }
            ",
            None,
        ),
        (
            r"
            @Directive({})
            class MyDirective {
              private readonly optional = inject(SOME_TOKEN, { optional: true });
              foo() {}
            }
            ",
            None,
        ),
        (
            r"
            @Pipe({ name: 'x' })
            class MyPipe {
              static readonly DEFAULT = 'default';
              private readonly myService = inject(MyService);
            }
            ",
            None,
        ),
        (
            r"
            @Service()
            class UserService {
              private readonly http = inject(HttpClient);

              private get baseUrl() {
                return this.http.baseUrl;
              }
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly count = 0;
              private readonly userServiceFactory = () => inject(UserService);
            }
            ",
            None,
        ),
        (
            r"
            @Injectable()
            class MyService {
              private value = 0;
              build() {
                return inject(HttpClient);
              }
            }
            ",
            None,
        ),
        (
            r"
            @Directive({})
            class MyDirective {
              private flag = false;
              private readonly userServiceFactory = function () { return inject(UserService); };
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly strategy = cond ? inject(PrimaryStrategy) : inject(FallbackStrategy);
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly logger = inject(LoggerService, { optional: true }) ?? noopLogger;
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly config = inject(AppConfig, { optional: true })!;
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly featureFlags = inject(FlagsService) as FeatureFlags;
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly tracedHttp = withTracing(inject(HttpClient));
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly apiClient = new ApiClient(inject(HttpClient));
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly handlers = [inject(HandlerA), inject(HandlerB)];
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly deps = { users: inject(UserService) };
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly userService = (audit(), inject(UserService));
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly currentUserId = `${inject(UserService).id}`;
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly client = (() => inject(ApiClient))();
              private count = 0;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly strategyFactory = () => (cond ? inject(PrimaryStrategy) : inject(FallbackStrategy));
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly userServiceFactory = withFactory(() => inject(UserService));
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly apiClient = new ApiClient(function () { return inject(HttpClient); });
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly handlersFactory = () => collectHandlers([inject(HandlerA), inject(HandlerB)]);
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly t = injectTranslations();
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private readonly t = injectTranslations();
              private count = 0;
            }
            ",
            Some(serde_json::json!([{ "additionalInjectFunctions": ["injectTranslations"] }])),
        ),
        (
            r"
            @Injectable()
            class MyService {
              private readonly http = inject(HttpClient);
              private readonly t = injectTranslations();
              private readonly flags = injectFeatureFlags();

              doThing() {}
            }
            ",
            Some(serde_json::json!([{ "additionalInjectFunctions": ["inject[A-Z].*"] }])),
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly t = injectTranslationsLater();
            }
            ",
            Some(serde_json::json!([{ "additionalInjectFunctions": ["injectTranslations"] }])),
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly translationsFactory = () => injectTranslations();
            }
            ",
            Some(serde_json::json!([{ "additionalInjectFunctions": ["injectTranslations"] }])),
        ),
    ];

    let fail = vec![
        (
            r"
            @Component({})
            class MyComponent {
              protected readonly pageTitle = `Dashboard ${this.fullName}`;

              private get fullName() {
                return `${this.userService.firstName} ${this.userService.lastName}`.trim();
              }

              private readonly userService = inject(UserService);
            }
            ",
            None,
        ),
        (
            r"
            @Injectable()
            class MyService {
              private count = 0;
              private readonly http = inject(HttpClient);
            }
            ",
            None,
        ),
        (
            r"
            @Directive({})
            class MyDirective {
              doThing() {}

              private readonly myService = inject(MyService);
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              constructor() {}

              private readonly myService = inject(MyService);
            }
            ",
            None,
        ),
        (
            r"
            @Injectable()
            class MyService {
              private readonly http = inject(HttpClient);
              private count = 0;
              private readonly router = inject(Router);
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              protected readonly pageTitle = `Dashboard ${this.fullName}`;

              private get fullName() {
                return `${this.userService.firstName} ${this.userService.lastName}`.trim();
              }

              private readonly userService = inject(UserService);
              private readonly router = inject(Router);
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly strategy = cond ? inject(PrimaryStrategy) : inject(FallbackStrategy);
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly logger = inject(LoggerService, { optional: true }) ?? noopLogger;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly config = inject(AppConfig, { optional: true })!;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly featureFlags = inject(FlagsService) as FeatureFlags;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly tracedHttp = withTracing(inject(HttpClient));
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly apiClient = new ApiClient(inject(HttpClient));
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly handlers = [inject(HandlerA), inject(HandlerB)];
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly deps = { users: inject(UserService) };
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly userService = (audit(), inject(UserService));
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly currentUserId = `${inject(UserService).id}`;
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly client = (() => inject(ApiClient))();
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly services = collectServices([inject(UserService), { router: inject(Router) }]);
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly legacyClient = (function () { return inject(ApiClient); })();
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly tracedClient = (() => withTracing(inject(ApiClient)))();
            }
            ",
            None,
        ),
        (
            r"
            @Service()
            class UserService {
              private get baseUrl() {
                return this.http.baseUrl;
              }

              private readonly http = inject(HttpClient);
            }
            ",
            None,
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly t = injectTranslations();
            }
            ",
            Some(serde_json::json!([{ "additionalInjectFunctions": ["injectTranslations"] }])),
        ),
        (
            r"
            @Injectable()
            class MyService {
              private readonly http = inject(HttpClient);
              private count = 0;
              private readonly flags = injectFeatureFlags();
            }
            ",
            Some(serde_json::json!([{ "additionalInjectFunctions": ["inject[A-Z].*"] }])),
        ),
        (
            r"
            @Component({})
            class MyComponent {
              private count = 0;
              private readonly t = withScope(injectTranslations());
            }
            ",
            Some(serde_json::json!([{ "additionalInjectFunctions": ["injectTranslations"] }])),
        ),
    ];

    Tester::new(InjectAtTop::NAME, InjectAtTop::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
