# Releasing Rydantic

## One-time Trusted Publishing setup

For **both** existing crates, `rydantic-derive` and `rydantic`, a crate owner must
configure [crates.io Trusted Publishing](https://crates.io/docs/trusted-publishing):

- GitHub repository owner: `trotsky1997`
- Repository: `Rydantic`
- Workflow filename: `publish.yml`
- GitHub environment: `crates-io`

Create the matching `crates-io` GitHub environment, restricted to the `main`
branch. Add reviewers only if releases should require a manual approval. These
trust/environment settings require explicit owner authorization; merging this
workflow does not configure them. No long-lived registry secret is needed.

The publishing job alone has `id-token: write`. The pinned official
`rust-lang/crates-io-auth-action` exchanges OIDC identity for a short-lived token
and revokes it in its post step. Do not log, store or pass this token as an input.

## Beta and stable cadence

Keep `main` releasable. Use `X.Y.Z-beta.N` for breaking or substantial changes,
then a separate tested `X.Y.Z` release when ready. There is no persistent beta
branch. Keep the workspace version and the runtime's exact `rydantic-derive`
dependency (`=X.Y.Z-beta.N`, or `=X.Y.Z`) synchronized. Record breaking changes,
affected behavior, and migration/backup warnings in the changelog; legacy data
and deployment compatibility are not an implicit release guarantee.

Keep public quickstarts and `website/site.json`'s `stableVersion` on the verified
stable release while preparing or publishing betas. Advance them only after a
new stable release is verified; explicitly mark beta-only guide sections.

1. Merge the release preparation PR and wait for **that exact main commit's CI**
   to finish successfully, including live database tests.
2. Review the changelog, package content and version. Create an immutable release
   tag `vX.Y.Z-beta.N` or `vX.Y.Z` at that commit. The release commit must contain
   this workflow and `scripts/release.py`.
3. Run **Publish crates** (`publish.yml`) with the workflow branch set to `main`
   and the existing release tag as its `tag` input. It intentionally does not run
   on pull requests, push events or release events, preventing accidental double
   publication. Authorized automation can dispatch the workflow instead of a
   person using the UI.
4. The read-only preflight rejects unknown refs, tags outside main history,
   mismatched versions, non-exact internal dependencies, missing/failed/pending
   main CI, or an already-published version of either crate. Outages and rate
   limits are not treated as proof that a version is absent.
5. The publish job checks out the verified immutable SHA, validates the package,
   publishes `rydantic-derive` once, verifies its checksum and sparse-index
   visibility, then validates and publishes `rydantic` once. Both uploads are
   followed by API and index checksum checks. The token is only passed to these
   steps; it is never stored in the repository or an Actions secret.
6. Confirm the workflow succeeded and inspect both exact crates.io versions.
   Only then describe crates as published. A GitHub Release or docs.rs page is
   not proof of registry publication. Create the GitHub Release separately and
   mark beta releases as prereleases, never as the latest stable release.

## Failure and partial publication

The workflow serializes releases and does not retry `cargo publish`. A nonzero
upload exit may still mean the server accepted the package, so it observes the
registry before deciding the outcome. Checksum mismatches, yanked versions,
network failures and unconfirmed outcomes stop the workflow.

If the derive crate was published but runtime publication failed, simply
re-running this workflow will fail closed at preflight. Inspect the original
run, exact tag, registry versions/checksums and package contents before planning
a targeted recovery of **only the missing crate**. Never republish an existing
version, move the tag, or infer failure from a timeout. Full recovery automation
is deliberately not implemented yet.

A new crate's first-ever publication is not supported by this flow; crates.io
currently requires a separate initial publication before Trusted Publishing can
be configured. This workflow only handles the two existing named crates.

## Testing the guardrails

`python3 -m unittest discover -s scripts -p test_release.py -v` runs network-free
regressions for version/pin checks, CI selection, registry error handling,
duplicate prevention, and publication verification. They run in CI. They do not
exercise OIDC or upload packages; only a real authorized release validates the
end-to-end registry path.

### Repairing publishing tooling without changing release source

The workflow and helper are loaded from the immutable main dispatch commit,
while package source is checked out separately at the verified immutable tag.
Both commits must have successful main-push CI. This lets a reviewed tooling
repair publish an existing tested tag without retagging or changing its package
source. No permissions or credentials are added by this separation.

The helper explicitly runs `cargo package --locked` and hashes its archive under
Cargo metadata's `target_directory`, including configured target overrides. It
does not assume `cargo publish --dry-run` retains an archive at that location.
Real no-network Cargo fixture tests cover default and overridden target paths.
