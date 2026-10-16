use std::borrow::Cow;

use oxc_ast::{
    AstKind,
    ast::{ObjectExpression, ObjectProperty, ObjectPropertyKind},
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
    utils::{decorator_call_name, decorator_metadata},
};

fn incorrect_order_diagnostic(span: Span, decorator: &str, expected: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Keys in @{decorator} decorator should be ordered: {expected}"))
        .with_label(span)
}

fn unconfigured_property_diagnostic(span: Span, decorator: &str, property: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!(
        "Property \"{property}\" is not in the configured key order for @{decorator}."
    ))
    .with_label(span)
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct SortKeysInTypeDecorator(Box<SortKeysInTypeDecoratorConfig>);

#[derive(Debug, Clone, JsonSchema, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SortKeysInTypeDecoratorConfig {
    /// Key order for `@Component`.
    #[serde(rename = "Component")]
    component: Vec<String>,
    /// Key order for `@Directive`.
    #[serde(rename = "Directive")]
    directive: Vec<String>,
    /// Key order for `@Injectable`.
    #[serde(rename = "Injectable")]
    injectable: Vec<String>,
    /// Key order for `@NgModule`.
    #[serde(rename = "NgModule")]
    ng_module: Vec<String>,
    /// Key order for `@Pipe`.
    #[serde(rename = "Pipe")]
    pipe: Vec<String>,
    /// When `false`, keys missing from the configured order are reported.
    #[serde(rename = "allowUnconfiguredProperties")]
    allow_unconfigured_properties: bool,
}

impl Default for SortKeysInTypeDecoratorConfig {
    fn default() -> Self {
        let order =
            |keys: &[&str]| -> Vec<String> { keys.iter().map(|key| (*key).to_string()).collect() };
        Self {
            component: order(&[
                "selector",
                "imports",
                "standalone",
                "templateUrl",
                "template",
                "styleUrl",
                "styleUrls",
                "styles",
                "providers",
                "changeDetection",
                "encapsulation",
                "viewProviders",
                "host",
                "hostDirectives",
                "inputs",
                "outputs",
                "animations",
                "schemas",
                "exportAs",
                "queries",
                "preserveWhitespaces",
                "jit",
                "moduleId",
                "interpolation",
            ]),
            directive: order(&[
                "selector",
                "standalone",
                "providers",
                "host",
                "hostDirectives",
                "inputs",
                "outputs",
                "exportAs",
                "queries",
                "jit",
            ]),
            injectable: order(&[
                "providedIn",
                "useClass",
                "useExisting",
                "useFactory",
                "useValue",
                "deps",
            ]),
            ng_module: order(&[
                "id",
                "imports",
                "declarations",
                "providers",
                "exports",
                "bootstrap",
                "schemas",
                "jit",
            ]),
            pipe: order(&["name", "standalone", "pure"]),
            allow_unconfigured_properties: true,
        }
    }
}

impl SortKeysInTypeDecoratorConfig {
    fn order(&self, decorator: &str) -> Option<&[String]> {
        match decorator {
            "Component" => Some(&self.component),
            "Directive" => Some(&self.directive),
            "Injectable" => Some(&self.injectable),
            "NgModule" => Some(&self.ng_module),
            "Pipe" => Some(&self.pipe),
            _ => None,
        }
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces a consistent key order in the metadata of `@Component`, `@Directive`,
    /// `@Injectable`, `@NgModule` and `@Pipe`. Keys missing from the configured order must come
    /// after the configured ones, or are reported when `allowUnconfiguredProperties` is `false`.
    ///
    /// ### Why is this bad?
    ///
    /// When every decorator lists its keys in the same order, readers find `selector`, `imports`
    /// or `providers` without scanning, and diffs touching the metadata stay small.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```ts
    /// @Component({
    ///   templateUrl: './app.component.html',
    ///   selector: 'app-root',
    /// })
    /// class AppComponent {}
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```ts
    /// @Component({
    ///   selector: 'app-root',
    ///   templateUrl: './app.component.html',
    /// })
    /// class AppComponent {}
    /// ```
    SortKeysInTypeDecorator,
    angular,
    style,
    fix,
    config = SortKeysInTypeDecoratorConfig,
    version = "next",
    short_description = "Ensure that keys in type decorators are sorted in a consistent order.",
);

impl Rule for SortKeysInTypeDecorator {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        DefaultRuleConfig::<Self>::from_value(value).map(DefaultRuleConfig::into_inner)
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::Class(class) = node.kind() else { return };
        if !class.is_declaration() {
            return;
        }
        for decorator in &class.decorators {
            let Some(name) = decorator_call_name(decorator) else { continue };
            let Some(order) = self.0.order(name) else { continue };
            let Some(metadata) = decorator_metadata(decorator) else { continue };
            self.check(ctx, name, order, metadata);
        }
    }
}

type NamedProperty<'b, 'a> = (&'b ObjectProperty<'a>, Cow<'a, str>);

impl SortKeysInTypeDecorator {
    fn check<'a>(
        &self,
        ctx: &LintContext<'a>,
        decorator: &str,
        order: &[String],
        metadata: &ObjectExpression<'a>,
    ) {
        let properties: Vec<NamedProperty<'_, 'a>> = metadata
            .properties
            .iter()
            .filter_map(|property| match property {
                ObjectPropertyKind::ObjectProperty(property) => Some(&**property),
                ObjectPropertyKind::SpreadProperty(_) => None,
            })
            .map(|property| (property, key_name(property, ctx)))
            .collect();
        let is_configured = |name: &str| order.iter().any(|key| key == name);

        if !self.0.allow_unconfigured_properties {
            for (property, name) in &properties {
                if !is_configured(name) {
                    ctx.diagnostic(unconfigured_property_diagnostic(
                        property.span,
                        decorator,
                        name,
                    ));
                }
            }
        }

        if properties.len() <= 1 {
            return;
        }

        let first_configured = properties.iter().position(|(_, name)| is_configured(name));
        let first_unconfigured = properties.iter().position(|(_, name)| !is_configured(name));
        if self.0.allow_unconfigured_properties
            && let (Some(configured), Some(unconfigured)) = (first_configured, first_unconfigured)
            && unconfigured < configured
        {
            report_incorrect_order(ctx, decorator, order, &properties, metadata, unconfigured);
            return;
        }

        let configured: Vec<usize> =
            (0..properties.len()).filter(|&i| is_configured(&properties[i].1)).collect();
        let expected: Vec<&str> = order
            .iter()
            .map(String::as_str)
            .filter(|key| configured.iter().any(|&i| properties[i].1 == *key))
            .collect();
        let out_of_order = configured.iter().enumerate().find(|&(position, &i)| {
            expected.get(position).is_none_or(|key| properties[i].1 != *key)
        });
        if let Some((_, &index)) = out_of_order {
            report_incorrect_order(ctx, decorator, order, &properties, metadata, index);
        }
    }
}

fn key_name<'a>(property: &ObjectProperty<'a>, ctx: &LintContext<'a>) -> Cow<'a, str> {
    property
        .key
        .static_name()
        .unwrap_or_else(|| Cow::Borrowed(ctx.source_range(property.key.span())))
}

fn report_incorrect_order<'a>(
    ctx: &LintContext<'a>,
    decorator: &str,
    order: &[String],
    properties: &[NamedProperty<'_, 'a>],
    metadata: &ObjectExpression<'a>,
    index: usize,
) {
    let expected = order
        .iter()
        .filter(|key| properties.iter().any(|(_, name)| name == key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    ctx.diagnostic_with_fix(
        incorrect_order_diagnostic(properties[index].0.span, decorator, &expected),
        |fixer| match sorted_object_text(ctx, order, properties, metadata) {
            Some(text) => fixer.replace(metadata.span, text),
            None => fixer.noop(),
        },
    );
}

// One property per line, each carrying its leading comments and same-line trailing comments.
fn sorted_object_text<'a>(
    ctx: &LintContext<'a>,
    order: &[String],
    properties: &[NamedProperty<'_, 'a>],
    object: &ObjectExpression<'a>,
) -> Option<String> {
    if object
        .properties
        .iter()
        .any(|property| matches!(property, ObjectPropertyKind::SpreadProperty(_)))
    {
        return None;
    }
    let source = ctx.source_text();
    let object_text = ctx.source_range(object.span);
    let indent = object_text
        .split('\n')
        .nth(1)
        .map_or("", |line| &line[..line.len() - line.trim_start().len()]);

    let comments = ctx.comments();
    let mut processed = vec![false; comments.len()];
    let mut leading: Vec<Vec<String>> = Vec::with_capacity(properties.len());
    let mut trailing: Vec<String> = Vec::with_capacity(properties.len());
    let mut previous_end = object.span.start + 1;
    for (property, _) in properties {
        let span = property.span;
        let mut before = vec![];
        for (i, comment) in comments.iter().enumerate() {
            if !processed[i] && comment.span.start > previous_end && comment.span.start < span.start
            {
                before.push(format!("{indent}{}", ctx.source_range(comment.span)));
                processed[i] = true;
            }
        }
        let mut after = String::new();
        for (i, comment) in comments.iter().enumerate() {
            if processed[i] || comment.span.start <= span.end {
                continue;
            }
            let gap = &source[span.end as usize..comment.span.start as usize];
            // Only commas and spaces may separate a trailing comment from its property.
            if gap.chars().all(|c| c == ',' || (c.is_whitespace() && c != '\n')) {
                after.extend(gap.chars().filter(|&c| c != ','));
                after.push_str(ctx.source_range(comment.span));
                processed[i] = true;
            }
        }
        leading.push(before);
        trailing.push(after);
        previous_end = span.end;
    }

    // A comment that is neither attached to a property nor inside one would be dropped.
    let drops_comment = comments.iter().zip(&processed).any(|(comment, &done)| {
        !done
            && object.span.contains_inclusive(comment.span)
            && !properties
                .iter()
                .any(|(property, _)| property.span.contains_inclusive(comment.span))
    });
    if drops_comment {
        return None;
    }

    // Stable, so unconfigured keys keep their order after the configured ones.
    let mut sorted: Vec<usize> = (0..properties.len()).collect();
    sorted.sort_by_key(|&i| {
        order.iter().position(|key| *key == properties[i].1).unwrap_or(usize::MAX)
    });

    let mut lines = vec![];
    for (position, &i) in sorted.iter().enumerate() {
        lines.extend(leading[i].iter().cloned());
        let mut line = format!("{indent}{}", ctx.source_range(properties[i].0.span));
        if position + 1 < sorted.len() {
            line.push(',');
        }
        line.push_str(&trailing[i]);
        lines.push(line);
    }
    // Two characters shallower than the property lines.
    let closing_indent = indent.char_indices().rev().nth(1).map_or("", |(i, _)| &indent[..i]);
    Some(format!("{{\n{}\n{closing_indent}}}", lines.join("\n")))
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        (
            r"
@Component({
  selector: 'app-test',
  imports: [CommonModule],
  standalone: true,
  templateUrl: './test.component.html',
  template: '<div>Test</div>',
  styleUrl: './test.component.css',
  styleUrls: ['./test.component.css'],
  styles: ['div { color: red; }'],
  providers: [TestService],
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  viewProviders: [ViewService],
  host: { '[class.test]': 'true' },
  hostDirectives: [TestDirective],
  inputs: ['value'],
  outputs: ['change'],
  animations: [trigger('test', [])],
  schemas: [CUSTOM_ELEMENTS_SCHEMA],
  exportAs: 'appTest',
  queries: { contentChild: new ContentChild('test') },
  preserveWhitespaces: false,
  jit: true,
  moduleId: 'test-module',
  interpolation: ['{{', '}}']
})
export class TestComponent {}
  ",
            None,
        ),
        (
            r"
@Directive({
  selector: '[appTest]',
  standalone: true,
  providers: [TestService],
  host: { '[class.test]': 'true' },
  hostDirectives: [OtherDirective],
  inputs: ['value'],
  outputs: ['change'],
  exportAs: 'appTest',
  queries: { contentChild: new ContentChild('test') },
  jit: true
})
export class TestDirective {}
  ",
            None,
        ),
        (
            r"
@NgModule({
  id: 'test-module',
  imports: [CommonModule],
  declarations: [TestComponent],
  providers: [TestService],
  exports: [TestComponent],
  bootstrap: [AppComponent],
  schemas: [CUSTOM_ELEMENTS_SCHEMA],
  jit: true
})
export class TestModule {}
  ",
            None,
        ),
        (
            r"
@Pipe({
  name: 'testPipe',
  standalone: true,
  pure: false
})
export class TestPipe {}
  ",
            None,
        ),
        (
            r"
    @Type({
      a: 'a',
      b: 'b',
      c: 'c'
    })
    class Test {}
    ",
            None,
        ),
        (
            r"
    @Type({})
    class Test {}
    ",
            None,
        ),
        (
            r"
    @Type({
      a: 'a'
    })
    class Test {}
    ",
            None,
        ),
        (
            r"
      @Component({
        selector: 'app-root',
        imports: [CommonModule],
        standalone: true,
        templateUrl: './app.component.html',
        styleUrl: './app.component.css',
        encapsulation: ViewEncapsulation.None,
        changeDetection: ChangeDetectionStrategy.OnPush
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @Directive({
        selector: '[app-test]',
        standalone: true
      })
      class Test {}
    ",
            Some(serde_json::json!([{"Directive":["selector","standalone"]}])),
        ),
        (
            r"
      @NgModule({
        declarations: [AppComponent],
        imports: [CommonModule]
      })
      class Test {}
    ",
            Some(serde_json::json!([{"NgModule":["declarations","imports"]}])),
        ),
        (
            r"
      @Pipe({
        name: 'myPipe',
        standalone: true
      })
      class Test {}
    ",
            Some(serde_json::json!([{"Pipe":["name","standalone"]}])),
        ),
        (
            r"
      @Component({
        selector: 'app-root',
        templateUrl: './app.component.html',
        styleUrl: './app.component.css'
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @NgModule({
        declarations: [AppComponent],
        exports: [AppComponent]
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"NgModule":["declarations","imports","exports","providers","bootstrap"]}]),
            ),
        ),
        (
            r"
      @Component({
        // Comment above selector
        selector: 'app-root', // Inline comment for selector
        /* Multi-line comment 
           above imports */
        imports: [
          CommonModule,
          FormsModule
        ], // Inline comment after imports
        standalone: true,
        templateUrl: './app.component.html',
        styleUrl: './app.component.css',
        // Comment above encapsulation
        encapsulation: ViewEncapsulation.None,
        changeDetection: ChangeDetectionStrategy.OnPush
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @NgModule({
        // Leading comment for declarations
        declarations: [
          /* Component list comment */
          AppComponent,
          HeaderComponent
        ],
        imports: [
          CommonModule, // Common module comment
          RouterModule /* Router module comment */
        ],
        /* Multi-line export comment
           with multiple lines
           before the property */
        exports: [AppComponent]
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"NgModule":["declarations","imports","exports","providers","bootstrap"]}]),
            ),
        ),
        (
            r"
      @Component({
        selector: 'app-root',
        custom: true
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"Component":["selector"],"allowUnconfiguredProperties":true}]),
            ),
        ),
        (
            r"
      @Component({
        selector: 'app-root',
        template: '<div></div>'
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"Component":["selector","template"],"allowUnconfiguredProperties":false}]),
            ),
        ),
        (
            r"
      @Component({
        moduleId: 'test-module',
        standalone: true,
        selector: 'app-root',
        imports: [CommonModule],
        template: '<div></div>',
        styleUrl: './app.component.css',
        encapsulation: ViewEncapsulation.None,
        changeDetection: ChangeDetectionStrategy.OnPush,
        providers: [TestService],
        host: {'[class.test]': 'true'}
      })
      class TestComponent {}

      @Directive({
        standalone: true,
        selector: '[appTest]',
        inputs: ['value'],
        outputs: ['change'],
        providers: [TestService],
        host: {'[class.test]': 'true'}
      })
      class TestDirective {}

      @Injectable({
        providedIn: 'root'
      })
      class TestService {}

      @NgModule({
        id: 'test-module',
        imports: [CommonModule],
        declarations: [TestComponent],
        providers: [TestService],
        exports: [TestComponent],
        bootstrap: [AppComponent],
        schemas: [CUSTOM_ELEMENTS_SCHEMA]
      })
      class TestModule {}

      @Pipe({
        standalone: true,
        name: 'testPipe',
        pure: true
      })
      class TestPipe {}
    ",
            Some(
                serde_json::json!([{"Component":["moduleId","standalone","signal","selector","imports","template","templateUrl","styleUrl","styleUrls","styles","encapsulation","changeDetection","providers","viewProviders","animations","entryComponents","preserveWhitespaces","interpolation","hostDirectives","host"],"Directive":["standalone","selector","inputs","outputs","providers","exportAs","queries","hostDirectives","host","jit"],"Injectable":["providedIn"],"NgModule":["id","jit","imports","declarations","providers","exports","entryComponents","bootstrap","schemas"],"Pipe":["standalone","name","pure"],"allowUnconfiguredProperties":false}]),
            ),
        ),
        (
            r"
      @Component({
        selector: 'app-root'
      })
      class TestComponent {}

      @Injectable({
        useFactory: createService
      })
      class TestService {}
    ",
            Some(
                serde_json::json!([{"Component":["selector"],"allowUnconfiguredProperties":false}]),
            ),
        ),
    ];

    let fail = vec![
        (
            r"
      @Component({
        changeDetection: ChangeDetectionStrategy.OnPush,
        selector: 'app-root',
        imports: [CommonModule],
        standalone: true,
        templateUrl: './app.component.html',
        styleUrl: './app.component.css',
        encapsulation: ViewEncapsulation.None
      })
      class Test {
      }
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @Directive({
        standalone: true,
        selector: '[app-test]'
      })
      class Test {
      }
    ",
            Some(serde_json::json!([{"Directive":["selector","standalone"]}])),
        ),
        (
            r"
      @NgModule({
        imports: [CommonModule],
        declarations: [AppComponent]
      })
      class Test {
      }
    ",
            Some(serde_json::json!([{"NgModule":["declarations","imports"]}])),
        ),
        (
            r"
      @Pipe({
        standalone: true,
        name: 'myPipe'
      })
      class Test {
      }
    ",
            Some(serde_json::json!([{"Pipe":["name","standalone"]}])),
        ),
        (
            r"
      @Component({
        styleUrl: './app.component.css',
        selector: 'app-root',
        templateUrl: './app.component.html'
      })
      class Test {
      }
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @NgModule({
        exports: [AppComponent],
        declarations: [AppComponent]
      })
      class Test {
      }
    ",
            Some(
                serde_json::json!([{"NgModule":["declarations","imports","exports","providers","bootstrap"]}]),
            ),
        ),
        (
            r"
      @Component({
        styleUrl: './app.component.css', // Inline comment for styleUrl
        selector: 'app-root' // Inline comment for selector
      })
      class Test {}
    ",
            Some(serde_json::json!([{"Component":["selector","styleUrl"]}])),
        ),
        (
            r"
      @Component({
        // Comment above styleUrl
        styleUrl: './app.component.css',
        // Comment above selector
        selector: 'app-root'
      })
      class Test {}
    ",
            Some(serde_json::json!([{"Component":["selector","styleUrl"]}])),
        ),
        (
            r"
      @Component({
        /* This is a multi-line comment
           above styleUrl property */
        styleUrl: './app.component.css',
        /* This is a multi-line comment
           above selector property */
        selector: 'app-root'
      })
      class Test {}
    ",
            Some(serde_json::json!([{"Component":["selector","styleUrl"]}])),
        ),
        (
            r"
      @Component({
        // Comment above changeDetection
        changeDetection: ChangeDetectionStrategy.OnPush, // Inline comment for changeDetection
        /* Multi-line comment
           above selector */
        selector: 'app-root', /* Inline multi-line comment after selector */
        // Comment above imports
        imports: [
          // Comment inside imports array
          CommonModule, // Comment after CommonModule
          FormsModule /* Comment after FormsModule */
        ],
        /* Comment above standalone */
        standalone: true, // Comment after standalone
        // Comment above templateUrl
        templateUrl: './app.component.html',
        /* Multi-line comment
           above styleUrl */
        styleUrl: './app.component.css',
        // Comment above encapsulation
        encapsulation: ViewEncapsulation.None /* Inline comment for encapsulation */
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @Component({
        styleUrl: './app.component.css',
        selector: 'app-root',
        templateUrl: './app.component.html',
        providers: [MyService, myProvider]
      })
      class Test {
      }
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @Component({
        custom: true
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"Component":["selector"],"allowUnconfiguredProperties":false}]),
            ),
        ),
        (
            r"
      @Component({
        template: '<div></div>',
        custom: true,
        selector: 'app-root'
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"Component":["selector","template"],"allowUnconfiguredProperties":false}]),
            ),
        ),
        (
            r"
      @Injectable({
        useFactory: createService,
        providedIn: 'root'
      })
      class TestService {}
    ",
            None,
        ),
    ];

    let fix = vec![
        (
            r"
      @Component({
        changeDetection: ChangeDetectionStrategy.OnPush,
        selector: 'app-root',
        imports: [CommonModule],
        standalone: true,
        templateUrl: './app.component.html',
        styleUrl: './app.component.css',
        encapsulation: ViewEncapsulation.None
      })
      class Test {
      }
    ",
            r"
      @Component({
        selector: 'app-root',
        imports: [CommonModule],
        standalone: true,
        templateUrl: './app.component.html',
        styleUrl: './app.component.css',
        encapsulation: ViewEncapsulation.None,
        changeDetection: ChangeDetectionStrategy.OnPush
      })
      class Test {
      }
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @Directive({
        standalone: true,
        selector: '[app-test]'
      })
      class Test {
      }
    ",
            r"
      @Directive({
        selector: '[app-test]',
        standalone: true
      })
      class Test {
      }
    ",
            Some(serde_json::json!([{"Directive":["selector","standalone"]}])),
        ),
        (
            r"
      @NgModule({
        imports: [CommonModule],
        declarations: [AppComponent]
      })
      class Test {
      }
    ",
            r"
      @NgModule({
        declarations: [AppComponent],
        imports: [CommonModule]
      })
      class Test {
      }
    ",
            Some(serde_json::json!([{"NgModule":["declarations","imports"]}])),
        ),
        (
            r"
      @Pipe({
        standalone: true,
        name: 'myPipe'
      })
      class Test {
      }
    ",
            r"
      @Pipe({
        name: 'myPipe',
        standalone: true
      })
      class Test {
      }
    ",
            Some(serde_json::json!([{"Pipe":["name","standalone"]}])),
        ),
        (
            r"
      @Component({
        styleUrl: './app.component.css',
        selector: 'app-root',
        templateUrl: './app.component.html'
      })
      class Test {
      }
    ",
            r"
      @Component({
        selector: 'app-root',
        templateUrl: './app.component.html',
        styleUrl: './app.component.css'
      })
      class Test {
      }
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @NgModule({
        exports: [AppComponent],
        declarations: [AppComponent]
      })
      class Test {
      }
    ",
            r"
      @NgModule({
        declarations: [AppComponent],
        exports: [AppComponent]
      })
      class Test {
      }
    ",
            Some(
                serde_json::json!([{"NgModule":["declarations","imports","exports","providers","bootstrap"]}]),
            ),
        ),
        (
            r"
      @Component({
        styleUrl: './app.component.css', // Inline comment for styleUrl
        selector: 'app-root' // Inline comment for selector
      })
      class Test {}
    ",
            r"
      @Component({
        selector: 'app-root', // Inline comment for selector
        styleUrl: './app.component.css' // Inline comment for styleUrl
      })
      class Test {}
    ",
            Some(serde_json::json!([{"Component":["selector","styleUrl"]}])),
        ),
        (
            r"
      @Component({
        // Comment above styleUrl
        styleUrl: './app.component.css',
        // Comment above selector
        selector: 'app-root'
      })
      class Test {}
    ",
            r"
      @Component({
        // Comment above selector
        selector: 'app-root',
        // Comment above styleUrl
        styleUrl: './app.component.css'
      })
      class Test {}
    ",
            Some(serde_json::json!([{"Component":["selector","styleUrl"]}])),
        ),
        (
            r"
      @Component({
        /* This is a multi-line comment
           above styleUrl property */
        styleUrl: './app.component.css',
        /* This is a multi-line comment
           above selector property */
        selector: 'app-root'
      })
      class Test {}
    ",
            r"
      @Component({
        /* This is a multi-line comment
           above selector property */
        selector: 'app-root',
        /* This is a multi-line comment
           above styleUrl property */
        styleUrl: './app.component.css'
      })
      class Test {}
    ",
            Some(serde_json::json!([{"Component":["selector","styleUrl"]}])),
        ),
        (
            r"
      @Component({
        // Comment above changeDetection
        changeDetection: ChangeDetectionStrategy.OnPush, // Inline comment for changeDetection
        /* Multi-line comment
           above selector */
        selector: 'app-root', /* Inline multi-line comment after selector */
        // Comment above imports
        imports: [
          // Comment inside imports array
          CommonModule, // Comment after CommonModule
          FormsModule /* Comment after FormsModule */
        ],
        /* Comment above standalone */
        standalone: true, // Comment after standalone
        // Comment above templateUrl
        templateUrl: './app.component.html',
        /* Multi-line comment
           above styleUrl */
        styleUrl: './app.component.css',
        // Comment above encapsulation
        encapsulation: ViewEncapsulation.None /* Inline comment for encapsulation */
      })
      class Test {}
    ",
            r"
      @Component({
        /* Multi-line comment
           above selector */
        selector: 'app-root', /* Inline multi-line comment after selector */
        // Comment above imports
        imports: [
          // Comment inside imports array
          CommonModule, // Comment after CommonModule
          FormsModule /* Comment after FormsModule */
        ],
        /* Comment above standalone */
        standalone: true, // Comment after standalone
        // Comment above templateUrl
        templateUrl: './app.component.html',
        /* Multi-line comment
           above styleUrl */
        styleUrl: './app.component.css',
        // Comment above encapsulation
        encapsulation: ViewEncapsulation.None, /* Inline comment for encapsulation */
        // Comment above changeDetection
        changeDetection: ChangeDetectionStrategy.OnPush // Inline comment for changeDetection
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @Component({
        styleUrl: './app.component.css',
        selector: 'app-root',
        templateUrl: './app.component.html',
        providers: [MyService, myProvider]
      })
      class Test {
      }
    ",
            r"
      @Component({
        selector: 'app-root',
        templateUrl: './app.component.html',
        styleUrl: './app.component.css',
        providers: [MyService, myProvider]
      })
      class Test {
      }
    ",
            Some(
                serde_json::json!([{"Component":["selector","imports","standalone","templateUrl","styleUrl","encapsulation","changeDetection"]}]),
            ),
        ),
        (
            r"
      @Component({
        template: '<div></div>',
        custom: true,
        selector: 'app-root'
      })
      class Test {}
    ",
            r"
      @Component({
        selector: 'app-root',
        template: '<div></div>',
        custom: true
      })
      class Test {}
    ",
            Some(
                serde_json::json!([{"Component":["selector","template"],"allowUnconfiguredProperties":false}]),
            ),
        ),
        (
            r"
      @Injectable({
        useFactory: createService,
        providedIn: 'root'
      })
      class TestService {}
    ",
            r"
      @Injectable({
        providedIn: 'root',
        useFactory: createService
      })
      class TestService {}
    ",
            None,
        ),
    ];

    Tester::new(SortKeysInTypeDecorator::NAME, SortKeysInTypeDecorator::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .expect_fix(fix)
        .test_and_snapshot();
}
