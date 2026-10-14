use cow_utils::CowUtils;
use oxc_allocator::Allocator;
use oxc_angular_compiler::{
    ast::{
        html::HtmlNode,
        r3::{
            R3BoundAttribute, R3BoundEvent, R3Element, R3Reference, R3Template, R3TemplateAttr,
            R3TextAttribute, R3Visitor, visit_all,
        },
    },
    parser::{ParseTemplateOptions, html::HtmlParser},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{AngularTemplate, angular_binding_name},
};

fn attributes_order_diagnostic(expected: &str, actual: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "The element's attributes/bindings did not match the expected order: expected {expected} instead of {actual}"
    ))
    .with_label(span)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum OrderType {
    /// `*ngIf="..."`
    StructuralDirective,
    /// `#ref`
    #[serde(rename = "TEMPLATE_REFERENCE")]
    TemplateReferenceVariable,
    /// `class="..."`
    AttributeBinding,
    /// `[value]="..."`
    InputBinding,
    /// `[(value)]="..."`
    TwoWayBinding,
    /// `(click)="..."`
    OutputBinding,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AttributesOrder(Box<AttributesOrderConfig>);

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct AttributesOrderConfig {
    /// Sort attributes of the same kind alphabetically.
    alphabetical: bool,
    /// The order of the kinds of attributes.
    order: Vec<OrderType>,
}

impl Default for AttributesOrderConfig {
    fn default() -> Self {
        Self {
            alphabetical: false,
            order: vec![
                OrderType::StructuralDirective,
                OrderType::TemplateReferenceVariable,
                OrderType::AttributeBinding,
                OrderType::InputBinding,
                OrderType::TwoWayBinding,
                OrderType::OutputBinding,
            ],
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that HTML attributes and Angular bindings are sorted in an expected order: by
    /// default structural directives, template references, attributes, inputs, two-way
    /// bindings, then outputs.
    ///
    /// ### Why is this bad?
    ///
    /// A consistent order makes it quicker to see what an element is, receives, and emits.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <input (input)="search()" [value]="query" class="search" #box>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <input #box class="search" [value]="query" (input)="search()">
    /// ```
    AttributesOrder,
    angular_template,
    style,
    fix,
    config = AttributesOrder,
    version = "next",
    short_description = "Ensures that HTML attributes and Angular bindings are sorted based on an expected order",
);

impl Rule for AttributesOrder {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { rule: self, ctx, template, structural: None }, template.nodes);
        }
    }
}

struct Attribute<'a> {
    /// The whole attribute in the source, as in `[value]="query"`.
    span: Span,
    /// The name without binding syntax, as in `value`.
    name: &'a str,
    order: OrderType,
}

impl Attribute<'_> {
    fn message_name(&self) -> String {
        let name = self.name;
        match self.order {
            OrderType::StructuralDirective => format!("`*{name}`"),
            OrderType::TemplateReferenceVariable => format!("`#{name}`"),
            // An `i18n-x` attribute sorts with the input `x` it translates, but keeps its name.
            OrderType::InputBinding if name.starts_with("i18n-") => format!("`{name}`"),
            OrderType::InputBinding => format!("`[{name}]`"),
            OrderType::OutputBinding => format!("`({name})`"),
            OrderType::TwoWayBinding => format!("`[({name})]`"),
            OrderType::AttributeBinding => format!("`{name}`"),
        }
    }
}

struct Visitor<'c, 'a> {
    rule: &'c AttributesOrder,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
    /// The structural directive written on the element being visited, as `*ngFor="..."`.
    structural: Option<Attribute<'a>>,
}

impl<'a> Visitor<'_, 'a> {
    fn text(&self, span: Span) -> &'a str {
        self.ctx.source_range(self.template.span(span))
    }

    fn bare_name(&self, key_span: Span) -> &'a str {
        angular_binding_name(self.ctx, self.template, key_span)
    }

    fn check(&self, attributes: &[Attribute<'a>]) {
        if attributes.len() < 2 {
            return;
        }
        let order_index = |a: &Attribute| {
            self.rule.0.order.iter().position(|o| *o == a.order).unwrap_or(usize::MAX)
        };
        let mut by_location: Vec<&Attribute> = attributes.iter().collect();
        by_location.sort_by_key(|a| a.span.start);
        let mut expected: Vec<&Attribute> = attributes.iter().collect();
        expected.sort_by(|a, b| {
            order_index(a).cmp(&order_index(b)).then_with(|| {
                if !self.rule.0.alphabetical {
                    return std::cmp::Ordering::Equal;
                }
                let a_name = a.name.strip_prefix("i18n-").unwrap_or(a.name);
                let b_name = b.name.strip_prefix("i18n-").unwrap_or(b.name);
                if a_name == b_name {
                    // An `i18n-` attribute goes right after the attribute it translates.
                    if a.name.starts_with("i18n-") {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Less
                    }
                } else {
                    a_name.cmp(b_name)
                }
            })
        });

        let mismatched: Vec<usize> = (0..by_location.len())
            .filter(|&i| !std::ptr::eq(by_location[i], expected[i]))
            .collect();
        let (Some(&first), Some(&last)) = (mismatched.first(), mismatched.last()) else { return };

        let span = Span::new(by_location[first].span.start, by_location[last].span.end);
        let names = |attributes: &[&Attribute]| {
            attributes.iter().map(|a| a.message_name()).collect::<Vec<_>>().join(", ")
        };
        let diagnostic = attributes_order_diagnostic(
            &names(&expected[first..=last]),
            &names(&by_location[first..=last]),
            span,
        );
        let mut replacement = String::new();
        let mut last_end = span.start;
        for i in first..=last {
            replacement
                .push_str(self.ctx.source_range(Span::new(last_end, by_location[i].span.start)));
            replacement.push_str(self.ctx.source_range(expected[i].span));
            last_end = by_location[i].span.end;
        }
        self.ctx.diagnostic_with_fix(diagnostic, |fixer| fixer.replace(span, replacement));
    }

    /// Attributes in the order `@angular-eslint` collects them, which decides ties.
    fn bound_attributes(
        &self,
        inputs: &[R3BoundAttribute<'a>],
        outputs: &[R3BoundEvent<'a>],
        text_attributes: &[R3TextAttribute<'a>],
        references: &[R3Reference<'a>],
    ) -> Vec<Attribute<'a>> {
        // An input and an output on the same span are the two halves of `[(x)]` or `bindon-x`.
        // Pairing before the `[` check keeps the two halves from becoming two attributes.
        let is_two_way = |input: &R3BoundAttribute| {
            outputs.iter().any(|output| output.source_span == input.source_span)
        };
        let mut two_way = vec![];
        let mut input_bindings = vec![];
        let mut attribute_bindings = vec![];
        for input in inputs {
            let attribute = Attribute {
                span: self.template.span(input.source_span),
                name: self.bare_name(input.key_span),
                order: OrderType::InputBinding,
            };
            if is_two_way(input) {
                two_way.push(Attribute { order: OrderType::TwoWayBinding, ..attribute });
            // Interpolated attributes are inputs, but look like attributes.
            } else if self.text(input.source_span).starts_with('[') {
                input_bindings.push(attribute);
            } else {
                attribute_bindings
                    .push(Attribute { order: OrderType::AttributeBinding, ..attribute });
            }
        }
        attribute_bindings.extend(text_attributes.iter().map(|attr| Attribute {
            span: self.template.span(attr.source_span),
            name: attr.name.as_str(),
            order: OrderType::AttributeBinding,
        }));
        let references = references.iter().map(|reference| Attribute {
            span: self.template.span(reference.source_span),
            name: reference.name.as_str(),
            order: OrderType::TemplateReferenceVariable,
        });
        let outputs = outputs
            .iter()
            .filter(|output| !inputs.iter().any(|input| input.source_span == output.source_span))
            .map(|output| Attribute {
                span: self.template.span(output.source_span),
                name: self.bare_name(output.key_span),
                order: OrderType::OutputBinding,
            });
        attribute_bindings
            .into_iter()
            .chain(references)
            .chain(two_way)
            .chain(input_bindings)
            .chain(outputs)
            .collect()
    }

    /// Attributes as the start tag spells them. `@angular-eslint` reads them this way when
    /// Angular i18n is used, because the template AST has no spans for `i18n` attributes.
    fn html_attributes(
        &self,
        start: Span,
        end: Option<Span>,
        inputs: &[R3BoundAttribute<'a>],
    ) -> Vec<Attribute<'a>> {
        let start = self.template.span(start);
        let mut html = self.ctx.source_range(start).to_string();
        if let Some(end) = end {
            html.push_str(self.ctx.source_range(self.template.span(end)));
        }
        let allocator = Allocator::default();
        let options = ParseTemplateOptions { preserve_line_endings: true, ..Default::default() };
        let parsed = HtmlParser::with_options(&allocator, &html, "", &options).parse();
        let Some(HtmlNode::Element(element)) =
            parsed.nodes.iter().find(|node| matches!(node, HtmlNode::Element(_)))
        else {
            return vec![];
        };
        let shift = |span: Span| Span::new(span.start + start.start, span.end + start.start);
        element
            .attrs
            .iter()
            .map(|attr| {
                let span = shift(attr.span);
                let raw = self.ctx.source_range(shift(attr.name_span));
                let (order, name) = if let Some(name) = raw.strip_prefix('#') {
                    (OrderType::TemplateReferenceVariable, name)
                } else if let Some(name) = raw.strip_prefix('*') {
                    (OrderType::StructuralDirective, name)
                } else if let Some(name) = raw.strip_prefix("[(").and_then(|n| n.strip_suffix(")]"))
                {
                    (OrderType::TwoWayBinding, name)
                } else if let Some(name) = raw.strip_prefix('[').and_then(|n| n.strip_suffix(']')) {
                    (OrderType::InputBinding, name)
                } else if let Some(name) = raw.strip_prefix('(').and_then(|n| n.strip_suffix(')')) {
                    (OrderType::OutputBinding, name)
                } else if raw
                    .strip_prefix("i18n-")
                    .is_some_and(|name| inputs.iter().any(|input| input.name == name))
                {
                    (OrderType::InputBinding, raw)
                } else {
                    (OrderType::AttributeBinding, raw)
                };
                Attribute { span, name, order }
            })
            .collect()
    }

    /// The first structural directive attribute, spanning `*name` and its value.
    fn structural_directive(&self, template: &R3Template<'a>) -> Option<Attribute<'a>> {
        let (name, key_span) = match template.template_attrs.first()? {
            R3TemplateAttr::Bound(attr) => (attr.name.as_str(), attr.key_span),
            R3TemplateAttr::Text(attr) => (attr.name.as_str(), attr.key_span?),
        };
        let key = self.template.span(key_span);
        let source = self.ctx.source_text();
        let rest = &source[key.end as usize..];
        #[expect(clippy::cast_possible_truncation)]
        let value_len = rest.strip_prefix('=').map_or(0, |value| {
            let quoted = value.chars().next().filter(|c| matches!(c, '"' | '\''));
            let len = match quoted {
                Some(quote) => value[1..].find(quote).map_or(value.len(), |end| end + 2),
                None => value.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(value.len()),
            };
            len + 1
        }) as u32;
        Some(Attribute {
            span: Span::new(key.start - 1, key.end + value_len),
            name,
            order: OrderType::StructuralDirective,
        })
    }
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        let structural = self.structural.take();
        let has_i18n = element.i18n.is_some()
            || element.inputs.iter().any(|input| input.i18n.is_some())
            || element.attributes.iter().any(|attr| attr.i18n.is_some());
        let attributes = if has_i18n {
            self.html_attributes(
                element.start_source_span,
                element.end_source_span,
                &element.inputs,
            )
        } else {
            let mut attributes = self.bound_attributes(
                &element.inputs,
                &element.outputs,
                &element.attributes,
                &element.references,
            );
            attributes.extend(structural);
            attributes
        };
        self.check(&attributes);
        self.visit_element_children(element);
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        let is_ng_template = template.tag_name.is_some_and(|name| {
            let name = name.cow_to_ascii_lowercase();
            name == "ng-template" || name == ":svg:ng-template"
        });
        if !is_ng_template {
            // A structural directive's template: its element is checked instead.
            self.structural = self.structural_directive(template);
            visit_all(self, &template.children);
            self.structural = None;
            return;
        }
        // `@angular-eslint` ignores a structural directive written on `<ng-template>` itself.
        self.structural = None;
        let has_i18n = template.i18n.is_some()
            || template.inputs.iter().any(|input| input.i18n.is_some())
            || template.attributes.iter().any(|attr| attr.i18n.is_some());
        if has_i18n {
            let attributes = self.html_attributes(
                template.start_source_span,
                template.end_source_span,
                &template.inputs,
            );
            self.check(&attributes);
            visit_all(self, &template.children);
            return;
        }
        let mut attributes: Vec<Attribute<'a>> = template
            .variables
            .iter()
            .map(|variable| Attribute {
                span: self.template.span(variable.source_span),
                // Spelled `let-x`, as `@angular-eslint` sorts it.
                name: self.text(Span::new(variable.source_span.start, variable.key_span.end)),
                order: OrderType::AttributeBinding,
            })
            .collect();
        attributes.extend(self.bound_attributes(
            &template.inputs,
            &template.outputs,
            &template.attributes,
            &template.references,
        ));
        self.check(&attributes);
        visit_all(self, &template.children);
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        (
            r#"<input #box class="search" [value]="query" [(ngModel)]="model" (input)="search()">"#,
            None,
        ),
        (r#"<div *ngIf="x" #r class="a" [b]="c" (d)="e()"></div>"#, None),
        (
            r#"<ng-container *ngFor="let item of items; index as i; trackBy: trackByFn" [class.x]="y"></ng-container>"#,
            None,
        ),
        (r#"<img title="{{ t }}" alt="a">"#, None),
        (r#"<p *ngIf class="a"></p>"#, None),
        (r#"<ng-template let-item [ngIf]="x" (click)="y()"></ng-template>"#, None),
        (r#"<a href="x" class="y"></a>"#, None),
        (r#"<a class="y" href="x"></a>"#, Some(json!([{ "alphabetical": true }]))),
        (
            r#"<ng-template #template i18n="@@a:b" let-foo>The value is {{ foo }}.</ng-template>"#,
            None,
        ),
        (r#"<ng-template title="{{t}}" class="a"></ng-template>"#, None),
        (r#"<ng-template href="x" let-a></ng-template>"#, Some(json!([{ "alphabetical": true }]))),
        // A structural directive on `<ng-template>` is not one of its children's attributes.
        (
            r#"<ng-template *ngIf="c"><div (click)="a()" class="b"></div></ng-template>"#,
            Some(
                json!([{ "order": ["OUTPUT_BINDING", "ATTRIBUTE_BINDING", "STRUCTURAL_DIRECTIVE", "TEMPLATE_REFERENCE", "INPUT_BINDING", "TWO_WAY_BINDING"] }]),
            ),
        ),
    ];

    let fail = vec![
        (r#"<input bindon-ngModel="v" #box>"#, None),
        (r#"<ng-template [(x)]="y" #t></ng-template>"#, None),
        (
            r#"<ng-template let-b href="x" let-a></ng-template>"#,
            Some(json!([{ "alphabetical": true }])),
        ),
        (r#"<input (input)="search()" [value]="query" class="search" #box>"#, None),
        (r#"<div class="a" *ngIf="x"></div>"#, None),
        (r#"<div [b]="c" *ngFor="let x of xs" class="a"></div>"#, None),
        (r#"<div (click)="x()" [(ngModel)]="m"></div>"#, None),
        (r#"<img alt="a" title="{{ t }}">"#, None),
        (r#"<a href="x" class="y"></a>"#, Some(json!([{ "alphabetical": true }]))),
        // Angular i18n: `i18n-beta` sorts with the input `[beta]`.
        (
            r#"<div [beta]="'b'" alpha="a" [gamma]="g" i18n-alpha="a18" i18n-beta="b18"></div>"#,
            Some(json!([{ "alphabetical": true }])),
        ),
        (
            r#"<ng-template i18n="@@a:b" let-foo #template>The value is {{ foo }}.</ng-template>"#,
            None,
        ),
        (
            r#"<div [b]="c" (d)="e()" class="a"></div>"#,
            Some(
                json!([{ "order": ["OUTPUT_BINDING", "INPUT_BINDING", "ATTRIBUTE_BINDING", "TEMPLATE_REFERENCE", "STRUCTURAL_DIRECTIVE", "TWO_WAY_BINDING"] }]),
            ),
        ),
    ];

    let fix = vec![
        (r#"<input bindon-ngModel="v" #box>"#, r#"<input #box bindon-ngModel="v">"#, None),
        (
            r#"<ng-template [(x)]="y" #t></ng-template>"#,
            r#"<ng-template #t [(x)]="y"></ng-template>"#,
            None,
        ),
        (
            r#"<input (input)="search()" [value]="query" class="search" #box>"#,
            r#"<input #box class="search" [value]="query" (input)="search()">"#,
            None,
        ),
        (r#"<div class="a" *ngIf="x"></div>"#, r#"<div *ngIf="x" class="a"></div>"#, None),
        (
            r#"<div [b]="c" *ngFor="let x of xs" class="a"></div>"#,
            r#"<div *ngFor="let x of xs" class="a" [b]="c"></div>"#,
            None,
        ),
        (
            r#"<div (click)="x()" [(ngModel)]="m"></div>"#,
            r#"<div [(ngModel)]="m" (click)="x()"></div>"#,
            None,
        ),
        (
            r#"<a href="x" class="y"></a>"#,
            r#"<a class="y" href="x"></a>"#,
            Some(json!([{ "alphabetical": true }])),
        ),
    ];

    Tester::new(AttributesOrder::NAME, AttributesOrder::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .expect_fix(fix)
        .test_and_snapshot();
}
