# Optional local Responses development bridge

This private Node package uses the **unbundled** `ModelRuntime` SDK from
`@earendil-works/pi-coding-agent`. It is separate from the Rust runtime and derive
crates. It is a local development bridge, not a production service.

## Install and test

Use an up-to-date, security-patched Node **22.x >=22.23.4** or **24.x >=24.22.1**.
These are the audited support baselines, not a guarantee against future security
issues. CI tests both exact baselines and the moving `22` / `24` release tracks.
Other Node major versions are not currently supported by this bridge.

**Breaking runtime requirement:** the previous `>=22.19.0` API-only range is no
longer the bridge support policy. Upgrade Node itself before reinstalling with
`npm ci`; an npm override cannot update Node's built-in fetch implementation.
No application data or credentials are migrated. npm normally only warns about
unsupported engines; this is not a startup security enforcement mechanism.
See [the dated runtime audit](RUNTIME-SECURITY.md) for evidence and limits.

From this directory:

```sh
npm ci --ignore-scripts --no-audit --no-fund
npm test
npm ls undici --all
npm ls minimatch --all
```

The tests use synthetic data and loopback HTTP servers. They do not log in,
read real credentials, or call an external model provider. They cover the real
SDK with both Node's native fetch and an explicitly supplied npm-undici fetch:
compressed Responses SSE, nonstream output, upstream errors, and cancellation.
They do not establish live OAuth/provider acceptance.

## Scoped undici override

Issue [#107](https://github.com/trotsky1997/Rydantic/issues/107) tracks an affected
transitive dependency: pi-coding-agent 1.3.0 pins undici 8.10.2. The bridge overrides
**that parent/version only** to undici **8.12.2**, retaining the existing pi package
versions and the upstream undici API floor (not the bridge support baseline).
The lockfile pins the official registry artifact
and integrity; the regression test checks SDK-local resolution and dispatcher API
exports. A clean `npm ci` is required to update an existing installation.

8.12.2 is the patched 8.x release for:

- [GHSA-xjj6-f33q-929c](https://github.com/nodejs/undici/security/advisories/GHSA-xjj6-f33q-929c): excessive response content encodings.
- [GHSA-4qv8-2p9j-2685](https://github.com/nodejs/undici/security/advisories/GHSA-4qv8-2p9j-2685): EventSource parser retained memory.
- [GHSA-55wc-3pg8-jfw6](https://github.com/nodejs/undici/security/advisories/GHSA-55wc-3pg8-jfw6): decompress-interceptor exceptions.

This repairs the installed npm dependency, not every HTTP implementation:

- The bridge's default SDK SSE and OAuth paths use `globalThis.fetch`. Creating
  `ModelRuntime` does not replace it. Node's built-in undici is shipped with Node
  and is **not** updated by an npm override; keep Node itself patched.
- pi's separately bundled CLI/RPC artifacts can contain embedded dependency code.
  npm overrides do **not** rewrite those bundles. This bridge uses the unbundled
  SDK, not those entrypoints; this change makes no bundled-CLI security claim.
- The bridge does not directly use EventSource or the decompress interceptor.
  No exploit through the default bridge or live provider was demonstrated, and
  this is not a claim that the entire dependency graph is vulnerability-free.

When upgrading pi, re-evaluate the version-qualified override, installed lockfile
and tests together. Remove it only when the upstream dependency resolves to an
appropriate patched release. Do not silently broaden it to every package or
accept an untested parent downgrade.

## Scoped minimatch override (2026-10-31)

Issue [#154](https://github.com/trotsky1997/Rydantic/issues/154) tracks a separate
Dependabot update failure: pi-coding-agent 1.3.0 pins minimatch 10.2.6. A second
override under **that parent/version only** selects **11.0.1**. It does not change
the pi versions, the undici override above, or the supported Node baselines.
Reinstall with `npm ci`; the lockfile includes the official artifact integrity
and the new `@isaacs/nonrecursive-glob-match` dependency.

[GHSA-g3hv-75wg-m249 / CVE-2026-118764](https://github.com/isaacs/minimatch/security/advisories/GHSA-g3hv-75wg-m249)
identifies 10.2.6 as affected and 11.0.1 as patched for excessive CPU/memory use
from attacker-controlled globs with `optimizationLevel >= 2`. This is an affected
installed dependency, **not a demonstrated bridge exploit**. The inspected pi
1.3.0 unbundled callers are model-resolver (`nocase: true`) and package-manager
(default options); neither opts into level 2. The bridge does not directly call
minimatch. This source inspection is not an exhaustive reachability proof.

**Major dependency upgrade warning:** minimatch 11 changes matcher internals and
removes APIs such as `makeRe` and `AST`; it is not a general drop-in replacement
for arbitrary minimatch consumers. The inspected pi callers use the retained
named `minimatch` function. Level-2 preprocessing now caps generated alternatives
using `braceExpandMax` (default 100,000), so exceeding the cap can discard
alternatives and change matching results. Do not describe this as lossless or as
preserving all v10 glob semantics. Users extending the SDK's glob behavior should
check their own patterns and API usage before upgrading; no stored application
data or credentials are migrated by this dependency update.

Regressions cover SDK-local resolution, the real SDK model-glob selector's order,
deduplication and no-match diagnostics, representative package-path globs, and a
small capped level-2 fixture in a time-limited subprocess. Existing real
ModelRuntime loopback tests cover bridge transport behavior. These checks do not
prove universal performance bounds, live provider acceptance, or package-manager
installation behavior. pi's CLI/RPC bundles contain embedded matcher code;
**npm overrides do not rewrite it**, and this change makes no bundled-entrypoint
security claim. Keep the bundle boundary and remaining dependency advisories
separate from the installed unbundled SDK fix.
