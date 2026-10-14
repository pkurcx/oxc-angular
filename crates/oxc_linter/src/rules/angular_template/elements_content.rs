use cow_utils::CowUtils;
use oxc_angular_compiler::ast::r3::{R3Element, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    context::LintContext,
    rule::{DefaultRuleConfig, Rule},
    utils::{AngularTemplate, is_angular_element_hidden},
};

fn elements_content_diagnostic(element: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("<{element}> should have content")).with_label(span)
}

const SAFELIST: [&str; 7] =
    ["aria-label", "innerHtml", "innerHTML", "innerText", "outerHTML", "textContent", "title"];

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ElementsContent(Box<ElementsContentConfig>);

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct ElementsContentConfig {
    /// Attributes and inputs that provide content, in addition to `aria-label`, `innerHtml`,
    /// `innerHTML`, `innerText`, `outerHTML`, `textContent` and `title`.
    allow_list: Vec<String>,
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that headings, anchors and buttons have content, or an attribute that provides it.
    ///
    /// ### Why is this bad?
    ///
    /// Screen readers announce an empty heading, link or button without a name, so users cannot
    /// tell what it is for.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <button></button>
    /// <h1></h1>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <button>Save</button>
    /// <a [innerText]="label"></a>
    /// ```
    ElementsContent,
    angular_template,
    correctness,
    config = ElementsContent,
    version = "next",
    short_description = "Ensures that the heading, anchor and button elements have content in them",
);

impl Rule for ElementsContent {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(
                &mut Visitor { rule: self, ctx, template, hidden_ancestors: 0 },
                template.nodes,
            );
        }
    }
}

struct Visitor<'c, 'a> {
    rule: &'c ElementsContent,
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
    hidden_ancestors: u32,
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        let hidden = is_angular_element_hidden(self.ctx, self.template, element);
        let name = element.name.cow_to_ascii_lowercase();
        let checked =
            matches!(name.as_ref(), "a" | "button" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
        if checked && element.children.is_empty() && !hidden && self.hidden_ancestors == 0 {
            let names = element.attributes.iter().map(|attr| attr.name.as_str());
            let mut names = names.chain(element.inputs.iter().map(|input| input.name.as_str()));
            let safelisted = names.any(|name| {
                SAFELIST.contains(&name)
                    || self.rule.0.allow_list.iter().any(|allowed| allowed == name)
            });
            if !safelisted {
                let span = self.template.span(element.source_span);
                self.ctx.diagnostic(elements_content_diagnostic(&element.name, span));
            }
        }
        self.hidden_ancestors += u32::from(hidden);
        self.visit_element_children(element);
        self.hidden_ancestors -= u32::from(hidden);
    }
}

#[test]
fn test() {
    use serde_json::json;

    use crate::tester::Tester;

    let pass = vec![
        ("<h1>Heading</h1>", None),
        ("<a>Anchor</a>", None),
        ("<button>{{ label }}</button>", None),
        ("<button><span>x</span></button>", None),
        ("<button> </button>", None),
        (r#"<button [innerText]="label"></button>"#, None),
        (r#"<a [attr.aria-label]="label"></a>"#, None),
        (r#"<h2 title="x"></h2>"#, None),
        (r#"<button aria-hidden="true"></button>"#, None),
        ("<div hidden><button></button></div>", None),
        (r#"<button style="display: none"></button>"#, None),
        (r#"<button [style.display]="'none'"></button>"#, None),
        ("<button appTooltip></button>", Some(json!([{ "allowList": ["appTooltip"] }]))),
        ("<div></div>", None),
    ];

    let fail = vec![
        ("<h1></h1>", None),
        ("<a></a>", None),
        ("<button></button>", None),
        (r#"<H6 class="x"></H6>"#, None),
        (r#"<button [attr.aria-hidden]="hidden"></button>"#, None),
        ("<button appTooltip></button>", None),
    ];

    Tester::new(ElementsContent::NAME, ElementsContent::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();
}
