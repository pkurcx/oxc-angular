use oxc_ast::{
    AstKind,
    ast::{Expression, ObjectProperty},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    fixer::{RuleFix, RuleFixer},
    rule::{DefaultRuleConfig, Rule},
    utils::{
        class_decorator, decorator_metadata, list_item_removal_span, metadata_property,
        named_import_removal_span,
    },
};

fn prefer_on_push_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Components should not opt out of the default `ChangeDetectionStrategy.OnPush` change detection strategy",
    )
    .with_help("Remove `changeDetection` and make sure the component works with OnPush.")
    .with_label(span)
}

fn redundant_on_push_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "`changeDetection: ChangeDetectionStrategy.OnPush` is redundant because `ChangeDetectionStrategy.OnPush` is the default change detection strategy",
    )
    .with_help("Remove `changeDetection`.")
    .with_label(span)
}

#[derive(Debug, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PreferOnPushComponentChangeDetectionConfig {
    /// Whether to allow a component to set `changeDetection: ChangeDetectionStrategy.OnPush`
    /// explicitly even though it is now the default.
    allow_explicit_on_push: bool,
}

impl Default for PreferOnPushComponentChangeDetectionConfig {
    fn default() -> Self {
        Self { allow_explicit_on_push: true }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct PreferOnPushComponentChangeDetection(PreferOnPushComponentChangeDetectionConfig);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports Angular components that opt out of the default `ChangeDetectionStrategy.OnPush`
    /// change detection strategy, by setting `changeDetection` to `ChangeDetectionStrategy.Eager`
    /// or the deprecated `ChangeDetectionStrategy.Default`. With `allowExplicitOnPush: false`, it
    /// also reports and removes a redundant `changeDetection: ChangeDetectionStrategy.OnPush`.
    ///
    /// ### Why is this bad?
    ///
    /// Since Angular v22, OnPush is the default. Eager change detection checks a component on
    /// every change detection cycle, which costs performance. Switching a component to OnPush can
    /// change its runtime behaviour, so the removal is offered as a suggestion only.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({ changeDetection: ChangeDetectionStrategy.Eager })
    /// class Test {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class Test {}
    /// ```
    PreferOnPushComponentChangeDetection,
    angular,
    perf,
    fix_suggestion,
    config = PreferOnPushComponentChangeDetectionConfig,
    version = "next",
    short_description = "Ensure components do not opt out of the default `ChangeDetectionStrategy.OnPush`.",
);

impl Rule for PreferOnPushComponentChangeDetection {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        let Some(prop) = class_decorator(class, "Component")
            .and_then(decorator_metadata)
            .and_then(|metadata| metadata_property(metadata, "changeDetection"))
        else {
            return;
        };
        let Expression::StaticMemberExpression(member) = &prop.value else { return };
        if !member.object.is_specific_id("ChangeDetectionStrategy") {
            return;
        }

        if member.property.name != "OnPush" {
            ctx.diagnostic_with_suggestion(
                prefer_on_push_diagnostic(member.property.span),
                |fixer| remove_change_detection(fixer, ctx, prop),
            );
        } else if !self.0.allow_explicit_on_push {
            ctx.diagnostic_with_fix(redundant_on_push_diagnostic(member.property.span), |fixer| {
                remove_change_detection(fixer, ctx, prop)
            });
        }
    }
}

// Removes the property and the `ChangeDetectionStrategy` import, which upstream assumes unused.
fn remove_change_detection<'a>(
    fixer: RuleFixer<'_, 'a>,
    ctx: &LintContext<'a>,
    prop: &ObjectProperty<'a>,
) -> RuleFix {
    let fixer = fixer.for_multifix();
    let mut fix = fixer.new_fix_with_capacity(2);
    fix.push(fixer.delete_range(list_item_removal_span(ctx.source_text(), prop.span)));
    if let Some(span) =
        named_import_removal_span(ctx.nodes().program(), "@angular/core", "ChangeDetectionStrategy")
    {
        fix.push(fixer.delete_range(span));
    }
    fix.with_message(
        "Remove `changeDetection` to use the default (`ChangeDetectionStrategy.OnPush`)",
    )
}

#[test]
fn test() {
    use crate::tester::Tester;
    use serde_json::json;

    let pass = vec![
        ("class Test {}", None),
        ("const options = {};\n@Component(options)\nclass Test {}", None),
        ("@Component()\nclass Test {}", None),
        ("@Component({})\nclass Test {}", None),
        ("@Component({ selector: 'app-test' })\nclass Test {}", None),
        ("@Component({ changeDetection: undefined })\nclass Test {}", None),
        (
            "@Component({\n  changeDetection: ChangeDetectionStrategy.OnPush,\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  [`changeDetection`]: ChangeDetectionStrategy.OnPush,\n})\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": true }])),
        ),
        ("@Component({\n  'changeDetection': changeDetection,\n})\nclass Test {}", None),
        (
            "const changeDetection = ChangeDetectionStrategy.Default;\n@Component({\n  changeDetection,\n})\nclass Test {}",
            None,
        ),
        (
            "function changeDetection() {\n  return ChangeDetectionStrategy.OnPush;\n}\n\n@Component({\n  ['changeDetection']: changeDetection(),\n})\nclass Test {}",
            None,
        ),
        ("@NgModule({\n  bootstrap: [Foo]\n})\nclass Test {}", None),
    ];

    // Upstream `~` marker lines stay as empty lines in both the source and the fixed output.
    let fail = vec![
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.Default })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ 'changeDetection': ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ ['changeDetection']: ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ [`changeDetection`]: ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.Eager, selector: 'app-test' })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ selector: 'app-test', changeDetection: ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": true }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.OnPush })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ 'changeDetection': ChangeDetectionStrategy.OnPush })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ [`changeDetection`]: ChangeDetectionStrategy.OnPush })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.OnPush, selector: 'app-test' })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ selector: 'app-test', changeDetection: ChangeDetectionStrategy.OnPush })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "@Component({ changeDetection: ChangeDetectionStrategy.OnPush })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({\n  changeDetection: ChangeDetectionStrategy.OnPush,\n\n  // keep this comment\n  selector: 'app-test',\n})\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
    ];

    let fix = vec![
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            "\n@Component({  })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.Default })\n\nclass Test {}",
            "\n@Component({  })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ 'changeDetection': ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            "\n@Component({  })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ ['changeDetection']: ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            "\n@Component({  })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ [`changeDetection`]: ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            "\n@Component({  })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.Eager, selector: 'app-test' })\n\nclass Test {}",
            "\n@Component({ selector: 'app-test' })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ selector: 'app-test', changeDetection: ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            "\n@Component({ selector: 'app-test', })\n\nclass Test {}",
            None,
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.Eager })\n\nclass Test {}",
            "\n@Component({  })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": true }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.OnPush })\n\nclass Test {}",
            "\n@Component({  })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ 'changeDetection': ChangeDetectionStrategy.OnPush })\n\nclass Test {}",
            "\n@Component({  })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ [`changeDetection`]: ChangeDetectionStrategy.OnPush })\n\nclass Test {}",
            "\n@Component({  })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ changeDetection: ChangeDetectionStrategy.OnPush, selector: 'app-test' })\n\nclass Test {}",
            "\n@Component({ selector: 'app-test' })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({ selector: 'app-test', changeDetection: ChangeDetectionStrategy.OnPush })\n\nclass Test {}",
            "\n@Component({ selector: 'app-test', })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "@Component({ changeDetection: ChangeDetectionStrategy.OnPush })\n\nclass Test {}",
            "@Component({  })\n\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
        (
            "import { ChangeDetectionStrategy } from '@angular/core';\n@Component({\n  changeDetection: ChangeDetectionStrategy.OnPush,\n\n  // keep this comment\n  selector: 'app-test',\n})\nclass Test {}",
            "\n@Component({\n  // keep this comment\n  selector: 'app-test',\n})\nclass Test {}",
            Some(json!([{ "allowExplicitOnPush": false }])),
        ),
    ];

    Tester::new(
        PreferOnPushComponentChangeDetection::NAME,
        PreferOnPushComponentChangeDetection::PLUGIN,
        pass,
        fail,
    )
    .change_rule_path_extension("ts")
    .expect_fix(fix)
    .test_and_snapshot();
}
