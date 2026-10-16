use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{decorator_call_name, decorator_metadata, list_item_removal_span, metadata_property},
};

fn prefer_standalone_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Components, Directives and Pipes should not opt out of standalone.")
        .with_help(
            "Following this guide is highly recommended: https://angular.dev/reference/migrations/standalone",
        )
        .with_label(span)
}

fn redundant_standalone_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("`standalone: true` is redundant because `standalone: true` is the default")
        .with_help("Remove `standalone: true`.")
        .with_label(span)
}

#[derive(Debug, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PreferStandaloneConfig {
    /// Whether to allow a Component, Directive or Pipe to set `standalone: true` explicitly even
    /// though it is now the default.
    allow_explicit_standalone: bool,
}

impl Default for PreferStandaloneConfig {
    fn default() -> Self {
        Self { allow_explicit_standalone: true }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct PreferStandalone(PreferStandaloneConfig);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports Angular Components, Directives and Pipes that opt out of standalone with any
    /// `standalone` value other than `true`. With `allowExplicitStandalone: false`, it also
    /// reports and removes a redundant `standalone: true`.
    ///
    /// ### Why is this bad?
    ///
    /// Standalone is the default since Angular v19. Opting out ties the class to the
    /// NgModule-based architecture and makes a later migration harder.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({ standalone: false })
    /// class Test {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class Test {}
    /// ```
    PreferStandalone,
    angular,
    style,
    fix_suggestion,
    config = PreferStandaloneConfig,
    version = "next",
    short_description = "Ensure Components, Directives and Pipes do not opt out of standalone.",
);

impl Rule for PreferStandalone {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if !class.is_declaration() {
            return;
        }
        for decorator in &class.decorators {
            if !matches!(decorator_call_name(decorator), Some("Component" | "Directive" | "Pipe")) {
                continue;
            }
            let Some(metadata) = decorator_metadata(decorator) else { continue };
            let Some(prop) = metadata_property(metadata, "standalone") else { continue };

            if matches!(&prop.value, Expression::BooleanLiteral(lit) if lit.value) {
                if self.0.allow_explicit_standalone {
                    continue;
                }
                ctx.diagnostic_with_fix(redundant_standalone_diagnostic(prop.span), |fixer| {
                    fixer.delete_range(list_item_removal_span(ctx.source_text(), prop.span))
                });
                continue;
            }

            ctx.diagnostic_with_suggestion(prefer_standalone_diagnostic(prop.span), |fixer| {
                // Only trivia and an optional comma sit between the property and the object end.
                let end = ctx
                    .find_next_token_within(prop.span.end, metadata.span.end, ",")
                    .map_or(prop.span.end, |offset| prop.span.end + offset + 1);
                fixer
                    .delete_range(Span::new(prop.span.start, end))
                    .with_message("Remove `standalone: false`")
            });
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;
    use serde_json::json;

    let pass = vec![
        ("@Component({})\nclass Test {}", None),
        ("@Component({\n  standalone: true,\n})\nclass Test {}", None),
        (
            "@Component({\n  standalone: true,\n})\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": true }])),
        ),
        (
            "@Component({\n  selector: 'test-selector',\n})\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        ("@Component({\n  selector: 'test-selector'\n})\nclass Test {}", None),
        ("@Component({\n  standalone: true,\n  selector: 'test-selector'\n})\nclass Test {}", None),
        (
            "@Component({\n  selector: 'test-selector',\n  template: '<div></div>',\n  styleUrls: ['./test.css']\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  selector: 'test-selector',\n  standalone: true,\n  template: '<div></div>',\n  styleUrls: ['./test.css']\n})\nclass Test {}",
            None,
        ),
        ("@Directive({})\nclass Test {}", None),
        ("@Directive({\n  standalone: true,\n})\nclass Test {}", None),
        (
            "@Directive({\n  selector: 'test-selector',\n})\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        ("@Directive({\n  selector: 'test-selector'\n})\nclass Test {}", None),
        ("@Directive({\n  standalone: true,\n  selector: 'test-selector'\n})\nclass Test {}", None),
        ("@Directive({\n  selector: 'test-selector',\n  providers: []\n})\nclass Test {}", None),
        (
            "@Directive({\n  selector: 'test-selector',\n  standalone: true,\n  providers: []\n})\nclass Test {}",
            None,
        ),
        ("@Directive()\nabstract class Test {}", None),
        ("@Pipe({})\nclass Test {}", None),
        ("@Pipe({\n  standalone: true,\n})\nclass Test {}", None),
        (
            "@Pipe({\n  name: 'test-pipe',\n})\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        ("@Pipe({\n  name: 'test-pipe'\n})\nclass Test {}", None),
        ("@Pipe({\n  standalone: true,\n  name: 'test-pipe'\n})\nclass Test {}", None),
        ("@Pipe({\n  name: 'my-pipe',\n  pure: true\n})\nclass Test {}", None),
        ("@Pipe({\n  name: 'my-pipe',\n  standalone: true,\n  pure: true\n})\nclass Test {}", None),
    ];

    // Upstream `~` marker lines stay as empty lines in both the source and the fixed output.
    let fail = vec![
        ("@Component({ standalone: false })\n\nclass Test {}", None),
        (
            "@Component({\n  standalone: false,\n\n  template: '<div></div>'\n})\nclass Test {}",
            None,
        ),
        ("@Directive({ standalone: false })\n\nclass Test {}", None),
        ("@Directive({\n  standalone: false,\n\n  selector: 'x-selector'\n})\nclass Test {}", None),
        ("@Pipe({ standalone: false })\n\nclass Test {}", None),
        ("@Pipe({\n  standalone: false,\n\n  name: 'pipe-name'\n})\nclass Test {}", None),
        (
            "@Component({ standalone: false })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Component({ standalone: true })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Component({ standalone: true, selector: 'app-test' })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Component({ selector: 'app-test', standalone: true })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Component({\n  standalone: true,\n\n  // keep this comment\n  selector: 'app-test',\n})\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Directive({ standalone: true, selector: '[appTest]' })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Pipe({ standalone: true, name: 'testPipe' })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
    ];

    let fix = vec![
        (
            "@Component({ standalone: false })\n\nclass Test {}",
            "@Component({  })\n\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  standalone: false,\n\n  template: '<div></div>'\n})\nclass Test {}",
            "@Component({\n  \n\n  template: '<div></div>'\n})\nclass Test {}",
            None,
        ),
        (
            "@Directive({ standalone: false })\n\nclass Test {}",
            "@Directive({  })\n\nclass Test {}",
            None,
        ),
        (
            "@Directive({\n  standalone: false,\n\n  selector: 'x-selector'\n})\nclass Test {}",
            "@Directive({\n  \n\n  selector: 'x-selector'\n})\nclass Test {}",
            None,
        ),
        ("@Pipe({ standalone: false })\n\nclass Test {}", "@Pipe({  })\n\nclass Test {}", None),
        (
            "@Pipe({\n  standalone: false,\n\n  name: 'pipe-name'\n})\nclass Test {}",
            "@Pipe({\n  \n\n  name: 'pipe-name'\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({ standalone: false })\n\nclass Test {}",
            "@Component({  })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Component({ standalone: true })\n\nclass Test {}",
            "@Component({  })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Component({ standalone: true, selector: 'app-test' })\n\nclass Test {}",
            "@Component({ selector: 'app-test' })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Component({ selector: 'app-test', standalone: true })\n\nclass Test {}",
            "@Component({ selector: 'app-test', })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Component({\n  standalone: true,\n\n  // keep this comment\n  selector: 'app-test',\n})\nclass Test {}",
            "@Component({\n  // keep this comment\n  selector: 'app-test',\n})\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Directive({ standalone: true, selector: '[appTest]' })\n\nclass Test {}",
            "@Directive({ selector: '[appTest]' })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
        (
            "@Pipe({ standalone: true, name: 'testPipe' })\n\nclass Test {}",
            "@Pipe({ name: 'testPipe' })\n\nclass Test {}",
            Some(json!([{ "allowExplicitStandalone": false }])),
        ),
    ];

    Tester::new(PreferStandalone::NAME, PreferStandalone::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
