use oxc_ast::{
    AstKind,
    ast::{ArrayExpressionElement, Expression},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use rustc_hash::FxHashSet;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{decorator_call_name, enclosing_class_decorator},
};

fn no_duplicates_in_metadata_arrays_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Entry is duplicated in metadata array")
        .with_help("Remove the duplicate entry.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDuplicatesInMetadataArrays;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows duplicate identifiers and strings in the metadata arrays of `@NgModule`
    /// (`providers`, `declarations`, `imports`, `exports`), `@Component` (`imports`, `providers`,
    /// `viewProviders`, `styleUrls`) and `@Directive` (`providers`).
    ///
    /// ### Why is this bad?
    ///
    /// A duplicate entry serves no purpose and usually comes from a copy-paste or merge mistake.
    /// A provider listed twice is registered twice, which can surprise code that expects a
    /// single instance.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @NgModule({ imports: [ImportA, ImportB, ImportA] })
    /// class Test {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @NgModule({ imports: [ImportA, ImportB] })
    /// class Test {}
    /// ```
    NoDuplicatesInMetadataArrays,
    angular,
    suspicious,
    version = "next",
    short_description = "Ensure that metadata arrays do not contain duplicate entries.",
);

impl Rule for NoDuplicatesInMetadataArrays {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::ArrayExpression(array) = node.kind() else { return };
        let AstKind::ObjectProperty(property) = ctx.nodes().parent_kind(node.id()) else {
            return;
        };
        if !matches!(&property.value, Expression::ArrayExpression(value) if value.span == array.span)
        {
            return;
        }
        let Some(key) = property.key.static_name() else { return };
        let Some(decorator) = enclosing_class_decorator(node, ctx) else { return };
        let checked: &[&str] = match decorator_call_name(decorator) {
            Some("NgModule") => &["providers", "declarations", "imports", "exports"],
            Some("Component") => &["imports", "providers", "viewProviders", "styleUrls"],
            Some("Directive") => &["providers"],
            _ => return,
        };
        if !checked.contains(&key.as_ref()) {
            return;
        }

        // Identifiers and strings share one key space: `[A, 'A']` is a duplicate.
        let mut seen = FxHashSet::default();
        for element in &array.elements {
            let entry = match element {
                ArrayExpressionElement::Identifier(ident) => ident.name.as_str(),
                ArrayExpressionElement::StringLiteral(lit) => lit.value.as_str(),
                _ => continue,
            };
            if !seen.insert(entry) {
                ctx.diagnostic(no_duplicates_in_metadata_arrays_diagnostic(element.span()));
            }
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    @NgModule({
      providers: [ProviderA, ProviderB, ProviderC],
      declarations: [DeclarationA, DeclarationB, DeclarationC],
      imports: [ImportA, ImportB, ImportC],
      exports: [ExportA, ExportB, ExportC]
    })
    class Test {}
  ",
        r"
    @Component({
      imports: [ImportA, ImportB, ImportC],
      providers: [ProviderA, ProviderB, ProviderC],
      viewProviders: [ViewProviderA, ViewProviderB, ViewProviderC],
      styleUrls: ['a.css', 'b.css', 'c.css']
    })
    class Test {}
  ",
        r"
    @Directive({
      providers: [ProviderA, ProviderB, ProviderC],
    })
    class Test {}
  ",
    ];

    let fail = vec![
        r"
      @NgModule({
        providers: [ProviderA, ProviderB, ProviderA, ProviderB]
      })
      class Test {}
    ",
        r"
      @NgModule({
        declarations: [DeclarationA, DeclarationB, DeclarationA, DeclarationB],
      })
      class Test {}
    ",
        r"
      @NgModule({
        imports: [ImportA, ImportB, ImportA, ImportB]
      })
      class Test {}
    ",
        r"
      @NgModule({
        exports: [ExportA, ExportB, ExportA, ExportB]
      })
      class Test {}
    ",
        r"
      @Component({
        imports: [ImportA, ImportB, ImportA, ImportB]
      })
      class Test {}
    ",
        r"
      @Component({
        providers: [ProviderA, ProviderB, ProviderA, ProviderB]
      })
      class Test {}
    ",
        r"
      @Component({
        viewProviders: [ViewProviderA, ViewProviderB, ViewProviderA, ViewProviderB]
      })
      class Test {}
    ",
        r"
      @Component({
        styleUrls: ['a.css', 'b.css', 'a.css', 'b.css']
      })
      class Test {}
    ",
        r"
      @Directive({
        providers: [ProviderA, ProviderB, ProviderA, ProviderB]
      })
      class Test {}
    ",
        r"
      @NgModule({
        imports: [
          ImportA,
          ImportB,
          ImportA,
          ImportB,
          ImportA
        ]
      })
      class Test {}
    ",
    ];

    Tester::new(
        NoDuplicatesInMetadataArrays::NAME,
        NoDuplicatesInMetadataArrays::PLUGIN,
        pass,
        fail,
    )
    .change_rule_path_extension("ts")
    .test_and_snapshot();
}
