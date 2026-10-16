use oxc_ast::{AstKind, ast::Expression};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};

use crate::{
    AstNode,
    context::LintContext,
    rule::Rule,
    utils::{decorator_call_name, enclosing_class_decorator, literal_text},
};

fn relative_url_prefix_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("The ./ and ../ prefix is standard syntax for relative URLs.")
        .with_help("Prefix the URL with `./` or `../`.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct RelativeUrlPrefix;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the `templateUrl` and `styleUrls` of an Angular `@Component` to start with `./`
    /// or `../`.
    ///
    /// ### Why is this bad?
    ///
    /// The `./` and `../` prefix is standard syntax for relative URLs. Angular currently resolves
    /// URLs without it, but code should not depend on that.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({
    ///   templateUrl: 'foobar.html',
    ///   styleUrls: ['foobar.css'],
    /// })
    /// class Test {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({
    ///   templateUrl: './foobar.html',
    ///   styleUrls: ['../shared.css'],
    /// })
    /// class Test {}
    /// ```
    RelativeUrlPrefix,
    angular,
    style,
    none,
    version = "next",
    short_description = "Require the `./` or `../` prefix on component template and style URLs.",
);

impl Rule for RelativeUrlPrefix {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::ObjectProperty(prop) = node.kind() else { return };
        let is_template_url = prop.key.is_specific_id("templateUrl");
        if !is_template_url && !prop.key.is_specific_id("styleUrls") {
            return;
        }
        if enclosing_class_decorator(node, ctx).and_then(decorator_call_name) != Some("Component") {
            return;
        }

        if is_template_url {
            if !is_relative_url(&prop.value) {
                ctx.diagnostic(relative_url_prefix_diagnostic(prop.value.span()));
            }
            return;
        }

        let Expression::ArrayExpression(array) = &prop.value else { return };
        for element in &array.elements {
            // Spread elements and non-string values are reported, as upstream does.
            if !element.is_elision() && !element.as_expression().is_some_and(is_relative_url) {
                ctx.diagnostic(relative_url_prefix_diagnostic(element.span()));
            }
        }
    }
}

// Upstream `/^\.\.?\/.+/`.
fn is_relative_url(expr: &Expression) -> bool {
    literal_text(expr).is_some_and(|url| {
        url.strip_prefix("./")
            .or_else(|| url.strip_prefix("../"))
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
    })
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        r"
    @Component({
      styleUrls: [
        './foo.css',
        '../bar.css',
        '../../baz.scss',
        '../../../baz.sass',
        './../test.css',
        '.././angular.sass'
      ]
    })
    class Test {}
    ",
        r"
    @Component({
      templateUrl: './foobar.html'
    })
    class Test {}
    ",
        r"
    @Component({
      templateUrl: '../foobar.html'
    })
    class Test {}
    ",
        r"
    @Component({
      templateUrl: '../../foobar.html'
    })
    class Test {}
    ",
        r"
    @Component({
      templateUrl: '../../../foobar.html'
    })
    class Test {}
    ",
        r"
    @Component({
      templateUrl: './../foobar.html'
    })
    class Test {}
    ",
        r"
    @Component({
      templateUrl: '.././foobar.html'
    })
    class Test {}
    ",
        r"
    @Component({
      templateUrl: `../foobar.html`,
      styleUrls: [
        `.././foobar.css`,
      ]
    })
    class Test {}
    ",
    ];

    let fail = vec![
        r"
        @Component({
          styleUrls: ['./foo.css', 'bar.css', '../baz.scss', '../../test.css']

        })
        class Test {}
      ",
        r"
        @Component({
          templateUrl: 'foobar.html'

        })
        class Test {}
      ",
    ];

    Tester::new(RelativeUrlPrefix::NAME, RelativeUrlPrefix::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .test_and_snapshot();
}
