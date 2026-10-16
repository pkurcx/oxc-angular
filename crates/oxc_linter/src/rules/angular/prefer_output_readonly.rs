use oxc_ast::{
    AstKind,
    ast::{Expression, TSType, TSTypeName},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{AstNode, context::LintContext, rule::Rule, utils::decorator_call_name};

fn prefer_output_readonly_diagnostic(span: Span, kind: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Prefer to declare `{kind}` as `readonly` since they are not supposed to be reassigned"
    ))
    .with_help("Add the `readonly` modifier.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferOutputReadonly;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces `readonly` on `@Output()` properties and on `OutputEmitterRef` and `OutputRef`
    /// properties, recognized by their type annotation or by an `output()` or
    /// `outputFromObservable()` initializer.
    ///
    /// ### Why is this bad?
    ///
    /// Parent components bind to an output once. Reassigning the property later breaks those
    /// bindings, and `readonly` lets TypeScript rule that out.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// class Test {
    ///   @Output() change = new EventEmitter<string>();
    ///   select = output<string>();
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// class Test {
    ///   @Output() readonly change = new EventEmitter<string>();
    ///   readonly select = output<string>();
    /// }
    /// ```
    PreferOutputReadonly,
    angular,
    style,
    suggestion,
    version = "next",
    short_description = "Prefer `readonly` on `@Output`, `OutputEmitterRef` and `OutputRef` properties.",
);

impl Rule for PreferOutputReadonly {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::PropertyDefinition(property) = node.kind() else { return };
        // Abstract properties are not checked.
        if property.readonly || property.r#type.is_abstract() {
            return;
        }
        let key = property.key.span();

        for decorator in &property.decorators {
            if decorator_call_name(decorator) == Some("Output") {
                report(ctx, key, "@Output");
            }
        }

        let type_name = property.type_annotation.as_ref().and_then(|annotation| {
            let TSType::TSTypeReference(reference) = &annotation.type_annotation else {
                return None;
            };
            let TSTypeName::IdentifierReference(ident) = &reference.type_name else { return None };
            Some(ident.name.as_str())
        });
        let callee = property.value.as_ref().and_then(|value| {
            let callee = match value {
                Expression::CallExpression(call) => &call.callee,
                Expression::NewExpression(new) => &new.callee,
                _ => return None,
            };
            let Expression::Identifier(ident) = callee else { return None };
            Some(ident.name.as_str())
        });

        if type_name == Some("OutputEmitterRef") || callee == Some("output") {
            report(ctx, key, "OutputEmitterRef");
        }
        if type_name == Some("OutputRef") || callee == Some("outputFromObservable") {
            report(ctx, key, "OutputRef");
        }
    }
}

fn report(ctx: &LintContext<'_>, key: Span, kind: &str) {
    ctx.diagnostic_with_suggestion(prefer_output_readonly_diagnostic(key, kind), |fixer| {
        fixer.insert_text_before_range(key, "readonly ").with_message("Add `readonly` modifier")
    });
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    class Test {
      testEmitter = new EventEmitter<string>();
    }
    ",
        r"
    class Test {
      @Output() readonly testEmitter = new EventEmitter<string>();
    }
    ",
        r"
    class Test {
      testEmitter: NotOutputEmitterRef<string>;
    }
    ",
        r"
    class Test {
      readonly testEmitter: OutputEmitterRef<string>;
    }
    ",
        r"
    class Test {
      testEmitter = notOutput();
    }
    ",
        r"
    class Test {
      readonly testEmitter = output();
    }
    ",
        r"
    class Test {
      testEmitter = notOutput<string>();
    }
    ",
        r"
    class Test {
      readonly testEmitter = output<string>();
    }
    ",
        r"
    class Test {
      readonly testEmitter: OutputRef<string>;
    }
    ",
        r"
    class Test {
      readonly testEmitter = outputFromObservable(testObservable);
    }
    ",
    ];

    let fail = vec![
        r"
        class Test {
          @Output() testEmitter = new EventEmitter<string>();
        }
      ",
        r"
        class Test {
          testEmitter: OutputEmitterRef<string>;
        }
      ",
        r"
        class Test {
          testEmitter = output();
        }
      ",
        r"
        class Test {
          testEmitter = output<string>();
        }
      ",
        r"
        class Test {
          testEmitter: OutputRef<string>;
        }
      ",
        r"
        class Test {
          testEmitter = outputFromObservable(testObservable);
        }
      ",
    ];

    let fix = vec![
        (
            r"
        class Test {
          @Output() testEmitter = new EventEmitter<string>();
        }
      ",
            r"
        class Test {
          @Output() readonly testEmitter = new EventEmitter<string>();
        }
      ",
        ),
        (
            r"
        class Test {
          testEmitter: OutputEmitterRef<string>;
        }
      ",
            r"
        class Test {
          readonly testEmitter: OutputEmitterRef<string>;
        }
      ",
        ),
        (
            r"
        class Test {
          testEmitter = output();
        }
      ",
            r"
        class Test {
          readonly testEmitter = output();
        }
      ",
        ),
        (
            r"
        class Test {
          testEmitter = output<string>();
        }
      ",
            r"
        class Test {
          readonly testEmitter = output<string>();
        }
      ",
        ),
        (
            r"
        class Test {
          testEmitter: OutputRef<string>;
        }
      ",
            r"
        class Test {
          readonly testEmitter: OutputRef<string>;
        }
      ",
        ),
        (
            r"
        class Test {
          testEmitter = outputFromObservable(testObservable);
        }
      ",
            r"
        class Test {
          readonly testEmitter = outputFromObservable(testObservable);
        }
      ",
        ),
    ];

    Tester::new(PreferOutputReadonly::NAME, PreferOutputReadonly::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
