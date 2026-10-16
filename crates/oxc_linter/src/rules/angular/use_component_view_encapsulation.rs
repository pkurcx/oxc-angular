use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{
        decorator_call_name, enclosing_class_decorator, list_item_removal_span,
        named_import_removal_span,
    },
};

fn use_component_view_encapsulation_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Using `ViewEncapsulation.None` makes your styles global, which may have an unintended effect",
    )
    .with_help("Use the default `ViewEncapsulation.Emulated` or `ViewEncapsulation.ShadowDom`.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct UseComponentViewEncapsulation;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows `encapsulation: ViewEncapsulation.None` in Angular `@Component` metadata.
    ///
    /// ### Why is this bad?
    ///
    /// `ViewEncapsulation.None` turns off style encapsulation, so the component's styles become
    /// global and can leak into unrelated components.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({
    ///   encapsulation: ViewEncapsulation.None,
    /// })
    /// class Test {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({
    ///   encapsulation: ViewEncapsulation.Emulated,
    /// })
    /// class Test {}
    /// ```
    UseComponentViewEncapsulation,
    angular,
    restriction,
    suggestion,
    version = "next",
    short_description = "Disallow `ViewEncapsulation.None` in `@Component`.",
);

impl Rule for UseComponentViewEncapsulation {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::ObjectProperty(prop) = node.kind() else { return };
        if !prop.key.is_specific_static_name("encapsulation") {
            return;
        }
        let Expression::StaticMemberExpression(member) = &prop.value else { return };
        if !member.object.is_specific_id("ViewEncapsulation") || member.property.name != "None" {
            return;
        }
        if enclosing_class_decorator(node, ctx).and_then(decorator_call_name) != Some("Component") {
            return;
        }

        ctx.diagnostic_with_suggestion(
            use_component_view_encapsulation_diagnostic(member.property.span),
            |fixer| {
                let fixer = fixer.for_multifix();
                let mut fix = fixer.new_fix_with_capacity(2);
                fix.push(fixer.delete_range(list_item_removal_span(ctx.source_text(), prop.span)));
                // Upstream removes the import without checking for other uses.
                if let Some(span) = named_import_removal_span(
                    ctx.nodes().program(),
                    "@angular/core",
                    "ViewEncapsulation",
                ) {
                    fix.push(fixer.delete_range(span));
                }
                fix.with_message("Remove `ViewEncapsulation.None`")
            },
        );
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "@Component({\n  encapsulation: ViewEncapsulation.Emulated,\n  selector: 'app-foo-bar'\n})\nclass Test {}",
        "@Component({\n  'encapsulation': ViewEncapsulation.Native,\n  selector: 'app-foo-bar',\n})\nclass Test {}",
        "@Component({\n  ['encapsulation']: ViewEncapsulation.ShadowDom,\n})\nclass Test {}",
        "function encapsulation() {\n  return ViewEncapsulation.None;\n}\n\n@Component({\n  [`encapsulation`]: encapsulation()\n})\nclass Test {}",
        "const encapsulation = 'templateUrl';\n@Component({\n  [encapsulation]: '../a.html'\n})\nclass Test {}",
        "const encapsulation = 'templateUrl';\n@Component({\n  encapsulation\n})\nclass Test {}",
        "const test = 'test';\n@Component({\n  encapsulation: test,\n})\nclass Test {}",
        "@Component({\n  encapsulation: undefined,\n})\nclass Test {}",
        "@Component({})\nclass Test {}",
        "const options = {};\n@Component(options)\nclass Test {}",
        "@NgModule({\n  bootstrap: [Foo]\n})\nclass Test {}",
    ];

    // Upstream `~` marker lines stay as empty lines in both the source and the fixed output.
    let fail = vec![
        "@Component({\n  encapsulation: ViewEncapsulation.None,\n\n  selector: 'app-foo-bar',\n})\nclass Test {}",
        "import type { ViewEncapsulation } from '@angular/core';\nimport { HttpClient } from '@angular/common/http';\n\n@Component({\n  selector: 'app-foo-bar',\n  'encapsulation': ViewEncapsulation.None\n\n})\nclass Test {}",
        "import { ViewEncapsulation } from '@angular/core';\nimport { HttpClient } from '@angular/common/http';\n\n@Component({\n  selector: 'app-foo-bar',\n  ['encapsulation']: ViewEncapsulation.None\n\n})\nclass Test {}",
        "import { ViewEncapsulation } from '@angular/core';\nimport { HttpClient } from '@angular/common/http';\n\n@Component({\n  selector: 'app-foo-bar',\n  [`encapsulation`]: ViewEncapsulation.None\n\n})\nclass Test {}",
    ];

    let fix = vec![
        (
            "@Component({\n  encapsulation: ViewEncapsulation.None,\n\n  selector: 'app-foo-bar',\n})\nclass Test {}",
            "@Component({\n  selector: 'app-foo-bar',\n})\nclass Test {}",
        ),
        (
            "import type { ViewEncapsulation } from '@angular/core';\nimport { HttpClient } from '@angular/common/http';\n\n@Component({\n  selector: 'app-foo-bar',\n  'encapsulation': ViewEncapsulation.None\n\n})\nclass Test {}",
            "\nimport { HttpClient } from '@angular/common/http';\n\n@Component({\n  selector: 'app-foo-bar',\n\n})\nclass Test {}",
        ),
        (
            "import { ViewEncapsulation } from '@angular/core';\nimport { HttpClient } from '@angular/common/http';\n\n@Component({\n  selector: 'app-foo-bar',\n  ['encapsulation']: ViewEncapsulation.None\n\n})\nclass Test {}",
            "\nimport { HttpClient } from '@angular/common/http';\n\n@Component({\n  selector: 'app-foo-bar',\n\n})\nclass Test {}",
        ),
        (
            "import { ViewEncapsulation } from '@angular/core';\nimport { HttpClient } from '@angular/common/http';\n\n@Component({\n  selector: 'app-foo-bar',\n  [`encapsulation`]: ViewEncapsulation.None\n\n})\nclass Test {}",
            "\nimport { HttpClient } from '@angular/common/http';\n\n@Component({\n  selector: 'app-foo-bar',\n\n})\nclass Test {}",
        ),
    ];

    Tester::new(
        UseComponentViewEncapsulation::NAME,
        UseComponentViewEncapsulation::PLUGIN,
        pass,
        fail,
    )
    .change_rule_path_extension("ts")
    .expect_fix(fix)
    .test_and_snapshot();
}
