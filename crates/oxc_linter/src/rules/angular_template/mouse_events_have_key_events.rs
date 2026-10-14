use oxc_angular_compiler::ast::r3::{R3BoundEvent, R3Element, R3Template, R3Visitor, visit_all};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{
    context::LintContext,
    rule::Rule,
    utils::{
        AngularTemplate, is_angular_dom_element, visit_element_in_eslint_order,
        visit_template_in_eslint_order,
    },
};

fn mouse_events_have_key_events_diagnostic(
    mouse_event: &str,
    key_event: &str,
    span: Span,
) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "`{mouse_event}` must be accompanied by `{key_event}` for accessibility"
    ))
    .with_help("See https://www.w3.org/WAI/WCAG21/Understanding/keyboard")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct MouseEventsHaveKeyEvents;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Ensures that `(mouseover)` and `(mouseout)` are accompanied by `(focus)` and `(blur)`.
    /// As in `@angular-eslint`, events of descendants count, so an element is also reported
    /// when one of its descendants has the mouse event without the key event.
    ///
    /// ### Why is this bad?
    ///
    /// Keyboard and screen reader users cannot trigger mouse events, so what they reveal must
    /// also be reachable on focus.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```html
    /// <div (mouseover)="show()"></div>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```html
    /// <div (mouseover)="show()" (focus)="show()"></div>
    /// ```
    MouseEventsHaveKeyEvents,
    angular_template,
    correctness,
    version = "next",
    short_description = "Ensures that the mouse events `mouseout` and `mouseover` are accompanied by `focus` and `blur` events respectively.",
);

const PAIRS: [(&str, &str); 2] = [("mouseout", "blur"), ("mouseover", "focus")];

impl Rule for MouseEventsHaveKeyEvents {
    fn run_once(&self, ctx: &LintContext) {
        for template in ctx.angular_templates() {
            visit_all(&mut Visitor { ctx, template }, template.nodes);
        }
    }
}

struct Visitor<'c, 'a> {
    ctx: &'c LintContext<'a>,
    template: &'c AngularTemplate<'a>,
}

impl<'a> R3Visitor<'a> for Visitor<'_, 'a> {
    fn visit_element(&mut self, element: &R3Element<'a>) {
        if is_angular_dom_element(&element.name, false) {
            let mut events = EventNames(vec![]);
            visit_element_in_eslint_order(&mut events, element);
            for (mouse_event, key_event) in PAIRS {
                if events.0.contains(&mouse_event) && !events.0.contains(&key_event) {
                    let span = self.template.span(element.source_span);
                    self.ctx.diagnostic(mouse_events_have_key_events_diagnostic(
                        mouse_event,
                        key_event,
                        span,
                    ));
                }
            }
        }
        self.visit_element_children(element);
    }
}

/// Names of the events bound on an element and its descendants.
struct EventNames<'a>(Vec<&'a str>);

impl<'a> R3Visitor<'a> for EventNames<'a> {
    fn visit_bound_event(&mut self, event: &R3BoundEvent<'a>) {
        self.0.push(event.name.as_str());
    }

    fn visit_element(&mut self, element: &R3Element<'a>) {
        visit_element_in_eslint_order(self, element);
    }

    fn visit_template(&mut self, template: &R3Template<'a>) {
        visit_template_in_eslint_order(self, template);
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r#"<div (mouseover)="onMouseOver()" (focus)="onFocus()"></div>"#,
        r#"<div (mouseout)="onMouseOut()" (blur)="onBlur()"></div>"#,
        r#"<app-item (mouseover)="x()"></app-item>"#,
        r#"<div (click)="x()"></div>"#,
    ];

    let fail = vec![
        r#"<div (mouseover)="onMouseOver()"></div>"#,
        r#"<div (mouseout)="onMouseOut()" (focus)="onFocus()"></div>"#,
        r#"<section><span (mouseover)="x()"></span></section>"#,
        r#"<div (mouseover)="a()" (mouseout)="b()"></div>"#,
    ];

    Tester::new(MouseEventsHaveKeyEvents::NAME, MouseEventsHaveKeyEvents::PLUGIN, pass, fail)
        .change_rule_path_extension("html")
        .test_and_snapshot();
}
