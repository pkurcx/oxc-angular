use oxc_ast::{
    AstKind,
    ast::{
        Argument, CallExpression, Decorator, Expression, MethodDefinition, MethodDefinitionKind,
        MethodDefinitionType, ObjectProperty, PropertyDefinition, PropertyDefinitionType,
        PropertyKey,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{decorator_call_name, enclosing_class_decorator, literal_text, metadata_property},
};

fn no_output_native_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(
        "Output bindings, including aliases, should not be named as standard DOM events",
    )
    .with_help("Use a name that describes the component event, such as `itemSelected`.")
    .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoOutputNative;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Disallows output bindings, including aliases, named after standard DOM events such as
    /// `click`, `change` or `blur`.
    ///
    /// ### Why is this bad?
    ///
    /// A `(click)` binding on the host element listens to both the custom output and the native
    /// DOM event, so the handler can run for events the component never emitted.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({})
    /// class Test {
    ///   @Output() change = new EventEmitter<string>();
    ///   click = output();
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({})
    /// class Test {
    ///   @Output() valueChange = new EventEmitter<string>();
    ///   itemClick = output();
    /// }
    /// ```
    NoOutputNative,
    angular,
    correctness,
    version = "next",
    short_description = "Disallow output bindings named after standard DOM events.",
);

impl Rule for NoOutputNative {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        match node.kind() {
            AstKind::ObjectProperty(property) => check_outputs_metadata(node, property, ctx),
            AstKind::PropertyDefinition(property) => check_output_property(property, ctx),
            AstKind::MethodDefinition(method) => check_output_getter(method, ctx),
            _ => {}
        }
    }
}

fn check_name(span: Span, text: &str, ctx: &LintContext) {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    // `'name: alias'` in the `outputs` metadata; other names have no colon.
    let mut names = text.split(':');
    if names.next().is_some_and(is_native_event) || names.next().is_some_and(is_native_event) {
        ctx.diagnostic(no_output_native_diagnostic(span));
    }
}

fn is_native_event(name: &str) -> bool {
    NATIVE_EVENT_NAMES.contains(&name)
}

// Upstream `getNativeEventNames` (MDN browser-compat-data 8.1.4); case-sensitive.
const NATIVE_EVENT_NAMES: &[&str] = &[
    "abort",
    "afterprint",
    "afterscriptexecute",
    "animationcancel",
    "animationend",
    "animationiteration",
    "animationstart",
    "appinstalled",
    "auxclick",
    "beforeinput",
    "beforeinstallprompt",
    "beforematch",
    "beforeprint",
    "beforescriptexecute",
    "beforetoggle",
    "beforeunload",
    "beforexrselect",
    "beginEvent",
    "blur",
    "cancel",
    "canplay",
    "canplaythrough",
    "change",
    "click",
    "close",
    "command",
    "compositionend",
    "compositionstart",
    "compositionupdate",
    "contentvisibilityautostatechange",
    "contextlost",
    "contextmenu",
    "contextrestored",
    "copy",
    "cuechange",
    "cut",
    "dblclick",
    "devicemotion",
    "deviceorientation",
    "deviceorientationabsolute",
    "DOMActivate",
    "DOMContentLoaded",
    "DOMMouseScroll",
    "drag",
    "dragend",
    "dragenter",
    "dragexit",
    "dragleave",
    "dragover",
    "dragstart",
    "drop",
    "durationchange",
    "emptied",
    "encrypted",
    "ended",
    "endEvent",
    "enterpictureinpicture",
    "error",
    "focus",
    "focusin",
    "focusout",
    "formdata",
    "freeze",
    "fullscreenchange",
    "fullscreenerror",
    "gamepadconnected",
    "gamepaddisconnected",
    "gesturechange",
    "gestureend",
    "gesturestart",
    "gotpointercapture",
    "hashchange",
    "input",
    "installresult",
    "interest",
    "invalid",
    "keydown",
    "keypress",
    "keyup",
    "languagechange",
    "leavepictureinpicture",
    "load",
    "loadeddata",
    "loadedmetadata",
    "loadstart",
    "location",
    "loseinterest",
    "lostpointercapture",
    "message",
    "messageerror",
    "mousedown",
    "mouseenter",
    "mouseleave",
    "mousemove",
    "mouseout",
    "mouseover",
    "mouseup",
    "mousewheel",
    "MozMousePixelScroll",
    "offline",
    "online",
    "orientationchange",
    "pagehide",
    "pagereveal",
    "pageshow",
    "pageswap",
    "paste",
    "pause",
    "play",
    "playing",
    "pointercancel",
    "pointerdown",
    "pointerenter",
    "pointerleave",
    "pointerlockchange",
    "pointerlockerror",
    "pointermove",
    "pointerout",
    "pointerover",
    "pointerrawupdate",
    "pointerup",
    "popstate",
    "prerenderingchange",
    "progress",
    "promptaction",
    "promptdismiss",
    "ratechange",
    "readystatechange",
    "rejectionhandled",
    "repeatEvent",
    "reset",
    "resize",
    "resume",
    "scroll",
    "scrollend",
    "scrollsnapchange",
    "scrollsnapchanging",
    "search",
    "securitypolicyviolation",
    "seeked",
    "seeking",
    "select",
    "selectionchange",
    "selectstart",
    "slotchange",
    "stalled",
    "storage",
    "stream",
    "submit",
    "suspend",
    "timeupdate",
    "toggle",
    "touchcancel",
    "touchend",
    "touchmove",
    "touchstart",
    "track",
    "transitioncancel",
    "transitionend",
    "transitionrun",
    "transitionstart",
    "unhandledrejection",
    "unload",
    "validationstatuschange",
    "visibilitychange",
    "volumechange",
    "vrdisplayactivate",
    "vrdisplayconnect",
    "vrdisplaydeactivate",
    "vrdisplaydisconnect",
    "vrdisplaypresentchange",
    "waiting",
    "waitingforkey",
    "webglcontextcreationerror",
    "webglcontextlost",
    "webglcontextrestored",
    "webkitmouseforcechanged",
    "webkitmouseforcedown",
    "webkitmouseforceup",
    "webkitmouseforcewillbegin",
    "wheel",
];

fn check_outputs_metadata<'a>(
    node: &AstNode<'a>,
    property: &ObjectProperty<'a>,
    ctx: &LintContext<'a>,
) {
    let Expression::ArrayExpression(array) = &property.value else { return };
    if !property.key.is_specific_static_name("outputs") {
        return;
    }
    let Some(decorator) = enclosing_class_decorator(node, ctx) else { return };
    if !matches!(decorator_call_name(decorator), Some("Component" | "Directive")) {
        return;
    }
    for element in &array.elements {
        if let Some(text) = element.as_expression().and_then(literal_text) {
            check_name(element.span(), text, ctx);
        }
    }
}

// The name and alias of an `@Output()` property or an `output()` field.
fn check_output_property(property: &PropertyDefinition, ctx: &LintContext) {
    if property.r#type != PropertyDefinitionType::PropertyDefinition {
        return;
    }
    let decorator = output_decorator(&property.decorators);
    let call = match &property.value {
        Some(Expression::CallExpression(call)) if call.callee.is_specific_id("output") => {
            Some(call)
        }
        _ => None,
    };
    if let Some(decorator) = decorator {
        check_decorator_aliases(decorator, ctx);
    }
    if !property.computed
        && (decorator.is_some() || call.is_some())
        && let Some(name) = key_name(&property.key)
    {
        check_name(property.key.span(), name, ctx);
    }
    if let Some(call) = call {
        check_object_aliases(call, ctx);
    }
}

fn check_output_getter(method: &MethodDefinition, ctx: &LintContext) {
    if method.kind != MethodDefinitionKind::Get
        || method.r#type != MethodDefinitionType::MethodDefinition
    {
        return;
    }
    let Some(decorator) = output_decorator(&method.decorators) else { return };
    check_decorator_aliases(decorator, ctx);
    if !method.computed
        && let Some(name) = key_name(&method.key)
    {
        check_name(method.key.span(), name, ctx);
    }
}

fn output_decorator<'b, 'a>(decorators: &'b [Decorator<'a>]) -> Option<&'b Decorator<'a>> {
    decorators.iter().find(|decorator| decorator_call_name(decorator) == Some("Output"))
}

fn key_name<'a>(key: &PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some(ident.name.as_str()),
        PropertyKey::StringLiteral(lit) => Some(lit.value.as_str()),
        _ => None,
    }
}

// `@Output('alias')` and `@Output({ alias: 'alias' })`.
fn check_decorator_aliases(decorator: &Decorator, ctx: &LintContext) {
    let Expression::CallExpression(call) = &decorator.expression else { return };
    for argument in &call.arguments {
        if let Some(text) = argument.as_expression().and_then(literal_text) {
            check_name(argument.span(), text, ctx);
        }
    }
    check_object_aliases(call, ctx);
}

// `{ alias: 'alias' }` passed as an argument.
fn check_object_aliases(call: &CallExpression, ctx: &LintContext) {
    for argument in &call.arguments {
        let Argument::ObjectExpression(object) = argument else { continue };
        if let Some(alias) = metadata_property(object, "alias")
            && let Some(text) = literal_text(&alias.value)
        {
            check_name(alias.value.span(), text, ctx);
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "class Test {}",
        r"
    @Page({
      outputs: ['play', popstate, `online`, 'obsolete: obsol', 'store: storage'],
    })
    class Test {}
    ",
        r"
    @Component()
    class Test {
      change = new EventEmitter();
    }
    ",
        r"
    @Directive()
    class Test {
      @Output() buttonChange = new EventEmitter<'change'>();
    }
    ",
        r"
    @Directive()
    class Test {
      buttonChange = output<'change'>();
    }
    ",
        r"
    @Component()
    class Test {
      @Output() Drag = new EventEmitter<{ click: string }>();
    }
    ",
        r"
    @Component()
    class Test {
      Drag = output<{ click: string }>();
    }
    ",
        r"
    @Directive()
    class Test {
      @Output(`changelower`) changeText = new EventEmitter<{ bar: string, blur: string }>();
    }
    ",
        r"
    @Directive()
    class Test {
      changeText = output<{ bar: string, blur: string }>({ alias: `changelower` });
    }
    ",
        r"
    @Component()
    class Test {
      @Output('buttonChange') changelower = new EventEmitter<ComplextObject>();
    }
    ",
        r"
    @Component()
    class Test {
      changelower = new output<ComplextObject>({ alias: 'buttonChange' });
    }
    ",
        r"
    @Directive()
    class Test<SVGScroll> {
      @Output() SVgZoom = new EventEmitter<SVGScroll>();
    }
    ",
        r"
    @Directive()
    class Test<SVGScroll> {
       SVgZoom = output<SVGScroll>();
    }
    ",
        r"
    const change = 'change';
    @Component()
    class Test {
      @Output(change) touchMove: EventEmitter<{ action: 'click' | 'close' }> = new EventEmitter<{ action: 'click' | 'close' }>();
    }
    ",
        r"
    const change = 'change';
    @Component()
    class Test {
      touchMove = output<{ action: 'click' | 'close' }>({ alias: change });
    }
    ",
        r"
    const blur = 'blur';
    const click = 'click';
    @Directive()
    class Test {
      @Output(blur) [click]: EventEmitter<Blur>;
    }
    ",
        r"
    const blur = 'blur';
    const click = 'click';
    @Directive()
    class Test {
      [click] = output<Blur>({ alias: blur });
    }
    ",
        r"
    @Component({
      selector: 'foo',
      'outputs': [`test: foo`]
    })
    class Test {}
    ",
        r"
    @Directive({
      selector: 'foo',
      ['outputs']: [`test: foo`]
    })
    class Test {}
    ",
        r"
    @Component({
      'selector': 'foo',
      [`outputs`]: [`test: foo`]
    })
    class Test {}
    ",
        r"
    @Directive({
      selector: 'foo',
    })
    class Test {
      @Output() get 'getter'() {}
    }
    ",
    ];

    let fail = vec![
        r"
        @Component({
          'outputs': ['pagehide']
        })
        class Test {}
      ",
        r"
        @Directive({
          inputs: ['abort'],
          ['outputs']: [boundary, `test: copy`],
        })
        class Test {}
      ",
        r"
        @Component({
          inputs: ['abort'],
          [`outputs`]: [boundary, `test: copy`],
        })
        class Test {}
      ",
        r"
        @Directive({
          outputs: ['orientationchange: orientation'],
        })
        class Test {}
      ",
        r"
        @Component()
        class Test {
          @Output() change: EventEmitter<any> = new EventEmitter<{}>();
        }
      ",
        r"
        @Component()
        class Test {
          change = output();
        }
      ",
        r"
        @Directive()
        class Test {
          @Output() @Custom('change') 'change' = new EventEmitter<void>();
        }
      ",
        r"
        @Directive()
        class Test {
          'change' = output();
        }
      ",
        r"
        @Component()
        class Test {
          @Custom() @Output(`change`) _change = getOutput();
        }
      ",
        r"
        @Component()
        class Test {
          _change = output({ alias: `change` });
        }
      ",
        r"
        @Directive()
        class Test {
          @Output('change') _change = (this.subject$ as Subject<{blur: boolean}>).pipe();
        }
      ",
        r"
        @Directive()
        class Test {
          _change = output({ alias: 'change' });
        }
      ",
        r"
        @Component()
        class Test {
          @Output('getter') get 'cut'() {}
        }
      ",
        r"
        @Directive()
        class Test {
          @Output(`devicemotion`) get getter() {}
        }
      ",
        r"
        @Injectable()
        class Test {
          @Output('click') blur = this.getOutput();
        }
      ",
        r"
        @Injectable()
        class Test {
          blur = output({ alias: 'click' });
        }
      ",
    ];

    Tester::new(NoOutputNative::NAME, NoOutputNative::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
