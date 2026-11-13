# Node and undici security baseline — 2026-11-13

Scope: this optional development bridge's unbundled SDK and Node-native fetch.
Issue [#107](https://github.com/trotsky1997/Rydantic/issues/107) tracks this work.
The SDK-local npm override is now **undici 8.12.4**. It does not replace Node's
built-in undici or rewrite pi's separately bundled CLI/RPC artifacts. pi remains
1.3.0 and the scoped minimatch 11.0.1 override is unchanged.

## Supported runtime policy

Use Node `>=22.23.5 <23 || >=24.22.3 <25`, then reinstall from the lockfile.
This raises the optional bridge's runtime floors, not any Rust crate version.
Previous 22.23.4 / 24.22.1 baselines no longer meet this policy. No application
data or credentials are migrated or deleted. npm engines normally warns rather
than blocking startup; this is not startup security enforcement. Other Node
major lines remain outside the audited support scope, not asserted vulnerable.

The [November Node security release](https://nodejs.org/en/blog/vulnerability/november-2026-security-releases)
includes runtime and dependency fixes. Official November 11 releases establish:

| Node baseline | Built-in undici in official source | Evidence |
| --- | --- | --- |
| 22.23.5 | 6.29.1, unchanged from 22.23.4 | [Release](https://nodejs.org/en/blog/release/v22.23.5), [bundled manifest](https://github.com/nodejs/node/blob/v22.23.5/deps/undici/src/package.json) |
| 24.22.3 | 7.30.2, replacing 7.30.1 in the previous baseline | [Release](https://nodejs.org/en/blog/release/v24.22.3), [bundled manifest](https://github.com/nodejs/node/blob/v24.22.3/deps/undici/src/package.json) |

These are upstream-source inventories, not claims of local execution on those
new releases. The Node 22 update is still needed for other runtime fixes even
though its undici 6.29.1 lies outside the six November undici advisories' stated
affected ranges. This does not establish that every Node security advisory is
reachable through this bridge.

CI tests exact baselines and moving 22/24 with `check-latest: true`. Tests record
`process.versions.node` and `process.versions.undici`, enforce the matching
minimums and reject a new built-in undici major until re-audited. Synthetic
boundary tests reject previous Node floors even with patched npm undici; the
npm override cannot compensate for an outdated runtime.

## November npm undici advisories

[undici 8.12.4](https://github.com/nodejs/undici/releases/tag/v8.12.4) was released
November 5. All six advisories below list fixes in **7.30.2 / 8.12.4**. Ranges are
undici versions, not Node versions. Installed 8.12.2 and the previous Node 24
bundle 7.30.1 are within the stated ranges; 6.29.1 is not.

| Advisory | Affected ranges | Required behavior or configuration |
| --- | --- | --- |
| [GHSA-mqg3-3v4r-cm32](https://github.com/nodejs/undici/security/advisories/GHSA-mqg3-3v4r-cm32) | >=7.30.1 <7.30.2; >=8.12.2 <8.12.4 | Shared cache with redirect-before-cache composition can ignore intermediate response cache policy. |
| [GHSA-7xg3-fphp-28hg](https://github.com/nodejs/undici/security/advisories/GHSA-7xg3-fphp-28hg) | >=7.0.0 <7.30.2; >=8.0.0 <8.12.4 | Credential-bearing same-origin redirects with shared caching can disclose cached responses. |
| [GHSA-5rm8-gv32-r954](https://github.com/nodejs/undici/security/advisories/GHSA-5rm8-gv32-r954) | >=7.0.0 <7.30.2; >=8.0.0 <8.12.4 | Redirect/cache composition can use another origin's 304 to revalidate cached content. |
| [GHSA-9v6h-8rgp-m69f](https://github.com/nodejs/undici/security/advisories/GHSA-9v6h-8rgp-m69f) | >=7.0.0 <7.30.2; >=8.0.0 <8.12.4 | Tuple-array credential headers with an outer normalizing interceptor can survive cross-origin redirect stripping. |
| [GHSA-3g9m-rv3r-w5w2](https://github.com/nodejs/undici/security/advisories/GHSA-3g9m-rv3r-w5w2) | >=7.0.0 <7.30.2; >=8.0.0 <8.12.4 | Cached-response replay with throwing handlers can terminate the process. |
| [GHSA-r4pc-4jcp-7pqc](https://github.com/nodejs/undici/security/advisories/GHSA-r4pc-4jcp-7pqc) | >=7.15.0 <7.30.2; >=8.0.0 <8.12.4 | Certain ASCII whitespace in fetch integrity metadata can skip SRI verification. |

These are confirmed affected dependencies, not proof of exploitability through
this bridge. The bridge does not directly compose the cache/redirect/interceptor
configurations above or supply integrity metadata. This limited source inspection
is not exhaustive SDK reachability analysis. The native-fetch and npm-undici
implementations are separate update targets, even though a custom dispatcher can
connect their use. Creating ModelRuntime does not replace global fetch.

Earlier fixes remain included: [excessive content encodings](https://github.com/nodejs/undici/security/advisories/GHSA-xjj6-f33q-929c),
[EventSource retained memory](https://github.com/nodejs/undici/security/advisories/GHSA-4qv8-2p9j-2685),
and [decompress-interceptor exceptions](https://github.com/nodejs/undici/security/advisories/GHSA-55wc-3pg8-jfw6).
The [October audit](https://github.com/trotsky1997/Rydantic/blob/8d5f5ebee09163f417cfce52f9722a030de38d84/crates/rydantic-ai/bridge/RUNTIME-SECURITY.md)
records their ranges and historical execution. Its earlier baselines and test
results are not evidence for the new candidate.

## Validation boundaries

The lockfile update uses the official npm artifact and integrity, changing only
the root engine policy and undici version/URL/integrity. Lifecycle scripts,
funding calls and npm audit inventory submission are disabled during installation.
The SDK-local test checks version, unchanged upstream API floor and exports.

The new bounded SRI test uses a loopback server, a short synthetic body, and
matching/mismatching SHA-256 metadata containing ASCII whitespace. Existing
ModelRuntime tests retain native and npm-undici compressed Responses SSE,
nonstream output, error handling and cancellation. No credentials, login, live
provider or external exploit endpoint is used.

The local cloud has only the previous Node 22.23.4 and 24.22.1 runtimes. Functional
and synthetic-policy tests can run there, while the actual-runtime policy test
must reject them. Those local checks are not full supported-runtime acceptance.
The exact patched-runtime suite is a separate GitHub CI gate, with results
reported in the associated PR rather than presumed in this document.

No whole-dependency-graph security guarantee, bundled pi CLI/RPC remediation,
production acceptance or performance claim is made. Continue applying security
updates and reassess this dated policy as advisories and supported lines change.
