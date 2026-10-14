import { Component } from "@angular/core";

@Component({
  template: `
    <!-- eslint-disable-next-line @angular-eslint/template/no-call-expression -->
    <div>{{ f() }}</div>
    <!-- eslint-disable -->
    <div>{{ $any(a) }}</div>
  `,
})
export class A {}

@Component({ template: `<div>{{ $any(b) }}</div>` })
export class B {}

// A template's directives do not reach the script.
function unused() {}
