use oxc_ast::{
    AstKind,
    ast::{ClassElement, Decorator, MethodDefinitionKind, TSType, TSTypeAnnotation},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{class_decorator, decorator_name},
};

fn prefer_inject_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Prefer using the `inject()` function over constructor parameter injection.",
    )
    .with_help(
        "Use Angular's migration schematic to refactor automatically: `ng generate @angular/core:inject`.",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferInject;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers the `inject()` function over constructor parameter injection in Angular classes
    /// (`@Component`, `@Directive`, `@Injectable`, `@Pipe` and `@Service`).
    ///
    /// ### Why is this bad?
    ///
    /// `inject()` is Angular's current dependency injection API. It works outside constructors,
    /// removes the parameter-property boilerplate and is required by functional guards,
    /// interceptors and signal-based APIs.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Injectable()
    /// class UserService {
    ///   constructor(private http: HttpClient) {}
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Injectable()
    /// class UserService {
    ///   private http = inject(HttpClient);
    /// }
    /// ```
    PreferInject,
    angular,
    style,
    version = "next",
    short_description = "Prefer `inject()` over constructor parameter injection.",
);

const DI_CLASS_DECORATORS: [&str; 5] = ["Component", "Directive", "Injectable", "Pipe", "Service"];

const DI_PARAMETER_DECORATORS: [&str; 5] = ["Inject", "Optional", "Self", "SkipSelf", "Host"];

impl Rule for PreferInject {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if !DI_CLASS_DECORATORS.iter().any(|name| class_decorator(class, name).is_some()) {
            return;
        }
        for element in &class.body.body {
            let ClassElement::MethodDefinition(method) = element else { continue };
            if method.kind != MethodDefinitionKind::Constructor {
                continue;
            }
            let params = &method.value.params;
            for param in &params.items {
                // ESTree puts the annotation of `x: T = init` on the inner identifier, so upstream skips it.
                let type_annotation =
                    param.type_annotation.as_deref().filter(|_| param.initializer.is_none());
                if is_injected(&param.decorators, param.has_modifier(), type_annotation) {
                    ctx.diagnostic(prefer_inject_diagnostic(param.span));
                }
            }
            if let Some(rest) = &params.rest
                && is_injected(&rest.decorators, false, rest.type_annotation.as_deref())
            {
                ctx.diagnostic(prefer_inject_diagnostic(rest.span));
            }
        }
    }
}

fn is_injected(
    decorators: &[Decorator],
    has_modifier: bool,
    type_annotation: Option<&TSTypeAnnotation>,
) -> bool {
    if has_modifier
        || decorators.iter().any(|decorator| {
            decorator_name(decorator).is_some_and(|name| DI_PARAMETER_DECORATORS.contains(&name))
        })
    {
        return true;
    }
    type_annotation.is_some_and(|annotation| {
        !matches!(
            annotation.type_annotation,
            TSType::TSStringKeyword(_)
                | TSType::TSNumberKeyword(_)
                | TSType::TSBooleanKeyword(_)
                | TSType::TSBigIntKeyword(_)
                | TSType::TSSymbolKeyword(_)
                | TSType::TSAnyKeyword(_)
                | TSType::TSUnknownKeyword(_)
        )
    })
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
            class PlainClass {
              constructor(private value: string) {}
            }
        ",
        r"
            @Injectable()
            class UserService {
              private http = inject(HttpClient);
            }
        ",
        r"
            @Service()
            class UserService {
              private http = inject(HttpClient);
            }
        ",
        r"
            @Component({})
            class MyComponent {
              constructor() {}
            }
        ",
        r"
            @Component({})
            class MyComponent extends BaseComponent {
              constructor() {
                super();
              }
            }
        ",
        r"
            @Injectable()
            class Logger {
              constructor(level: string) {}
            }
        ",
    ];

    let fail = vec![
        r"
            @Injectable()
            class UserService {
              constructor(private http: HttpClient) {}
            }
        ",
        r"
            @Service()
            class UserService {
              constructor(private http: HttpClient) {}
            }
        ",
        r"
            @Component({})
            class MyComponent {
              constructor(
                private userService: UserService,
                private http: HttpClient
              ) {}
            }
        ",
        r"
            @Injectable()
            class ConfigService {
              constructor(
                @Inject(CONFIG_TOKEN) private config: AppConfig,
              ) {}
            }
        ",
        r"
            @Injectable()
            class ConfigService {
              constructor(
                @Optional() private logger?: LoggerService,
              ) {}
            }
        ",
        r"
            @Injectable()
            class ConfigService {
              constructor(
                @Self() private control: NgControl,
              ) {}
            }
        ",
        r"
            @Injectable()
            class ConfigService {
              constructor(
                @SkipSelf() private skipSelfService: SkipSelfService,
              ) {}
            }
        ",
        r"
            @Injectable()
            class ConfigService {
              constructor(
                @Host() private hostService: HostService,
              ) {}
            }
        ",
        r"
            @Injectable()
            class ConfigService {
              constructor(
                @Inject(CONFIG_TOKEN) private config: AppConfig,
                @Optional() @Self() private logger?: LoggerService,
              ) {}
            }
        ",
        r"
            @Component({})
            class MyComponent extends BaseComponent {
              constructor(
                private service: MyService,
              ) {
                super();
              }
            }
        ",
        r"
            @Component({})
            class MyComponent {
              constructor(elementRef: ElementRef) {}
            }
        ",
    ];

    Tester::new(PreferInject::NAME, PreferInject::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
