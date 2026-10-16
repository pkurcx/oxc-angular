use oxc_ast::{AstKind, ast::Expression};
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

fn consistent_component_styles_diagnostic(message: &'static str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(message).with_label(span)
}

#[derive(Debug, Default, Clone, Copy, JsonSchema, Deserialize)]
#[serde(rename_all = "lowercase")]
enum StylesMode {
    /// A single stylesheet is a plain string: `styles: '...'` and `styleUrl: '...'`.
    #[default]
    String,
    /// Styles are always arrays: `styles: ['...']` and `styleUrls: ['...']`.
    Array,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct ConsistentComponentStyles(StylesMode);

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces consistent usage of `styles`, `styleUrls` and `styleUrl` in Angular `@Component`
    /// metadata. With `"string"` (the default), a single stylesheet must be written as a string
    /// in `styles` or `styleUrl`; with `"array"`, `styles` must be an array and `styleUrls` used
    /// instead of `styleUrl`.
    ///
    /// ### Why is this bad?
    ///
    /// Mixing the string and array forms across components makes the codebase harder to scan.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule with the default `"string"` option:
    /// ```ts
    /// @Component({
    ///   styles: [':host { display: block; }'],
    ///   styleUrls: ['./test.component.css'],
    /// })
    /// class Test {}
    /// ```
    ///
    /// Examples of **correct** code for this rule with the default `"string"` option:
    /// ```ts
    /// @Component({
    ///   styles: ':host { display: block; }',
    ///   styleUrl: './test.component.css',
    /// })
    /// class Test {}
    /// ```
    ConsistentComponentStyles,
    angular,
    style,
    fix,
    config = StylesMode,
    version = "next",
    short_description = "Enforce consistent usage of `styles`, `styleUrls` and `styleUrl` in `@Component`.",
);

impl Rule for ConsistentComponentStyles {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::ObjectProperty(prop) = node.kind() else { return };
        let is_component = || {
            enclosing_class_decorator(node, ctx).and_then(decorator_call_name) == Some("Component")
        };

        match self.0 {
            StylesMode::String => {
                let is_styles = prop.key.is_specific_static_name("styles");
                if !is_styles && !prop.key.is_specific_static_name("styleUrls") {
                    return;
                }
                let Expression::ArrayExpression(array) = &prop.value else { return };
                let [element] = &array.elements[..] else { return };
                let Some(element) = element.as_expression().filter(|el| is_literal_or_template(el))
                else {
                    return;
                };
                if !is_component() {
                    return;
                }
                if is_styles {
                    ctx.diagnostic_with_fix(
                        consistent_component_styles_diagnostic(
                            "Use a `string` instead of a `string[]` for the `styles` property",
                            array.span,
                        ),
                        |fixer| fixer.replace_with(&array.span, element),
                    );
                } else {
                    let text = format!("styleUrl: {}", ctx.source_range(element.span()));
                    ctx.diagnostic_with_fix(
                        consistent_component_styles_diagnostic(
                            "Use `styleUrl` instead of `styleUrls` for a single stylesheet",
                            prop.span,
                        ),
                        |fixer| fixer.replace(prop.span, text),
                    );
                }
            }
            StylesMode::Array => {
                let is_styles = prop.key.is_specific_static_name("styles");
                if !is_styles && !prop.key.is_specific_static_name("styleUrl") {
                    return;
                }
                if !is_literal_or_template(&prop.value) || !is_component() {
                    return;
                }
                let value_text = ctx.source_range(prop.value.span());
                if is_styles {
                    let span = prop.value.span();
                    let text = format!("[{value_text}]");
                    ctx.diagnostic_with_fix(
                        consistent_component_styles_diagnostic(
                            "Use a `string[]` instead of a `string` for the `styles` property",
                            span,
                        ),
                        |fixer| fixer.replace(span, text),
                    );
                } else {
                    let text = format!("styleUrls: [{value_text}]");
                    ctx.diagnostic_with_fix(
                        consistent_component_styles_diagnostic(
                            "Use `styleUrls` instead of `styleUrl`",
                            prop.span,
                        ),
                        |fixer| fixer.replace(prop.span, text),
                    );
                }
            }
        }
    }
}

fn is_literal_or_template(expr: &Expression) -> bool {
    expr.is_literal() || matches!(expr, Expression::TemplateLiteral(_))
}

#[test]
fn test() {
    use crate::tester::Tester;
    use serde_json::json;

    let pass = vec![
        // Default.
        ("@Component({\n  styles: ':host { display: block; }',\n})\nclass Test {}", None),
        ("@Component({\n  styles: `\n    :host { display: block; }\n  `,\n})\nclass Test {}", None),
        (
            "@Component({\n  selector: 'my-test',\n  standalone: true,\n  imports: [CommonModule],\n  styles: `\n    :host { display: block; }\n  `,\n  providers: [FooService]\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  styles: [\n    ':host { display: block; }',\n    `.foo { color: red; }`\n  ],\n})\nclass Test {}",
            None,
        ),
        ("@Component({\n  styleUrl: `./test.component.css`,\n})\nclass Test {}", None),
        (
            "@Component({\n  styleUrls: [\n    '../shared.css',\n    `./test.component.css`\n  ],\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  selector: 'my-test',\n  standalone: true,\n  imports: [CommonModule],\n  styleUrls: [\n    '../shared.css',\n    `./test.component.css`\n  ],\n  providers: [FooService]\n})\nclass Test {}",
            None,
        ),
        // String.
        (
            "@Component({\n  styles: ':host { display: block; }',\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        (
            "@Component({\n  styles: `\n    :host { display: block; }\n  `,\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        (
            "@Component({\n  selector: 'my-test',\n  standalone: true,\n  imports: [CommonModule],\n  styles: `\n    :host { display: block; }\n  `,\n  providers: [FooService]\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        (
            "@Component({\n  styles: [\n    ':host { display: block; }',\n    `.foo { color: red; }`\n  ],\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        (
            "@Component({\n  styleUrl: `./test.component.css`,\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        (
            "@Component({\n  styleUrls: [\n    '../shared.css',\n    `./test.component.css`\n  ],\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        (
            "@Component({\n  selector: 'my-test',\n  standalone: true,\n  imports: [CommonModule],\n  styleUrls: [\n    '../shared.css',\n    `./test.component.css`\n  ],\n  providers: [FooService]\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        // Array.
        (
            "@Component({\n  styles: [':host { display: block; }'],\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styles: [\n    `\n      :host { display: block; }\n    `\n  ],\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  selector: 'my-test',\n  standalone: true,\n  imports: [CommonModule],\n  styles: [\n    `\n      :host { display: block; }\n    `\n  ],\n  providers: [FooService]\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styles: [\n    ':host { display: block; }',\n    `.foo { color: red; }`\n  ],\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styleUrls: [`./test.component.css`],\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styleUrls: [\n    '../shared.css',\n    `./test.component.css`\n  ],\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  selector: 'my-test',\n  standalone: true,\n  imports: [CommonModule],\n  styleUrls: [\n    '../shared.css',\n    `./test.component.css`\n  ],\n  providers: [FooService]\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
    ];

    // Upstream `~` marker lines stay as empty lines in both the source and the fixed output.
    let fail = vec![
        // Default.
        ("@Component({\n  styles: [':host { display: block; }']\n\n})\nclass Test {}", None),
        (
            "@Component({\n  standalone: true,\n  imports: [MatButtonModule],\n  styles: [':host { display: block; }'],\n\n  providers: []\n})\nclass Test {}",
            None,
        ),
        ("@Component({\n  styleUrls: ['./test.component.css']\n\n})\nclass Test {}", None),
        (
            "@Component({\n  standalone: true,\n  imports: [MatButtonModule],\n  styleUrls: ['./test.component.css'],\n\n  providers: []\n})\nclass Test {}",
            None,
        ),
        (
            "const type = 'block';\n@Component({\n  styles: [`:host\\t{ display: ${type}; }`]\n\n})\nclass Test {}",
            None,
        ),
        ("@Component({\n  styles: [':host\\t{ display: block; }']\n\n})\nclass Test {}", None),
        ("@Component({\n  styles: [':host{ display: block; }']\n\n})\nclass Test {}", None),
        ("@Component({\n  styles: [\":host{ display: block; }\"]\n\n})\nclass Test {}", None),
        ("@Component({\n  styles: [`:host{ display: block; }`]\n\n})\nclass Test {}", None),
        ("@Component({\n  styleUrls: [`./test.component.css`]\n\n})\nclass Test {}", None),
        // String.
        (
            "@Component({\n  styles: [':host { display: block; }']\n\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        (
            "@Component({\n  styleUrls: ['./test.component.css']\n\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        // Array.
        (
            "@Component({\n  styles: ':host { display: block; }'\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  standalone: true,\n  imports: [MatButtonModule],\n  styles: ':host { display: block; }',\n\n  providers: []\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styleUrl: './test.component.css',\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "const type = 'block';\n@Component({\n  styles: `:host\\t{ display: ${type}; }`\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styles: ':host\\t{ display: block; }'\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styles: ':host{ display: block; }'\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styles: \":host{ display: block; }\"\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styles: `:host{ display: block; }`\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
    ];

    let fix = vec![
        // Default.
        (
            "@Component({\n  styles: [':host { display: block; }']\n\n})\nclass Test {}",
            "@Component({\n  styles: ':host { display: block; }'\n\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  standalone: true,\n  imports: [MatButtonModule],\n  styles: [':host { display: block; }'],\n\n  providers: []\n})\nclass Test {}",
            "@Component({\n  standalone: true,\n  imports: [MatButtonModule],\n  styles: ':host { display: block; }',\n\n  providers: []\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  styleUrls: ['./test.component.css']\n\n})\nclass Test {}",
            "@Component({\n  styleUrl: './test.component.css'\n\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  standalone: true,\n  imports: [MatButtonModule],\n  styleUrls: ['./test.component.css'],\n\n  providers: []\n})\nclass Test {}",
            "@Component({\n  standalone: true,\n  imports: [MatButtonModule],\n  styleUrl: './test.component.css',\n\n  providers: []\n})\nclass Test {}",
            None,
        ),
        (
            "const type = 'block';\n@Component({\n  styles: [`:host\\t{ display: ${type}; }`]\n\n})\nclass Test {}",
            "const type = 'block';\n@Component({\n  styles: `:host\\t{ display: ${type}; }`\n\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  styles: [':host\\t{ display: block; }']\n\n})\nclass Test {}",
            "@Component({\n  styles: ':host\\t{ display: block; }'\n\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  styles: [':host{ display: block; }']\n\n})\nclass Test {}",
            "@Component({\n  styles: ':host{ display: block; }'\n\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  styles: [\":host{ display: block; }\"]\n\n})\nclass Test {}",
            "@Component({\n  styles: \":host{ display: block; }\"\n\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  styles: [`:host{ display: block; }`]\n\n})\nclass Test {}",
            "@Component({\n  styles: `:host{ display: block; }`\n\n})\nclass Test {}",
            None,
        ),
        (
            "@Component({\n  styleUrls: [`./test.component.css`]\n\n})\nclass Test {}",
            "@Component({\n  styleUrl: `./test.component.css`\n\n})\nclass Test {}",
            None,
        ),
        // String.
        (
            "@Component({\n  styles: [':host { display: block; }']\n\n})\nclass Test {}",
            "@Component({\n  styles: ':host { display: block; }'\n\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        (
            "@Component({\n  styleUrls: ['./test.component.css']\n\n})\nclass Test {}",
            "@Component({\n  styleUrl: './test.component.css'\n\n})\nclass Test {}",
            Some(json!(["string"])),
        ),
        // Array.
        (
            "@Component({\n  styles: ':host { display: block; }'\n\n})\nclass Test {}",
            "@Component({\n  styles: [':host { display: block; }']\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  standalone: true,\n  imports: [MatButtonModule],\n  styles: ':host { display: block; }',\n\n  providers: []\n})\nclass Test {}",
            "@Component({\n  standalone: true,\n  imports: [MatButtonModule],\n  styles: [':host { display: block; }'],\n\n  providers: []\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styleUrl: './test.component.css',\n\n})\nclass Test {}",
            "@Component({\n  styleUrls: ['./test.component.css'],\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "const type = 'block';\n@Component({\n  styles: `:host\\t{ display: ${type}; }`\n\n})\nclass Test {}",
            "const type = 'block';\n@Component({\n  styles: [`:host\\t{ display: ${type}; }`]\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styles: ':host\\t{ display: block; }'\n\n})\nclass Test {}",
            "@Component({\n  styles: [':host\\t{ display: block; }']\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styles: ':host{ display: block; }'\n\n})\nclass Test {}",
            "@Component({\n  styles: [':host{ display: block; }']\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styles: \":host{ display: block; }\"\n\n})\nclass Test {}",
            "@Component({\n  styles: [\":host{ display: block; }\"]\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
        (
            "@Component({\n  styles: `:host{ display: block; }`\n\n})\nclass Test {}",
            "@Component({\n  styles: [`:host{ display: block; }`]\n\n})\nclass Test {}",
            Some(json!(["array"])),
        ),
    ];

    Tester::new(ConsistentComponentStyles::NAME, ConsistentComponentStyles::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
