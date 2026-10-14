import { Component } from "@angular/core";

@Component({ selector: "app-inline", template: "<div>{{ $any(x) }}</div>" })
export class InlineComponent {}
