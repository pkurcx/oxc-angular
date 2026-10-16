use oxc_ast::{AstKind, ast::MethodDefinitionKind};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{angular_class_decorator, decorator_name, nearest_class_declaration},
};

fn contextual_decorator_diagnostic(span: Span, class_decorator: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Decorator out of context for \"@{class_decorator}()\""))
        .with_help(format!(
            "Angular does not process this decorator in a `@{class_decorator}` class."
        ))
        .with_label(span)
}

const INNER_CLASS_DECORATORS: [&str; 14] = [
    "Attribute",
    "Host",
    "Inject",
    "Optional",
    "Self",
    "SkipSelf",
    "HostListener",
    "ContentChild",
    "ContentChildren",
    "HostBinding",
    "Input",
    "Output",
    "ViewChild",
    "ViewChildren",
];

// The only inner decorators allowed outside `@Component` and `@Directive`.
const DI_DECORATORS: [&str; 5] = ["Host", "Inject", "Optional", "Self", "SkipSelf"];

#[derive(Debug, Default, Clone)]
pub struct ContextualDecorator;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that class members only use Angular decorators that the enclosing class's Angular
    /// decorator supports. `@Component` and `@Directive` classes accept every member decorator;
    /// `@Injectable`, `@NgModule`, `@Pipe` and `@Service` classes accept only `@Host`, `@Inject`,
    /// `@Optional`, `@Self` and `@SkipSelf`.
    ///
    /// ### Why is this bad?
    ///
    /// Decorators such as `@Input()`, `@Output()`, `@ViewChild()` or `@HostBinding()` only have an
    /// effect in components and directives. Angular ignores them elsewhere, so the code silently
    /// does not do what it says.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Injectable({ providedIn: 'root' })
    /// class Test {
    ///   @Input() label: string;
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({ selector: 'app-test' })
    /// class Test {
    ///   @Input() label: string;
    /// }
    /// ```
    ContextualDecorator,
    angular,
    correctness,
    version = "next",
    short_description = "Ensure that classes use contextual decorators in their body.",
);

impl Rule for ContextualDecorator {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        // Abstract members and parameters without a modifier are not checked.
        let decorators = match node.kind() {
            AstKind::MethodDefinition(method)
                if method.kind != MethodDefinitionKind::Constructor
                    && !method.r#type.is_abstract() =>
            {
                &method.decorators
            }
            AstKind::PropertyDefinition(property) if !property.r#type.is_abstract() => {
                &property.decorators
            }
            AstKind::FormalParameter(param) if param.has_modifier() => &param.decorators,
            _ => return,
        };
        if decorators.is_empty() {
            return;
        }
        let Some(class) = nearest_class_declaration(node, ctx) else { return };
        let Some((_, class_decorator)) = angular_class_decorator(class) else { return };
        if matches!(class_decorator, "Component" | "Directive") {
            return;
        }
        for decorator in decorators {
            if decorator_name(decorator).is_some_and(|name| {
                INNER_CLASS_DECORATORS.contains(&name) && !DI_DECORATORS.contains(&name)
            }) {
                ctx.diagnostic(contextual_decorator_diagnostic(decorator.span, class_decorator));
            }
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @Input()
          get label(): string {
            return this._label;
          }
          set label(value: string) {
            this._label = value;
          }
          private _label: string;
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @Input()
          set label(value: string) {
            this._label = value;
          }
          get label(): string {
            return this._label;
          }
          private _label: string;
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @ViewChild(Pane)
          set label(value: Pane) {
            doSomething();
          }
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @HostListener('mouseover')
          mouseOver() {
            this.doSomething();
          }
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          constructor(
            @Host() private readonly host: DynamicHost
          ) {}
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          constructor(
            @Inject(LOCALE_ID) private readonly localeId: string
          ) {}
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          constructor(
            @Optional() testBase: TestBase,
          ) {}
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          constructor(
            @Self() public readonly test: Test,
          ) {}
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          constructor(
            @SkipSelf() protected readonly parentTest: ParentTest
          ) {}
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @ContentChild(Pane) pane: Pane;
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @ContentChildren(Pane, { descendants: true }) arbitraryNestedPanes: QueryList<Pane>;
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @HostBinding('class.card-outline') private isCardOutline: boolean;
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @Input() label: string;
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @Output() emitter = new EventEmitter<void>();
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @ViewChild(Pane)
          set pane(value: Pane) {
            console.log('panel setter called');
          }
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @ViewChildren(Pane) panes: QueryList<Pane>;
        }
      ",
        r"
        @Component({
          template: 'Hi!'
        })
        class Test {
          @ContentChild(Pane) pane: Pane;

          @Input()
          get label(): string {
            return this._label;
          }
          set label(value: string) {
            this._label = value;
          }
          private _label: string;

          @ViewChild(Pane)
          set label(value: Pane) {
            doSomething();
          }

          get type(): string {
            return this._type;
          }
          set type(value: string) {
            this._type = value;
          }
          private _type: string;

          private prop: string | undefined;

          constructor(
            @Attribute('test') private readonly test: string,
            @Host() @Optional() private readonly host: DynamicHost,
            @Inject(LOCALE_ID) private readonly localeId: string,
            @Inject(TEST_BASE) @Optional() testBase: TestBase,
            @Optional() @Self() public readonly selfTest: Test,
            @Optional() @SkipSelf() protected readonly parentTest: ParentTest
          ) {}

          @HostListener('mouseover')
          mouseOver(): void {
            this.doSomething();
          }

          clickHandler(): void {}
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @Input()
          get label(): string {
            return this._label;
          }
          set label(value: string) {
            this._label = value;
          }
          private _label: string;
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @Input()
          set label(value: string) {
            this._label = value;
          }
          get label(): string {
            return this._label;
          }
          private _label: string;
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @ViewChild(Pane)
          set label(value: Pane) {
            doSomething();
          }
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @HostListener('mouseover')
          mouseOver() {
            this.doSomething();
          }
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          constructor(
            @Host() private readonly host: DynamicHost
          ) {}
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          constructor(
            @Inject(LOCALE_ID) private readonly localeId: string
          ) {}
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          constructor(
            @Optional() testBase: TestBase,
          ) {}
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          constructor(
            @Self() public readonly test: Test,
          ) {}
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          constructor(
            @SkipSelf() protected readonly parentTest: ParentTest
          ) {}
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @ContentChild(Pane) pane: Pane;
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @ContentChildren(Pane, { descendants: true }) arbitraryNestedPanes: QueryList<Pane>;
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @HostBinding('class.card-outline') private isCardOutline: boolean;
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @Input() label: string;
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @Output() emitter = new EventEmitter<void>();
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @ViewChild(Pane)
          set pane(value: Pane) {
            console.log('panel setter called');
          }
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @ViewChildren(Pane) panes: QueryList<Pane>;
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class Test {
          @ContentChild(Pane) pane: Pane;

          @Input()
          get label(): string {
            return this._label;
          }
          set label(value: string) {
            this._label = value;
          }
          private _label: string;

          @ViewChild(Pane)
          set label(value: Pane) {
            doSomething();
          }

          get type(): string {
            return this._type;
          }
          set type(value: string) {
            this._type = value;
          }
          private _type: string;

          private prop: string | undefined;

          constructor(
            @Attribute('test') private readonly test: string,
            @Host() @Optional() private readonly host: DynamicHost,
            @Inject(LOCALE_ID) private readonly localeId: string,
            @Inject(TEST_BASE) @Optional() testBase: TestBase,
            @Optional() @Self() public readonly selfTest: Test,
            @Optional() @SkipSelf() protected readonly parentTest: ParentTest
          ) {}

          @HostListener('mouseover')
          mouseOver(): void {
            this.doSomething();
          }

          clickHandler(): void {}
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          constructor(
            @Host() private readonly host: DynamicHost
          ) {}
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          constructor(
            @Inject(LOCALE_ID) private readonly localeId: string
          ) {}
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          constructor(
            @Optional() testBase: TestBase,
          ) {}
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          constructor(
            @Self() public readonly test: Test,
          ) {}
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          constructor(
            @SkipSelf() protected readonly parentTest: ParentTest
          ) {}
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          get type(): string {
            return this._type;
          }
          set type(value: string) {
            this._type = value;
          }
          private _type: string;

          private prop: string | undefined;

          constructor(
            @Host() @Optional() private readonly host: DynamicHost,
            @Inject(LOCALE_ID) private readonly localeId: string,
            @Inject(TEST_BASE) @Optional() testBase: TestBase,
            @Optional() @Self() public readonly test: Test,
            @Optional() @SkipSelf() protected readonly parentTest: ParentTest
          ) {}

          clickHandler(): void {}
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          constructor(
            @Host() private readonly host: DynamicHost
          ) {}
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          constructor(
            @Inject(LOCALE_ID) private readonly localeId: string
          ) {}
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          constructor(
            @Optional() testBase: TestBase,
          ) {}
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          constructor(
            @Self() public readonly test: Test,
          ) {}
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          constructor(
            @SkipSelf() protected readonly parentTest: ParentTest
          ) {}
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          get type(): string {
            return this._type;
          }
          set type(value: string) {
            this._type = value;
          }
          private _type: string;

          private prop: string | undefined;

          constructor(
            @Host() @Optional() private readonly host: DynamicHost,
            @Inject(LOCALE_ID) private readonly localeId: string,
            @Inject(TEST_BASE) @Optional() testBase: TestBase,
            @Optional() @Self() public readonly test: Test,
            @Optional() @SkipSelf() protected readonly parentTest: ParentTest
          ) {}

          clickHandler(): void {}
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          constructor(
            @Host() private readonly host: DynamicHost
          ) {}
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          constructor(
            @Inject(LOCALE_ID) private readonly localeId: string
          ) {}
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          constructor(
            @Optional() testBase: TestBase,
          ) {}
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          constructor(
            @Self() public readonly test: Test,
          ) {}
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          constructor(
            @SkipSelf() protected readonly parentTest: ParentTest
          ) {}
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          get type(): string {
            return this._type;
          }
          set type(value: string) {
            this._type = value;
          }
          private _type: string;

          private prop: string | undefined;

          constructor(
            @Host() @Optional() private readonly host: DynamicHost,
            @Inject(LOCALE_ID) private readonly localeId: string,
            @Inject(TEST_BASE) @Optional() testBase: TestBase,
            @Optional() @Self() public readonly test: Test,
            @Optional() @SkipSelf() protected readonly parentTest: ParentTest
          ) {}

          clickHandler(): void {}
        }
      ",
        r"
        @Service()
        class Test {
          constructor(
            @Optional() testBase: TestBase,
            @Inject(LOCALE_ID) private readonly localeId: string,
            @Self() public readonly test: Test,
            @SkipSelf() protected readonly parentTest: ParentTest,
            @Host() private readonly host: DynamicHost,
          ) {}
        }
      ",
        r"
        @Injectable()
        class TestService {
          constructor() {}
        }

        @Component({
          selector: 'app-test',
          template: '<h1>Hello</h1>',
          providers: [TestService]
        })
        class TestComponent implements OnChanges {
          @Output() emitter = new EventEmitter<void>();

          constructor(private test: TestService) {}
        }
      ",
    ];

    let fail = vec![
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          @Input()
          get label(): string {
            return this._label;
          }
          set label(value: string) {
            this._label = value;
          }
          private _label: string;
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          @Input()
          set label(value: string) {
            this._label = value;
          }
          get label(): string {
            return this._label;
          }
          private _label: string;
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          @ViewChild(Pane)
          set label(value: Pane) {
            doSomething();
          }
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          @HostListener('mouseover')
          mouseOver() {
            this.doSomething();
          }
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          constructor(
            @Attribute('test') private readonly test: string
          ) {}
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          @ContentChild(Pane) pane: Pane;
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          @ContentChildren(Pane, { descendants: true }) arbitraryNestedPanes: QueryList<Pane>;
        }
      ",
        r"
        @Injectable({
          providedIn: 'root'
        })
        class Test {
          @HostBinding('class.card-outline') private isCardOutline: boolean;
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          @Input() label: string;
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          @Output() emitter = new EventEmitter<void>();
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          @ViewChild(Pane) pane: Pane;
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          @ViewChildren(Pane) panes: QueryList<Pane>;
        }
      ",
        r"
        @NgModule({
          providers: []
        })
        class Test {
          @ContentChild(Pane) pane: Pane;
  
          @Input()
          get label(): string {
            return this._label;
          }
          set label(value: string) {
            this._label = value;
          }
          private _label: string;
  
          @ViewChild(Pane)
          set label(value: Pane) {
            doSomething();
          }
  
          get type(): string {
            return this._type;
          }
          set type(value: string) {
            this._type = value;
          }
          private _type: string;
  
          private prop: string | undefined;
  
          constructor(
            @Attribute('test') private readonly test: string,
            @Inject(LOCALE_ID) localeId: string
          ) {}
  
          @HostListener('mouseover')
          mouseOver() {
            this.doSomething();
          }
  
          clickHandler(): void {}
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @Input()
          get label(): string {
            return this._label;
          }
          set label(value: string) {
            this._label = value;
          }
          private _label: string;
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @Input()
          set label(value: string) {
            this._label = value;
          }
          get label(): string {
            return this._label;
          }
          private _label: string;
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @ViewChild(Pane)
          set label(value: Pane) {
            doSomething();
          }
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @HostListener('mouseover')
          mouseOver() {
            this.doSomething();
          }
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          constructor(
            @Attribute('test') private readonly test: string
          ) {}
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @ContentChild(Pane) pane: Pane;
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @ContentChildren(Pane, { descendants: true }) arbitraryNestedPanes: QueryList<Pane>;
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @HostBinding('class.card-outline') private isCardOutline: boolean;
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @Input() label: string;
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @Output() emitter = new EventEmitter<void>();
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @ViewChild(Pane) pane: Pane;
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @ViewChildren(Pane) panes: QueryList<Pane>;
        }
      ",
        r"
        @Pipe({
          name: 'test'
        })
        class Test {
          @ContentChild(Pane) pane: Pane;
  
          @Input()
          get label(): string {
            return this._label;
          }
          set label(value: string) {
            this._label = value;
          }
          private _label: string;
  
          @ViewChild(Pane)
          set label(value: Pane) {
            doSomething();
          }
  
          get type(): string {
            return this._type;
          }
          set type(value: string) {
            this._type = value;
          }
          private _type: string;
  
          private prop: string | undefined;
  
          constructor(
            @Attribute('test') private readonly test: string,
            @Inject(LOCALE_ID) localeId: string
          ) {}
  
          @HostListener('mouseover')
          mouseOver() {
            this.doSomething();
          }
  
          clickHandler(): void {}
        }
      ",
        r"
        @Service()
        class Test {
          @Input() label: string;
        }
      ",
        r"
        @Service()
        class Test {
          @Output() emitter = new EventEmitter<void>();
        }
      ",
        r"
        @Directive({
          selector: 'test'
        })
        class TestDirective {
          @Input() label: string;
        }
  
        @Pipe({
          name: 'test'
        })
        class Test {
          @Input() label: string;
        }
      ",
    ];

    Tester::new(ContextualDecorator::NAME, ContextualDecorator::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
