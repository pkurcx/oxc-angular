use oxc_ast::{
    AstKind,
    ast::{
        CallExpression, Class, ClassElement, Expression, MethodDefinition, MethodDefinitionKind,
        PropertyKey,
    },
};
use oxc_ast_visit::{VisitJs, walk_js};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{angular_class_decorator, nearest_class_declaration},
};

fn no_implicit_take_until_destroyed_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "`takeUntilDestroyed()` must be called with an explicit `DestroyRef` parameter when used outside of an injection context.",
    )
    .with_help(
        "Inject `DestroyRef` with `inject(DestroyRef)` and pass it: `takeUntilDestroyed(this.destroyRef)`. \
        See https://angular.dev/guide/di/dependency-injection-context and https://angular.dev/ecosystem/rxjs-interop/take-until-destroyed",
    )
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoImplicitTakeUntilDestroyed;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that `takeUntilDestroyed()` is called with an explicit `DestroyRef` when used
    /// outside of an injection context.
    ///
    /// ### Why is this bad?
    ///
    /// Without an argument, `takeUntilDestroyed()` injects the current `DestroyRef`, which only
    /// works in an injection context: a constructor or field initializer of a class decorated with
    /// `@Component`, `@Directive`, `@Injectable`, `@Pipe` or `@Service`, or a `factory` /
    /// `useFactory` function. Elsewhere, such as in `ngOnInit()`, a regular method, a plain class
    /// or an `@NgModule`, it throws `NG0203` at runtime.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({})
    /// class Test {
    ///   ngOnInit() {
    ///     this.data$.pipe(takeUntilDestroyed()).subscribe();
    ///   }
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class Test {
    ///   private destroyRef = inject(DestroyRef);
    ///
    ///   constructor() {
    ///     this.data$.pipe(takeUntilDestroyed()).subscribe();
    ///   }
    ///
    ///   ngOnInit() {
    ///     this.data$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe();
    ///   }
    /// }
    /// ```
    NoImplicitTakeUntilDestroyed,
    angular,
    correctness,
    none,
    version = "next",
    short_description = "Require an explicit `DestroyRef` for `takeUntilDestroyed()` outside of an injection context.",
);

impl Rule for NoImplicitTakeUntilDestroyed {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::CallExpression(call) = node.kind() else { return };
        if !call.callee.is_specific_id("takeUntilDestroyed")
            || !call.arguments.is_empty()
            || is_in_injection_context(node, ctx)
        {
            return;
        }
        ctx.diagnostic(no_implicit_take_until_destroyed_diagnostic(call.span));
    }
}

fn is_in_injection_context<'a>(node: &AstNode<'a>, ctx: &LintContext<'a>) -> bool {
    let nodes = ctx.nodes();
    nodes.ancestors(node.id()).any(|ancestor| match ancestor.kind() {
        AstKind::ArrowFunctionExpression(_) | AstKind::Function(_) => matches!(
            nodes.parent_kind(ancestor.id()),
            AstKind::ObjectProperty(prop)
                if prop.key.is_specific_id("factory") || prop.key.is_specific_id("useFactory")
        ),
        AstKind::PropertyDefinition(_)
        | AstKind::MethodDefinition(MethodDefinition {
            kind: MethodDefinitionKind::Constructor,
            ..
        }) => di_class(ancestor, ctx).is_some(),
        AstKind::MethodDefinition(method) => di_class(ancestor, ctx)
            .is_some_and(|class| is_method_called_from_injection_context(method, class)),
        _ => false,
    })
}

// The enclosing class declaration when Angular's DI instantiates it; `@NgModule` has no `DestroyRef`.
fn di_class<'a>(node: &AstNode<'a>, ctx: &LintContext<'a>) -> Option<&'a Class<'a>> {
    nearest_class_declaration(node, ctx)
        .filter(|class| angular_class_decorator(class).is_some_and(|(_, name)| name != "NgModule"))
}

// True when the method is called only from constructors and field initializers.
fn is_method_called_from_injection_context(method: &MethodDefinition, class: &Class) -> bool {
    let (name, private) = match &method.key {
        PropertyKey::StaticIdentifier(ident) => (ident.name.as_str(), false),
        PropertyKey::PrivateIdentifier(ident) => (ident.name.as_str(), true),
        _ => return false,
    };

    let mut called = false;
    for member in &class.body.body {
        // Recursive calls do not count.
        if member.span() == method.span {
            continue;
        }
        let mut finder = MethodCallFinder { name, private, found: false };
        finder.visit_class_element(member);
        if !finder.found {
            continue;
        }
        let from_injection_context = match member {
            ClassElement::PropertyDefinition(_) => true,
            ClassElement::MethodDefinition(member) => member.kind.is_constructor(),
            _ => false,
        };
        if !from_injection_context {
            return false;
        }
        called = true;
    }
    called
}

// Finds `this.name()` or `this.#name()`, including inside nested functions.
struct MethodCallFinder<'n> {
    name: &'n str,
    private: bool,
    found: bool,
}

impl<'a> VisitJs<'a> for MethodCallFinder<'_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        self.found |= match &it.callee {
            Expression::StaticMemberExpression(member) => {
                !self.private
                    && matches!(member.object, Expression::ThisExpression(_))
                    && member.property.name.as_str() == self.name
            }
            Expression::PrivateFieldExpression(field) => {
                self.private
                    && matches!(field.object, Expression::ThisExpression(_))
                    && field.field.name.as_str() == self.name
            }
            _ => false,
        };
        walk_js::walk_call_expression(self, it);
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"@Component()
class Test {
  constructor() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Directive()
class Test {
  constructor() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Injectable()
class Test {
  constructor() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Pipe({ name: 'myPipe' })
class MyPipe {
  constructor() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Service()
class TestService {
  constructor() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Component()
class Test {
  private data = this.http.get('/api').pipe(takeUntilDestroyed());
}",
        r"@Injectable()
class TestService {
  private data = this.http.get('/api').pipe(takeUntilDestroyed());
}",
        r"@Pipe({ name: 'myPipe' })
class MyPipe {
  private data = this.http.get('/api').pipe(takeUntilDestroyed());
}",
        r"@Service()
class TestService {
  private data = this.http.get('/api').pipe(takeUntilDestroyed());
}",
        r"const MY_TOKEN = new InjectionToken('my-token', {
  factory: () => {
    return someObservable.pipe(takeUntilDestroyed());
  }
});",
        r"@Component()
class Test {
  ngOnInit() {
    this.data$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe();
  }
}",
        r"@Component()
class Test {
  doSomething() {
    this.data$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe();
  }
}",
        r"@Injectable()
class TestService {
  loadData() {
    this.data$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe();
  }
}",
        r"@Component({
  providers: [
    {
      provide: SomeToken,
      useFactory: () => data$.pipe(takeUntilDestroyed())
    }
  ]
})
class Test {}",
        r"@Component()
class Test {
  #myObservable = new Subject();

  constructor() {
    this.#someLogic();
  }

  #someLogic() {
    this.#myObservable.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Component()
class Test {
  constructor() {
    this.initSubscriptions();
  }

  private initSubscriptions() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Directive()
class Test {
  constructor() {
    this.setup();
  }

  private setup() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Injectable()
class TestService {
  constructor() {
    this.init();
  }

  private init() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Component()
class Test {
  private data = this.init();

  constructor() {
    this.init();
  }

  private init() {
    return this.data$.pipe(takeUntilDestroyed());
  }
}",
        r"@Component()
class Test {
  constructor() {
    this.init(0);
  }

  private init(attempt: number) {
    if (attempt < 3) {
      this.init(attempt + 1);
    }
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
    ];

    let fail = vec![
        r"@Component()
class Test {
  ngOnInit() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Component()
class Test {
  ngAfterViewInit() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Component()
class Test {
  anyMethod() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Directive()
class Test {
  anyMethod() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Injectable()
class TestService {
  ngOnDestroy() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Injectable()
class TestService {
  loadData() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"class Test {
  constructor() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@NgModule()
class AppModule {
  constructor() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@NgModule()
class AppModule {
  private data = this.http.get('/api').pipe(takeUntilDestroyed());
}",
        r"@Component()
class Test {
  constructor() {}

  ngOnInit() {
    this.loadData();
  }

  loadData() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Component()
class Test {
  constructor() {
    this.loadData();
  }

  ngOnInit() {
    this.loadData();
  }

  loadData() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
        r"@Component()
class Test {
  private data = this.loadData();

  ngOnInit() {
    this.loadData();
  }

  private loadData() {
    return this.data$.pipe(takeUntilDestroyed());
  }
}",
        r"@Injectable()
class TestService {
  constructor() {
    this.#subscribe();
  }

  refresh() {
    this.#subscribe();
  }

  #subscribe() {
    this.data$.pipe(takeUntilDestroyed()).subscribe();
  }
}",
    ];

    Tester::new(
        NoImplicitTakeUntilDestroyed::NAME,
        NoImplicitTakeUntilDestroyed::PLUGIN,
        pass,
        fail,
    )
    .change_rule_path_extension("ts")
    .test_and_snapshot();
}
