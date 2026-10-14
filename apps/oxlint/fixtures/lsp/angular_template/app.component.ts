import { Component } from "@angular/core";

@Component({
  template: `
    <div>{{ $any(x) }}</div>
  `,
})
export class AppComponent {}
