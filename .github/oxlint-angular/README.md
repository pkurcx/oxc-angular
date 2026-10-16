# @pkurcx/oxlint-angular

[oxlint](https://oxc.rs/docs/guide/usage/linter) with two Angular plugins, native ports of angular-eslint 22.6.0:

- `angular`: [`@angular-eslint/eslint-plugin`](https://github.com/angular-eslint/angular-eslint/tree/main/packages/eslint-plugin),
  rules for components, directives, pipes and services in `.ts` files.
- `angular-template`: [`@angular-eslint/eslint-plugin-template`](https://github.com/angular-eslint/angular-eslint/tree/main/packages/eslint-plugin-template),
  rules for templates.

Everything else is upstream oxlint at the same version.

> [!NOTE]
> This is an unofficial fork, not published by the Oxc project.
> Source: [pkurcx/oxc-angular](https://github.com/pkurcx/oxc-angular).

## Install

```sh
npm add -D @pkurcx/oxlint-angular
```

The binary is still `oxlint`, so install it instead of `oxlint`, not next to it.

## Configure

Enable the plugins as `angular` or `@angular-eslint`, and `angular-template` or `@angular-eslint/template`. Rule keys
accept both prefixes, so existing angular-eslint rule settings and their options can be moved over as they are.

```json
{
  "plugins": ["typescript", "angular", "angular-template"],
  "rules": {
    "@angular-eslint/component-selector": ["error", { "type": "element", "prefix": "app", "style": "kebab-case" }],
    "@angular-eslint/prefer-on-push-component-change-detection": "error",
    "@angular-eslint/template/no-any": "error"
  }
}
```

### Choosing files

With `angular-template` in the root `plugins`, every `.html` file is linted as a template, so a non-Angular page such as
`src/index.html` is reported for parse errors. To scope it, enable it in an override instead. Its `plugins` add to the
root ones, and `files` must cover `.ts` as well for inline templates:

```json
{
  "plugins": ["typescript"],
  "overrides": [
    {
      "files": ["src/app/**/*.{html,ts}"],
      "excludeFiles": ["**/*.generated.html"],
      "plugins": ["angular-template"],
      "rules": { "angular-template/no-any": "error" }
    }
  ]
}
```

`excludeFiles` only drops template rules. Use `ignorePatterns` for files that should not be linted at all, such as
`coverage/**`.

## What is linted

`angular` rules run on `.ts` files. `angular-template` rules run on:

- `.html` files, as templates. Only template rules run on them.
- The inline `template` of a `@Component` in a file importing `@angular/core`. Templates given as `templateUrl` are
  linted through their `.html` file.

An inline template is skipped when it is a template literal with `${}` substitutions or contains a backslash escape.
A template that fails to parse is reported as an error, and its rules are skipped.

`<!-- eslint-disable -->`, `<!-- eslint-disable-next-line rule -->` and `<!-- eslint-enable -->` comments work in
templates and apply only to template rules.

## Differences from angular-eslint

These `@angular-eslint/eslint-plugin` rules need the TypeScript type checker and are not ported: `no-developer-preview`,
`no-experimental`, `no-uncalled-signals`, `prefer-signal-model`, `prefer-signals`, `reactive-context-must-read-signal`.
`angular/sorted-component-imports` has no angular-eslint counterpart.

Rules that deviate from angular-eslint say so in their documentation, in a "Differences compared to `@angular-eslint`" section.
The template parser is [`oxc-angular-compiler`](https://github.com/pkurcx/oxc-angular-compiler) rather than
`@angular/compiler`, so a template it rejects is reported and skipped even when Angular accepts it.
