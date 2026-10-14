use oxc_angular_compiler::ast::{
    expression::{AngularExpression, BindingType},
    r3::{R3BoundAttribute, R3Element, R3Visitor, visit_all},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::AngularTemplate,
};

fn no_inline_styles_diagnostic(element: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "<{element}/> element should not have inline styles via style attribute. Please use classes instead."
    ))
    .with_label(span)
}

/// `true` or `false`, or `"dynamic"` to allow only bindings whose value is not a constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
enum AllowOption {
    Bool(bool),
    Mode(AllowMode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum AllowMode {
    Dynamic,
}

impl Default for AllowOption {
    fn default() -> Self {
        Self::Bool(false)
    }
}

impl AllowOption {
    fn allows(self, value: &AngularExpression) -> bool {
        match self {
            Self::Bool(allowed) => allowed,
            Self::Mode(AllowMode::Dynamic) => !is_constant_expression(value),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NoInlineStyles {
    /// Whether `[ngStyle]` bindings are allowed. `"dynamic"` allows them only when the bound
    /// value is not a constant: `{ color: textColor }` is allowed, `{ color: 'red' }` is not.
    allow_ng_style: AllowOption,
    /// Whether `[style]`, `[style.property]` and `style="{{ }}"` bindings are allowed.
    /// `"dynamic"` allows them only when the bound value is not a constant:
    /// `[style.width.px]="column.width"` is allowed, `[style.color]="'red'"` is not.
    allow_bind_to_style: AllowOption,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows inline styles in templates: the `style` attribute, `[attr.style]`, `[style]`,
    /// `[style.property]` and `[ngStyle]` bindings.
    ///
    /// ### Why is this bad?
    ///
    /// Inline styles are hard to keep consistent and reuse, and can violate a Content Security
    /// Policy. Styles belong in stylesheets, applied with classes.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div style="color: red"></div>
    /// <div [style.color]="'red'"></div>
    /// <div [ngStyle]="{ color: textColor }"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div class="warning"></div>
    /// <div [class.warning]="isWarning"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule with `{ "allowBindToStyle": "dynamic" }`:
    /// ```html
    /// <div [style.width.px]="column.width"></div>
    /// ```
    ///
    /// ### Differences compared to `@angular-eslint`
    ///
    /// - `[style.x]` and `[attr.style]` are also reported on elements with a structural directive
    ///   such as `*ngIf`, which `@angular-eslint` misses.
    NoInlineStyles,
    angular_template,
    restriction,
    config = NoInlineStyles,
    version = "next",
    short_description = "Disallows the use of inline styles in HTML templates",
);

impl Rule for NoInlineStyles {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { rule: self, ctx, template }, template.nodes);
        }
    }
}

impl NoInlineStyles {
    fn is_disallowed(&self, input: &R3BoundAttribute) -> bool {
        match input.binding_type {
            BindingType::Attribute => input.name == "style",
            BindingType::Style => !self.allow_bind_to_style.allows(&input.value),
            BindingType::Property if input.name == "style" => {
                !self.allow_bind_to_style.allows(&input.value)
            }
            BindingType::Property if input.name == "ngStyle" => {
                !self.allow_ng_style.allows(&input.value)
            }
            _ => false,
        }
    }
}

struct Visitor<'c, 'a> {
    rule: &'c NoInlineStyles,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        if element.attributes.iter().any(|attr| attr.name == "style")
            || element.inputs.iter().any(|input| self.rule.is_disallowed(input))
        {
            let span = self.template.span(element.source_span);
            self.ctx.diagnostic(no_inline_styles_diagnostic(&element.name, span));
        }
        self.visit_element_children(element);
    }
}

/// Whether a value is built only from literals and pure operators, so it never changes.
fn is_constant_expression(expression: &AngularExpression) -> bool {
    match expression {
        AngularExpression::LiteralPrimitive(_) | AngularExpression::Empty(_) => true,
        AngularExpression::LiteralMap(map) => map.values.iter().all(is_constant_expression),
        AngularExpression::LiteralArray(array) => {
            array.expressions.iter().all(is_constant_expression)
        }
        AngularExpression::Interpolation(e) => e.expressions.iter().all(is_constant_expression),
        AngularExpression::TemplateLiteral(e) => e.expressions.iter().all(is_constant_expression),
        AngularExpression::ParenthesizedExpression(e) => is_constant_expression(&e.expression),
        AngularExpression::Unary(e) => is_constant_expression(&e.expr),
        AngularExpression::Binary(e) => {
            is_constant_expression(&e.left) && is_constant_expression(&e.right)
        }
        AngularExpression::Conditional(e) => {
            is_constant_expression(&e.condition)
                && is_constant_expression(&e.true_exp)
                && is_constant_expression(&e.false_exp)
        }
        AngularExpression::PrefixNot(e) => is_constant_expression(&e.expression),
        AngularExpression::TypeofExpression(e) => is_constant_expression(&e.expression),
        AngularExpression::VoidExpression(e) => is_constant_expression(&e.expression),
        AngularExpression::NonNullAssert(e) => is_constant_expression(&e.expression),
        _ => false,
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        (r#"<img alt="Foo eating a sandwich.">"#, None),
        (r#"<img src="foo" [attr.alt]="altText">"#, None),
        (r#"<img src="foo" [attr.alt]="'Alt Text'">"#, None),
        (r#"<img src="foo" alt="">"#, None),
        (r#"<object aria-label="foo">"#, None),
        (r#"<object aria-labelledby="id1">"#, None),
        ("<object>Meaningful description</object>", None),
        (r#"<object title="An object">"#, None),
        (r#"<area aria-label="foo" />"#, None),
        (r#"<area aria-labelledby="id1" />"#, None),
        (r#"<area alt="This is descriptive!" />"#, None),
        (r#"<input type="text">"#, None),
        (r#"<input type="image" alt="This is descriptive!">"#, None),
        (r#"<input type="image" aria-label="foo">"#, None),
        (r#"<input type="image" aria-labelledby="id1">"#, None),
        (r#"<div [attr.stylesheet]="x">"#, None),
        (
            r#"<input [ngStyle]="{ 'background-color': '#fff' }">"#,
            Some(json!([{ "allowNgStyle": true }])),
        ),
        (
            r#"<input [style.backgroundColor]="'#fff'">"#,
            Some(json!([{ "allowBindToStyle": true }])),
        ),
        (
            r#"<input [style]="{ 'background-color': '#fff' }">"#,
            Some(json!([{ "allowBindToStyle": true }])),
        ),
        (r#"<input [style]="styleObject">"#, Some(json!([{ "allowBindToStyle": true }]))),
        (r#"<input [style.width.px]="w">"#, Some(json!([{ "allowBindToStyle": true }]))),
        (r#"<input style="{{ x }}">"#, Some(json!([{ "allowBindToStyle": true }]))),
        (r#"<input bind-style="x">"#, Some(json!([{ "allowBindToStyle": true }]))),
        (
            r#"<input [ngStyle]="{ 'background-color': 'red' }" [style.background-color]="'#fff'">"#,
            Some(json!([{ "allowNgStyle": true, "allowBindToStyle": true }])),
        ),
        (
            r#"<input [ngStyle]="{ 'background-color': 'red' }" [style]="styleObject" [style.background-color]="'#fff'">"#,
            Some(json!([{ "allowNgStyle": true, "allowBindToStyle": true }])),
        ),
        (
            r#"<div [style.color]="textColor"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.width.px]="column.width"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (r#"<div [style]="styleObject"></div>"#, Some(json!([{ "allowBindToStyle": "dynamic" }]))),
        (
            r#"<div [style]="{ color: textColor, 'background-color': backgroundColor }"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style]="{ color: 'red', 'background-color': backgroundColor }"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.width.px]="getWidth()"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.color]="color$ | async"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.color]="isActive ? 'green' : 'gray'"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.width]="width + 'px'"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.width]="`${width}px`"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.opacity]="-offset"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.top.px]="positions[index]"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.color]="config?.color"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div style="color: {{ textColor }}"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div bind-style="styleObject"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style]="{ ...baseStyles }"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [ngStyle]="{ color: textColor }"></div>"#,
            Some(json!([{ "allowNgStyle": "dynamic" }])),
        ),
        (r#"<div [ngStyle]="styleObject"></div>"#, Some(json!([{ "allowNgStyle": "dynamic" }]))),
        (
            r#"<div [ngStyle]="{ 'font-size.px': fontSize }" [style.color]="textColor"></div>"#,
            Some(json!([{ "allowNgStyle": "dynamic", "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [ngStyle]="{ color: textColor }" [style.color]="'red'"></div>"#,
            Some(json!([{ "allowNgStyle": "dynamic", "allowBindToStyle": true }])),
        ),
        // Only elements are checked, as in `@angular-eslint`.
        (r#"<ng-template [ngIf]="x" style="color: red"><span></span></ng-template>"#, None),
        (r#"<div *ngIf="x" [style.color]="c"></div>"#, Some(json!([{ "allowBindToStyle": true }]))),
    ];

    let fail = vec![
        (
            r#"
        <ng-template>
          <div>
            <img style="position: relative;">
          </div>
        </ng-template>
      "#,
            None,
        ),
        (r#"<object style="padding: 30px;"></object>"#, None),
        (r#"<area style="padding: 30px;" />"#, None),
        (r#"<input style="color: red;">"#, None),
        (r#"<input [attr.style]="'padding: 10px;'">"#, None),
        (r#"<input [style.background-color]="'#fff'">"#, None),
        (
            r#"<input [style]="'border: 1px solid black;'" [ngStyle]="{ 'padding': '10px' }" [style.background-color]="'#fff'">"#,
            Some(json!([{ "allowNgStyle": true }])),
        ),
        (r#"<input [ngStyle]="'background-color: #fff'">"#, None),
        (
            r#"<input [style]="'border: 1px solid black;'">"#,
            Some(json!([{ "allowNgStyle": true, "allowBindToStyle": false }])),
        ),
        (r#"<input [attr.style]="'padding: 10px;'">"#, Some(json!([{ "allowBindToStyle": true }]))),
        (
            r#"<input [ngStyle]="{ 'padding': '10px' }" [style]="'border: 1px solid black;'">"#,
            Some(json!([{ "allowNgStyle": true, "allowBindToStyle": false }])),
        ),
        (r#"<div [style.color]="'red'"></div>"#, Some(json!([{ "allowBindToStyle": "dynamic" }]))),
        (r#"<div [style.width.px]="100"></div>"#, Some(json!([{ "allowBindToStyle": "dynamic" }]))),
        (r#"<div [style]="'color: red'"></div>"#, Some(json!([{ "allowBindToStyle": "dynamic" }]))),
        (
            r#"<div [style]="{ color: 'red', 'background-color': '#fff' }"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.width]="10 + 'px'"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.color]="true ? 'red' : 'blue'"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [style.width]="`100px`"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div style="color: {{ 'red' }}"></div>"#,
            Some(json!([{ "allowBindToStyle": "dynamic" }])),
        ),
        (r#"<div [attr.style]="styles"></div>"#, Some(json!([{ "allowBindToStyle": "dynamic" }]))),
        (
            r#"<div [ngStyle]="{ color: 'red' }"></div>"#,
            Some(json!([{ "allowNgStyle": "dynamic" }])),
        ),
        (r#"<div [ngStyle]="'color: red'"></div>"#, Some(json!([{ "allowNgStyle": "dynamic" }]))),
        (
            r#"<div [ngStyle]="{ color: 'red' }" [style.width.px]="column.width"></div>"#,
            Some(json!([{ "allowNgStyle": "dynamic", "allowBindToStyle": "dynamic" }])),
        ),
        (
            r#"<div [ngStyle]="{ color: 'red' }" [style.color]="'blue'"></div>"#,
            Some(json!([{ "allowNgStyle": true, "allowBindToStyle": "dynamic" }])),
        ),
        (r#"<div *ngIf="x" style="color: red"></div>"#, None),
        // `@angular-eslint` misses this: its parser overwrites the binding type of hoisted inputs.
        (r#"<div *ngIf="x" [style.color]="'red'"></div>"#, None),
        (r#"@if (x) { <p [style.color]="c"></p> }"#, None),
    ];

    Tester::new(NoInlineStyles::NAME, NoInlineStyles::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();

    let inline_pass = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<div class=\"x\"></div>' }) class A {}",
    ];
    let inline_fail = vec![
        "import { Component } from '@angular/core'; @Component({ template: '<div style=\"color: red\"></div>' }) class A {}",
    ];
    Tester::new(NoInlineStyles::NAME, NoInlineStyles::PLUGIN, inline_pass, inline_fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("inline")
        .test_and_snapshot();
}
