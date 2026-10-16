use oxc_ast::AstKind;
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{decorator_call_name, decorator_metadata, metadata_property},
};

fn no_queries_metadata_property_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Use query decorators rather than the `queries` metadata property")
        .with_help(
            "Declare the query with `@ViewChild`, `@ViewChildren`, `@ContentChild` or `@ContentChildren` on the class member.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoQueriesMetadataProperty;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows the `queries` metadata property of `@Component` and `@Directive`.
    ///
    /// ### Why is this bad?
    ///
    /// Query decorators sit on the class member they fill, so readers see which properties are
    /// view or content queries without cross-referencing the metadata. The `queries` property is a
    /// legacy pattern that keeps the query far from its declaration.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({
    ///   selector: 'app-test',
    ///   queries: { child: new ViewChild(ChildDirective) },
    /// })
    /// class Test {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({ selector: 'app-test' })
    /// class Test {
    ///   @ViewChild(ChildDirective) child: ChildDirective;
    /// }
    /// ```
    NoQueriesMetadataProperty,
    angular,
    style,
    version = "next",
    short_description = "Disallow the `queries` metadata property.",
);

impl Rule for NoQueriesMetadataProperty {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if !class.is_declaration() {
            return;
        }
        for decorator in &class.decorators {
            if !matches!(decorator_call_name(decorator), Some("Component" | "Directive")) {
                continue;
            }
            let queries = decorator_metadata(decorator)
                .and_then(|metadata| metadata_property(metadata, "queries"));
            if let Some(queries) = queries {
                ctx.diagnostic(no_queries_metadata_property_diagnostic(queries.span));
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
      selector: 'app-test',
      template: 'Hello'
    })
    class TestComponent {}
",
        r"
    @Directive({
      selector: 'app-test'
    })
    class TestDirective {}
",
    ];

    let fail = vec![
        r"
        @Component({
          queries: {
            contentChild: new ContentChild(ChildDirective),
            contentChildren: new ContentChildren(ChildDirective),
            viewChild: new ViewChild(ChildDirective),
            viewChildren: new ViewChildren(ChildDirective)
          },
          selector: 'app-test'
        })
        class TestComponent {}
      ",
        r"
        @Directive({
          queries: {
            contentChild: new ContentChild(ChildDirective),
            contentChildren: new ContentChildren(ChildDirective),
            viewChild: new ViewChild(ChildDirective),
            viewChildren: new ViewChildren(ChildDirective)
          },
          selector: 'app-test'
        })
        class TestDirective {}
      ",
    ];

    Tester::new(NoQueriesMetadataProperty::NAME, NoQueriesMetadataProperty::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
