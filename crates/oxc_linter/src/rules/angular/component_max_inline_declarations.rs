use oxc_ast::{
    AstKind,
    ast::{ArrayExpressionElement, Expression},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    AstNode,
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{decorator_call_name, enclosing_class_decorator},
};

fn component_max_inline_declarations_diagnostic(
    span: Span,
    property: &str,
    line_count: usize,
    max: usize,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "`{property}` has too many lines ({line_count}). Maximum allowed is {max}"
    ))
    .with_help(format!("Move the inline `{property}` to a separate file."))
    .with_label(span)
}

#[derive(Debug, Clone, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ComponentMaxInlineDeclarationsConfig {
    /// Maximum number of lines in an inline `template`.
    template: usize,
    /// Maximum number of lines in inline `styles`, summed over all entries.
    styles: usize,
    /// Maximum number of lines in an inline `animations` array, brackets excluded.
    animations: usize,
}

impl Default for ComponentMaxInlineDeclarationsConfig {
    fn default() -> Self {
        Self { template: 3, styles: 3, animations: 15 }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct ComponentMaxInlineDeclarations(Box<ComponentMaxInlineDeclarationsConfig>);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces a maximum number of lines in the inline `template`, `styles` and `animations` of
    /// an Angular `@Component`.
    ///
    /// ### Why is this bad?
    ///
    /// Long inline templates and styles make a component hard to read, and get weaker editor
    /// support than separate `.html` and `.css` files.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({
    ///   template: `
    ///     <div>first line</div>
    ///     <div>second line</div>
    ///     <div>third line</div>
    ///     <div>fourth line</div>
    ///   `,
    /// })
    /// class Test {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({
    ///   templateUrl: './test.component.html',
    /// })
    /// class Test {}
    /// ```
    ComponentMaxInlineDeclarations,
    angular,
    pedantic,
    none,
    config = ComponentMaxInlineDeclarationsConfig,
    version = "next",
    short_description = "Enforce a maximum number of lines in inline template, styles and animations.",
);

impl Rule for ComponentMaxInlineDeclarations {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::ObjectProperty(prop) = node.kind() else { return };
        let Some(property) = ["template", "styles", "animations"]
            .into_iter()
            .find(|name| prop.key.is_specific_id(*name))
        else {
            return;
        };
        if enclosing_class_decorator(node, ctx).and_then(decorator_call_name) != Some("Component") {
            return;
        }

        let (line_count, max) = match property {
            "template" => (lines_count(&prop.value, ctx), self.0.template),
            "styles" => {
                let line_count = match &prop.value {
                    Expression::ArrayExpression(array) => array
                        .elements
                        .iter()
                        .filter_map(ArrayExpressionElement::as_expression)
                        .map(|element| lines_count(element, ctx))
                        .sum(),
                    value => lines_count(value, ctx),
                };
                (line_count, self.0.styles)
            }
            _ => {
                let Expression::ArrayExpression(array) = &prop.value else { return };
                if array.elements.is_empty() {
                    return;
                }
                // The lines between the brackets, at least one.
                let line_count = line_breaks(ctx.source_range(array.span)).saturating_sub(2).max(1);
                (line_count, self.0.animations)
            }
        };

        if line_count > max {
            ctx.diagnostic(component_max_inline_declarations_diagnostic(
                prop.value.span(),
                property,
                line_count,
                max,
            ));
        }
    }
}

// Upstream counts a literal's raw source but only the first quasi of a template literal.
fn lines_count(expr: &Expression, ctx: &LintContext) -> usize {
    let text = match expr {
        Expression::TemplateLiteral(template) => {
            template.quasis.first().map_or("", |quasi| quasi.value.raw.as_str())
        }
        _ if expr.is_literal() => ctx.source_range(expr.span()),
        _ => return 0,
    };
    line_breaks(text.trim()) + 1
}

// `\r\n`, `\r` and `\n` each count as one break, as in upstream's `/\r\n|\r|\n/`.
fn line_breaks(text: &str) -> usize {
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .filter(|&(i, &b)| b == b'\n' || (b == b'\r' && bytes.get(i + 1) != Some(&b'\n')))
        .count()
}

#[test]
fn test() {
    use crate::tester::Tester;
    use serde_json::json;

    let pass = vec![
        (
            r"
    @Component({
      template: '<div>just one line template</div>'
    })
    class Test {}
    ",
            None,
        ),
        (
            r"
    @Component({
      styles: ['div { display: none; }']
    })
    class Test {}
    ",
            None,
        ),
        (
            r"
    @Component({
      styles: 'div { display: none; }'
    })
    class Test {}
    ",
            None,
        ),
        (
            r"
    @Component({
      animations: [state('void', style({opacity: 0, transform: 'scale(1, 0)'}))]
    })
    class Test {}
    ",
            None,
        ),
        (
            r"
    @Component({
      styleUrls: ['./foobar.scss'],
      templateUrl: './foobar.html',
    })
    class Test {}
    ",
            None,
        ),
        (
            r"
    @Component({
      animations: [
        state('void', style({opacity: 0, transform: 'scale(1, 0)'}))
      ],
      templateUrl: './foobar.html',
    })
    class Test {}
    ",
            None,
        ),
    ];

    // Each upstream `~` marker line stays as an empty line; the animations counts depend on it.
    let fail = vec![
        (
            r"
      @Component({
        template: `

          <div>first line</div>
          <div>second line</div>
          <div>third line</div>
          <div>fourth line</div>
        `

      })
      class Test {}
      ",
            None,
        ),
        (
            r"
      @Component({
        template: '<div>first line</div>'

      })
      class Test {}
      ",
            Some(json!([{ "template": 0 }])),
        ),
        (
            r"
      @Component({
        styles: [

          `
            div {
              display: block;
              height: 40px;
            }
          `
        ]

      })
      class Test {}
      ",
            None,
        ),
        (
            r"
      @Component({
        styles: `

          div {
            display: block;
            height: 40px;
          }
        `

      })
      class Test {}
      ",
            None,
        ),
        (
            r"
      @Component({
        styles: [

          `
            div {
              display: block;
            }
          `,
          `
            span {
              width: 30px;
            }
          `
        ]

      })
      class Test {}
      ",
            None,
        ),
        (
            r"
      @Component({
        styles: ['div { display: none; }']

      })
      class Test {}
      ",
            Some(json!([{ "styles": 0 }])),
        ),
        (
            r"
      @Component({
        animations: [{

          transformPanelWrap: trigger('transformPanelWrap', [
            transition('* => void', query('@transformPanel', [animateChild()], {optional: true})),
          ]),
          transformPanel: trigger('transformPanel', [
            state('void', style({
              transform: 'scaleY(0.8)',
              minWidth: '100%',
              opacity: 0
            })),
            state('showing', style({
              opacity: 1,
              minWidth: 'calc(100% + 32px)',
              transform: 'scaleY(1)'
            })),
            state('next', style({height: '0px', visibility: 'hidden'}))
          ])
        }]

      })
      class Test {}
      ",
            None,
        ),
        (
            r"
      @Component({
        animations: [

          trigger('dialogContainer', [
            transition('* => void', query('@transformPanel', [animateChild()], {optional: true}))
          ]),
          trigger('transformPanel', [
            state('void', style({
              transform: 'scaleY(0.8)',
              minWidth: '100%',
              opacity: 0
            })),
            state('showing', style({
              opacity: 1,
              minWidth: 'calc(100% + 32px)',
              transform: 'scaleY(1)'
            }))
          ]),
          trigger('transformPanel', [
            state('void', style({opacity: 0, transform: 'scale(1, 0)'}))
          ])
        ]

      })
      class Test {}
      ",
            None,
        ),
        (
            r"
      @Component({
        animations: [{

          transformPanel: trigger('transformPanel', [
            state('void', style({opacity: 0, transform: 'scale(1, 0)'}))
          ])
        }]

      })
      class Test {}
      ",
            Some(json!([{ "animations": 2 }])),
        ),
    ];

    Tester::new(
        ComponentMaxInlineDeclarations::NAME,
        ComponentMaxInlineDeclarations::PLUGIN,
        pass,
        fail,
    )
    .change_rule_path_extension("ts")
    .test_and_snapshot();
}
