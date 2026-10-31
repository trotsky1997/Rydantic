# Optional local Responses development bridge

This private Node package uses the **unbundled** `ModelRuntime` SDK from
`@earendil-works/pi-coding-agent`. It is separate from the Rust runtime and derive
crates. It is a local development bridge, not a production service.

## Install and test

Use an up-to-date, security-patched Node release on a supported release line.
`engines.node` (`>=22.19.0`) describes API compatibility, **not** a minimum secure
Node version. From this directory:

```sh
npm ci --ignore-scripts --no-audit --no-fund
npm test
npm ls undici --all
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
versions and Node engine floor. The lockfile pins the official registry artifact
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
