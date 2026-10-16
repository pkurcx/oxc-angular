use oxc_ast::{
    AstKind,
    ast::{
        Argument, Expression, ImportDeclarationSpecifier, ObjectProperty, ObjectPropertyKind,
        Program, Statement,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        decorator_call_name, list_item_removal_span, named_import_insertion,
        named_import_removal_span,
    },
};

fn prefer_service_decorator_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Use the `@Service()` decorator instead of `@Injectable({ providedIn: 'root' })`.",
    )
    .with_help("`@Service()` is shorthand for `@Injectable({ providedIn: 'root' })`.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct PreferServiceDecorator;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Prefers the `@Service()` decorator (Angular 22+) over `@Injectable({ providedIn: 'root' })`.
    /// An `@Injectable` whose metadata holds anything besides `providedIn: 'root'` and an optional
    /// `useFactory` is left alone, because `@Service()` has no equivalent for it.
    ///
    /// ### Why is this bad?
    ///
    /// `@Service()` expresses the same root-provided, tree-shakable service without the metadata
    /// boilerplate.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Injectable({ providedIn: 'root' })
    /// class MyService {}
    ///
    /// @Injectable({ providedIn: 'root', useFactory: () => new Other() })
    /// class Other {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Service()
    /// class MyService {}
    ///
    /// @Service({ factory: () => new Other() })
    /// class Other {}
    /// ```
    PreferServiceDecorator,
    angular,
    style,
    fix,
    version = "next",
    short_description = "Prefer `@Service()` over `@Injectable({ providedIn: 'root' })`.",
);

const ANGULAR_CORE: &str = "@angular/core";

impl Rule for PreferServiceDecorator {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if !class.is_declaration() {
            return;
        }
        for decorator in &class.decorators {
            if decorator_call_name(decorator) != Some("Injectable") {
                continue;
            }
            let Expression::CallExpression(call) = &decorator.expression else { continue };
            let [Argument::ObjectExpression(metadata)] = &call.arguments[..] else {
                continue;
            };

            // Spread elements are not counted, as upstream does.
            let properties: Vec<&ObjectProperty<'a>> = metadata
                .properties
                .iter()
                .filter_map(|property| match property {
                    ObjectPropertyKind::ObjectProperty(property) => Some(&**property),
                    ObjectPropertyKind::SpreadProperty(_) => None,
                })
                .collect();
            let find = |name: &str| {
                properties
                    .iter()
                    .copied()
                    .find(|property| property.key.is_specific_static_name(name))
            };
            let Some(provided_in) = find("providedIn") else { continue };
            let use_factory = find("useFactory");
            if !is_root(&provided_in.value)
                || properties.len() != 1 + usize::from(use_factory.is_some())
            {
                continue;
            }

            let diagnostic = prefer_service_decorator_diagnostic(decorator.span);
            // Dropping the metadata or one of its properties would lose a spread.
            if properties.len() != metadata.properties.len() {
                ctx.diagnostic(diagnostic);
                continue;
            }
            ctx.diagnostic_with_fix(diagnostic, |fixer| {
                let fixer = fixer.for_multifix();
                let mut fix = fixer.new_fix_with_capacity(5);
                fix.push(fixer.replace(call.callee.span(), "Service"));

                let program = ctx.nodes().program();
                let from_used_once =
                    ctx.scoping().get_root_binding("Injectable".into()).is_some_and(|symbol| {
                        ctx.scoping().get_resolved_reference_ids(symbol).len() == 1
                    });
                let to_insertion = named_import_insertion(program, ANGULAR_CORE, "Service", false);
                match import_specifier_span(program, "Injectable") {
                    Some(span) if from_used_once && to_insertion.is_some() => {
                        fix.push(fixer.replace(span, "Service"));
                    }
                    _ => {
                        if let Some((span, text)) = to_insertion {
                            fix.push(fixer.insert_text_after_range(span, text));
                        }
                        if from_used_once
                            && let Some(span) =
                                named_import_removal_span(program, ANGULAR_CORE, "Injectable")
                        {
                            fix.push(fixer.delete_range(span));
                        }
                    }
                }

                match use_factory {
                    None => fix.push(fixer.delete_range(metadata.span)),
                    Some(use_factory) => {
                        fix.push(fixer.replace(use_factory.key.span(), "factory"));
                        fix.push(fixer.delete_range(list_item_removal_span(
                            ctx.source_text(),
                            provided_in.span,
                        )));
                    }
                }
                fix.with_message("Replace with `@Service()`")
            });
        }
    }
}

fn is_root(value: &Expression) -> bool {
    match value {
        Expression::StringLiteral(literal) => literal.value == "root",
        Expression::TemplateLiteral(literal) => {
            literal.single_quasi().is_some_and(|quasi| quasi.as_str() == "root")
        }
        _ => false,
    }
}

fn import_specifier_span(program: &Program, name: &str) -> Option<Span> {
    program.body.iter().find_map(|statement| {
        let Statement::ImportDeclaration(import) = statement else { return None };
        if import.source.value != ANGULAR_CORE {
            return None;
        }
        import.specifiers.as_ref()?.iter().find_map(|specifier| match specifier {
            ImportDeclarationSpecifier::ImportSpecifier(s) if s.imported.name() == name => {
                Some(s.span)
            }
            _ => None,
        })
    })
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "class Test {}",
        r"
            @Injectable()
            class Test {}
        ",
        r"
            @Injectable({ providedIn: null })
            class Test {}
        ",
        r"
            @Injectable({ providedIn: 'platform' })
            class Test {}
        ",
        r"
            @Injectable({ providedIn: 'any' })
            class Test {}
        ",
        r"
            @Injectable({ providedIn: SomeModule })
            class Test {}
        ",
        r"
            @Injectable({ useClass: TestImpl })
            class Test {}
        ",
        r"
            @Injectable({ useExisting: TestImpl })
            class Test {}
        ",
        r"
            @Injectable({ useValue: TEST_VALUE })
            class Test {}
        ",
        r"
            @Injectable({ providedIn: 'root', useClass: TestImpl })
            class Test {}
        ",
        r"
            @Injectable({ providedIn: 'root', useFactory: () => new Test(), deps: [Dep] })
            class Test {}
        ",
        r"
            @Service()
            class Test {}
        ",
        r"
            @CustomInjectable({ providedIn: 'root' })
            class Test {}
        ",
    ];

    let fail = vec![
        "import { Injectable } from '@angular/core';\n\n@Injectable({ providedIn: 'root' })\nclass MyService {}\n",
        "import { Injectable } from '@angular/core';\n\n@Injectable({ providedIn: `root` })\nclass MyService {}\n",
        "import { Injectable } from '@angular/core';\n\n@Injectable({ providedIn: 'root', useFactory: () => new MyService() })\nclass MyService {}\n",
        "import { Injectable } from '@angular/core';\n\n@Injectable({ useFactory: () => new MyService(), providedIn: 'root' })\nclass MyService {}\n",
        "import { Service, Injectable } from '@angular/core';\n\n@Injectable({ providedIn: 'root' })\nclass A {}\n\n@Service()\nclass B {}\n",
        "import { Injectable } from '@angular/core';\n\n@Injectable({\n  providedIn: 'root',\n})\nclass MyService {}\n",
        "import { Injectable } from '@angular/core';\n\n@Injectable({\n  providedIn: 'root',\n  useFactory: () => new MyService(),\n})\nclass MyService {}\n",
    ];

    let fix = vec![
        (
            "import { Injectable } from '@angular/core';\n\n@Injectable({ providedIn: 'root' })\nclass MyService {}\n",
            "import { Service } from '@angular/core';\n\n@Service()\nclass MyService {}\n",
        ),
        (
            "import { Injectable } from '@angular/core';\n\n@Injectable({ providedIn: `root` })\nclass MyService {}\n",
            "import { Service } from '@angular/core';\n\n@Service()\nclass MyService {}\n",
        ),
        (
            "import { Injectable } from '@angular/core';\n\n@Injectable({ providedIn: 'root', useFactory: () => new MyService() })\nclass MyService {}\n",
            "import { Service } from '@angular/core';\n\n@Service({ factory: () => new MyService() })\nclass MyService {}\n",
        ),
        (
            "import { Injectable } from '@angular/core';\n\n@Injectable({ useFactory: () => new MyService(), providedIn: 'root' })\nclass MyService {}\n",
            "import { Service } from '@angular/core';\n\n@Service({ factory: () => new MyService(), })\nclass MyService {}\n",
        ),
        (
            "import { Service, Injectable } from '@angular/core';\n\n@Injectable({ providedIn: 'root' })\nclass A {}\n\n@Service()\nclass B {}\n",
            "import { Service } from '@angular/core';\n\n@Service()\nclass A {}\n\n@Service()\nclass B {}\n",
        ),
        (
            "import { Injectable } from '@angular/core';\n\n@Injectable({\n  providedIn: 'root',\n})\nclass MyService {}\n",
            "import { Service } from '@angular/core';\n\n@Service()\nclass MyService {}\n",
        ),
        (
            "import { Injectable } from '@angular/core';\n\n@Injectable({\n  providedIn: 'root',\n  useFactory: () => new MyService(),\n})\nclass MyService {}\n",
            "import { Service } from '@angular/core';\n\n@Service({\n  factory: () => new MyService(),\n})\nclass MyService {}\n",
        ),
    ];

    Tester::new(PreferServiceDecorator::NAME, PreferServiceDecorator::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
