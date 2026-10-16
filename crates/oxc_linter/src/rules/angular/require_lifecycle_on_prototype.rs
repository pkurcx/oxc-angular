use oxc_ast::{
    AstKind,
    ast::{Expression, MemberExpression, PropertyKey},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use oxc_syntax::operator::AssignmentOperator;

use crate::{AstNode, context::LintContext, rule::Rule, utils::is_angular_lifecycle_method};

fn require_lifecycle_on_prototype_diagnostic(span: Span, method: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The {method} lifecycle method should be defined on the object's prototype."
    ))
    .with_help("Declare it as a class method. See https://github.com/angular/angular/issues/38241")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RequireLifecycleOnPrototype;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that lifecycle methods are defined on the object's prototype instead of on an
    /// instance: it reports class fields named after a lifecycle method and assignments of a
    /// lifecycle method to anything other than a `prototype`.
    ///
    /// ### Why is this bad?
    ///
    /// Angular looks lifecycle hooks up on the prototype. A hook defined as an instance property,
    /// such as `ngOnInit = () => {}`, may never be called.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent {
    ///   ngOnInit = () => this.load();
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class UserComponent {
    ///   ngOnInit() { this.load(); }
    /// }
    /// ```
    RequireLifecycleOnPrototype,
    angular,
    correctness,
    version = "next",
    short_description = "Ensure that lifecycle methods are defined on the object's prototype.",
);

impl Rule for RequireLifecycleOnPrototype {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let (name, span) = match node.kind() {
            AstKind::PropertyDefinition(property) if !property.r#type.is_abstract() => {
                match &property.key {
                    PropertyKey::StaticIdentifier(ident) => (ident.name.as_str(), ident.span),
                    PropertyKey::Identifier(ident) => (ident.name.as_str(), ident.span),
                    PropertyKey::StringLiteral(lit) => (lit.value.as_str(), lit.span),
                    _ => return,
                }
            }
            AstKind::AssignmentExpression(assignment)
                if assignment.operator == AssignmentOperator::Assign =>
            {
                match assignment.left.as_member_expression() {
                    Some(MemberExpression::StaticMemberExpression(member))
                        if !is_prototype(&member.object) =>
                    {
                        (member.property.name.as_str(), member.property.span)
                    }
                    Some(MemberExpression::ComputedMemberExpression(member))
                        if !is_prototype(&member.object) =>
                    {
                        match &member.expression {
                            Expression::StringLiteral(lit) => (lit.value.as_str(), lit.span),
                            Expression::Identifier(ident) => (ident.name.as_str(), ident.span),
                            _ => return,
                        }
                    }
                    _ => return,
                }
            }
            _ => return,
        };
        if is_angular_lifecycle_method(name) {
            ctx.diagnostic(require_lifecycle_on_prototype_diagnostic(span, name));
        }
    }
}

// `x.prototype` or `x['prototype']`, also behind parentheses and `as` casts.
fn is_prototype(mut expr: &Expression) -> bool {
    loop {
        match expr {
            Expression::ParenthesizedExpression(paren) => expr = &paren.expression,
            Expression::TSAsExpression(cast) => expr = &cast.expression,
            _ => break,
        }
    }
    match expr {
        Expression::StaticMemberExpression(member) => member.property.name == "prototype",
        Expression::ComputedMemberExpression(member) => match &member.expression {
            Expression::StringLiteral(lit) => lit.value == "prototype",
            Expression::Identifier(ident) => ident.name == "prototype",
            _ => false,
        },
        _ => false,
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    @Component({})
    class Test {
      ngAfterContentChecked() {}
    }
  ",
        r"
    @Component({})
    class Test {
      ngAfterContentInit() {}
    }
  ",
        r"
    @Component({})
    class Test {
      ngAfterViewChecked() {}
    }
  ",
        r"
    @Component({})
    class Test {
      ngAfterViewInit() {}
    }
  ",
        r"
    @Component({})
    class Test {
      ngOnChanges() {}
    }
  ",
        r"
    @Component({})
    class Test {
      ngOnDestroy() {}
    }
  ",
        r"
    @Component({})
    class Test {
      ngOnInit() {}
    }
  ",
        r"
    @Component({})
    class Test {
      ngDoCheck() {}
    }
  ",
        r"
    @Directive({})
    class Test {
      ngAfterContentChecked() {}
    }
  ",
        r"
    @Directive({})
    class Test {
      ngAfterContentInit() {}
    }
  ",
        r"
    @Directive({})
    class Test {
      ngAfterViewChecked() {}
    }
  ",
        r"
    @Directive({})
    class Test {
      ngAfterViewInit() {}
    }
  ",
        r"
    @Directive({})
    class Test {
      ngOnChanges() {}
    }
  ",
        r"
    @Directive({})
    class Test {
      ngOnDestroy() {}
    }
  ",
        r"
    @Directive({})
    class Test {
      ngOnInit() {}
    }
  ",
        r"
    @Directive({})
    class Test {
      ngDoCheck() {}
    }
  ",
        r"
    @Injectable({})
    class Test {
      ngOnDestroy() {}
    }
  ",
        r"
    @Pipe({})
    class Test {
      ngOnDestroy() {}
    }
  ",
        r"
    @Component({})
    class Test {}
    function hook(type) {
      type.prototype.ngOnDestroy = () => {};
    }
    hook(Test);
  ",
        r"
    @Component({})
    class Test {}
    function hook(type) {
      (type.prototype as any).ngOnDestroy = () => {};
    }
    hook(Test);
  ",
        r"
    @Component({})
    class Test {}
    function hook(type) {
      type['prototype'].ngOnDestroy = () => {};
    }
    hook(Test);
  ",
        r"
    @Component({})
    class Test {}
    function hook(type) {
      (type.prototype as unknown as { ngOnDestroy: () => void}).ngOnDestroy = () => {};
    }
    hook(Test);
  ",
        r"
    @Component({})
    class Test {
      onDestroy = () => {}
    }
  ",
        r"
    @Component({})
    class Test {
      constructor() {
        this.onDestroy = () => {}
      }
    }
  ",
        r"
    @Component({})
    class Test {
      constructor() {
        this['onDestroy'] = () => {}
      }
    }
  ",
        r"
    @Component({})
    class Test {
      constructor() {
        let ngOnDestroy;
        ngOnDestroy = () => {};
      }
    }
  ",
    ];

    let fail = vec![
        r"
      @Component({})
      class Test {
        ngOnChanges = func;
      }
      ",
        r"
      @Component({})
      class Test {
        ngOnInit = func;
      }
      ",
        r"
      @Component({})
      class Test {
        ngDoCheck = func;
      }
      ",
        r"
      @Component({})
      class Test {
        ngAfterContentInit = func;
      }
      ",
        r"
      @Component({})
      class Test {
        ngAfterContentChecked = func;
      }
      ",
        r"
      @Component({})
      class Test {
        ngAfterViewInit = func;
      }
      ",
        r"
      @Component({})
      class Test {
        ngAfterViewChecked = func;
      }
      ",
        r"
      @Component({})
      class Test {
        ngOnDestroy = func;
      }
      ",
        r"
      @Directive({})
      class Test {
        ngOnChanges = func;
      }
      ",
        r"
      @Directive({})
      class Test {
        ngOnInit = func;
      }
      ",
        r"
      @Directive({})
      class Test {
        ngDoCheck = func;
      }
      ",
        r"
      @Directive({})
      class Test {
        ngAfterContentInit = func;
      }
      ",
        r"
      @Directive({})
      class Test {
        ngAfterContentChecked = func;
      }
      ",
        r"
      @Directive({})
      class Test {
        ngAfterViewInit = func;
      }
      ",
        r"
      @Directive({})
      class Test {
        ngAfterViewChecked = func;
      }
      ",
        r"
      @Directive({})
      class Test {
        ngOnDestroy = func;
      }
      ",
        r"
      @Injectable({})
      class Test {
        ngOnDestroy = func;
      }
      ",
        r"
      @Pipe({})
      class Test {
        ngOnDestroy = func;
      }
      ",
        r"
      class Test {
        constructor() {
          this.ngOnDestroy = func;
        }
      }
      ",
        r"
      class Test {
        constructor() {
          this['ngOnDestroy'] = func;
        }
      }
      ",
        r"
      class Test {
        run() {
          this.ngOnDestroy = func;
        }
      }
      ",
        r"
      function hook(component) {
        component.ngOnDestroy = func;
      }
      ",
        r"
      function hook(component) {
        (component as any).ngOnDestroy = func;
      }
      ",
        r"
      class Test {
        ['ngOnDestroy'] = func;
      }
      ",
    ];

    Tester::new(RequireLifecycleOnPrototype::NAME, RequireLifecycleOnPrototype::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
