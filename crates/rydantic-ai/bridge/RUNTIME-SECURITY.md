# Node runtime audit — 2026-10-31

Scope: this optional development bridge's unbundled SDK and Node-native fetch.
The installed npm undici override from PR #148 remains **8.12.2**. It does not
patch Node's built-in undici or pi's separately bundled CLI/RPC artifacts.

## Runtime inventory and support policy

Observed by running `node -p 'JSON.stringify({node:process.versions.node,
undici:process.versions.undici})'` on Linux x64:

| Node | Built-in undici | Role |
| --- | --- | --- |
| 22.19.0 | 6.21.2 | Former API floor; no longer supported by the bridge |
| 22.23.4 | 6.29.1 | Audited 22.x baseline and latest 22.x at audit time |
| 24.22.1 | 7.30.1 | Audited 24.x baseline |
| 24.22.2 | 7.30.1 | Latest 24.x at audit time |

22.19.0, 22.23.4 and 24.22.2 were downloaded from the official Node distribution
and checked against that release's SHA256 manifest. 24.22.1 was the preinstalled
cloud runtime, independently executed; its upstream release notes corroborate
undici 7.30.1. This is not a claim of testing every operating system/build.

Node's [October security release](https://nodejs.org/en/blog/vulnerability/october-2026-security-releases)
includes undici updates; release notes specify
[6.29.1 in 22.23.4](https://nodejs.org/en/blog/release/v22.23.4) and
[7.30.1 in 24.22.1](https://nodejs.org/en/blog/release/v24.22.1).
[24.22.2](https://nodejs.org/en/blog/release/v24.22.2) retains that undici version.

The bridge now declares `>=22.23.4 <23 || >=24.22.1 <25`. This is a **breaking
runtime support change**, not a Rust crate API/version change. Upgrade Node and
reinstall from the lockfile. No data or credentials are migrated or deleted.
Other Node major lines are outside this bridge's audited support scope, not
necessarily vulnerable. npm's engines field normally warns rather than blocking
startup. CI tests exact baselines plus moving 22/24 releases; the runtime test
records Node/undici versions and rejects a baseline regression. Later patches
within the same undici major are accepted; a new major requires re-audit.

## Advisory applicability is not reachability proof

Upstream affected ranges below refer to **undici**, not Node version numbers:

| Advisory | Affected undici ranges | Patched branches | Bridge interpretation |
| --- | --- | --- | --- |
| [GHSA-xjj6-f33q-929c](https://github.com/nodejs/undici/security/advisories/GHSA-xjj6-f33q-929c) | >=6.23.0 <6.29.1; >=7.18.0 <7.30.1; >=8.0.0 <8.12.2 | 6.29.1 / 7.30.1 / 8.12.2 | Fetch response with excessive content encodings and early connection close can crash affected undici. Default bridge uses native fetch, but no exploit through this bridge/provider was demonstrated. |
| [GHSA-4qv8-2p9j-2685](https://github.com/nodejs/undici/security/advisories/GHSA-4qv8-2p9j-2685) | >=6.5.0 <6.29.1; >=7.0.0 <7.30.1; >=8.0.0 <8.12.2 | 6.29.1 / 7.30.1 / 8.12.2 | EventSource retained-memory issue; the bridge does not directly use EventSource. Ordinary SSE streaming is not evidence that this parser is used. |
| [GHSA-55wc-3pg8-jfw6](https://github.com/nodejs/undici/security/advisories/GHSA-55wc-3pg8-jfw6) | >=7.15.0 <7.30.1; >=8.0.0 <8.12.2 | 7.30.1 / 8.12.2 | Decompress interceptor plus a throwing downstream handler; bridge does not directly compose this interceptor. No affected 6.x range is listed. |

In particular **22.19.0 / undici 6.21.2 is outside the listed excessive-encoding
and interceptor ranges**, but inside the EventSource version range. It must not
be called an established default-fetch exploit on that basis. Conversely, the
old open-ended Node range admitted later vulnerable dependency versions:
[22.23.3 bundled 6.28.1](https://nodejs.org/en/blog/release/v22.23.3), inside the
excessive-encoding range. Passing functional bridge tests did not prove safety.

The new baselines contain upstream fixes for all three advisories where their
undici branch is affected. They are not a vulnerability-free certification of
Node, the full dependency graph, live OAuth/providers, or pi's bundled tools.
Continue applying security updates and reassess this dated policy as advisories
or supported Node lines change. No exploit was run against external services;
validation uses only synthetic fixtures and loopback servers.

## Verification performed

On this change, a clean `npm ci --ignore-scripts --no-audit --no-fund` completed
(using a writable temporary npm cache in the cloud workspace). `node --test
'*.test.mjs'` passed **9/9** on 22.23.4, 24.22.1 and 24.22.2. The two existing
functional test files separately passed **8/8** on the former 22.19.0 floor;
the new runtime-policy test correctly failed there with
`Node below audited baseline: 22.19.0`. That negative test checks support policy,
not exploitation. Independent review also reran the supported-baseline suite.

No npm audit inventory was submitted. No live provider/OAuth test, adversarial
exploit reproduction, other Node major, or other OS was tested in this audit.
GitHub CI results are recorded in the associated PR, not presumed by this local
verification record.
