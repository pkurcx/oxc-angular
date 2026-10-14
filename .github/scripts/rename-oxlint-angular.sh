#!/usr/bin/env bash
# Rewrite the oxlint npm package into `@pkurcx/oxlint-angular` before building and publishing it.
# Usage: bash .github/scripts/rename-oxlint-angular.sh

set -euo pipefail

node -e '
const fs = require("fs");
const file = "npm/oxlint/package.json";
const pkg = JSON.parse(fs.readFileSync(file, "utf8"));
pkg.name = "@pkurcx/oxlint-angular";
// `--provenance` rejects a repository that does not match the one running the workflow.
pkg.repository.url = "git+https://github.com/pkurcx/oxc-angular";
pkg.bugs = "https://github.com/pkurcx/oxc-angular/issues";
pkg.homepage = "https://github.com/pkurcx/oxc-angular";
pkg.publishConfig = { access: "public" };
pkg.napi.packageName = "@pkurcx/oxlint-angular-binding";
fs.writeFileSync(file, JSON.stringify(pkg, null, 2) + "\n");
'

# The committed loader is bundled into `dist` as is; the publish job never regenerates it.
sed -i.bak 's#@oxlint/binding-#@pkurcx/oxlint-angular-binding-#g' apps/oxlint/src-js/bindings.js
# Keep the renamed bindings external, as `@oxlint/*` is upstream.
sed -i.bak 's#"@oxlint/\*",#"@oxlint/*", "@pkurcx/*",#' apps/oxlint/tsdown.config.ts
rm apps/oxlint/src-js/bindings.js.bak apps/oxlint/tsdown.config.ts.bak

grep -q '"@pkurcx/\*"' apps/oxlint/tsdown.config.ts
if grep -q '@oxlint/binding' apps/oxlint/src-js/bindings.js; then
  echo "::error::bindings.js still references @oxlint/binding"
  exit 1
fi

cp .github/oxlint-angular/README.md npm/oxlint/README.md
