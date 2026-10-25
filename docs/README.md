# Rydantic documentation

Read the published guides at [Rydantic Docs](https://rydantic.mintlify.site/introduction). This directory is the [Mintlify](https://www.mintlify.com/) source project. The default guides describe stable 0.8.0, published on 2026-10-25, including ordered callback lists, per-call context, typed pipelines and optional OpenAPI components. Breaking changes and migration responsibilities remain explicit, especially the Context unwind-boundary change from 0.7.x. See the [stable release record](https://github.com/trotsky1997/Rydantic/releases/tag/v0.8.0) and [registry acceptance](../acceptance/core-0.8/STABLE-REGISTRY.md) for verification details; use versioned source and API documentation for older deployments.

## Read the guides

- [Quickstart](quickstart.mdx)
- [Fields and validation](models/validation.mdx)
- [Axum and Tokio](integrations/axum.mdx)
- [Database models](integrations/sql.mdx)
- [Automatic migrations](integrations/migrations.mdx)
- [Native Cap’n Proto](integrations/capnp.mdx)
- [Cargo features](reference/features.mdx)

## Preview and validate

From the repository root, use Node.js 24 LTS:

```sh
npm ci --ignore-scripts
npm run docs:test
npm run docs:check
npm run site:build
npm run check
```

The repository no longer installs the Mintlify CLI, its preview client, or their
blocked transitive dependencies (Refs [#80](https://github.com/trotsky1997/Rydantic/issues/80)).
**Tooling change:** `docs:dev` has been removed. There is no local Mintlify preview
or local proprietary strict-prebuild validation in this workflow. Installing an
untracked CLI or adding overrides is not the remediation.

`docs:check` uses the official MDX compiler with GFM and YAML frontmatter support,
without evaluating page JavaScript. It checks all MDX files, static local snippet
imports, navigation, assets, local links and heading/explicit anchors. Imported
snippets are restricted to code, plain formatted text and comments; headings,
links, JSX and nested imports are rejected because their rendered parent-page
anchor/base-URL semantics are not modeled by this checker.
The repository's supported configuration and component subset is checked explicitly;
icons accept static library names only. New config keys/components/JSX attributes
or dynamic expression/link/import patterns require extending the
checker and its regression tests. External URL availability, runtime component
behavior, Mintlify-specific custom-heading syntax and full proprietary renderer
semantics are not certified by this local check. Existing homepage, stable-version
and source-snippet checks remain separate and required.

For a rendered preview, open the existing Mintlify GitHub App's preview link on the
pull request. Before merging documentation/tooling changes, verify its actual
**Ready** deployment and open the affected pages (including navigation and snippets).
A skipped `Mintlify Deployment` check is not proof of a successful build; inspect
the preview status/link itself. If no ready preview is available, keep the PR
unmerged and investigate. This is an explicit manual review gate, not an enforced
GitHub branch rule. After merge, verify the published documentation and Pages site.
No hosting configuration, credentials or GitHub App permissions need changing.

## Connect Mintlify

Connect the existing `trotsky1997/Rydantic` repository in the Mintlify dashboard. Set the production branch to `main` and the documentation directory to `docs`. Keep the GitHub App installation limited to the intended repository. Pushes to `main` then publish the docs through Mintlify.

The public documentation origin is configured as `docs` in `website/site.json`. The homepage build uses it for every documentation link. Update that one value when changing the documentation hostname.

## Homepage

`website/` contains the GitHub Pages homepage, independent of Mintlify hosting:

```sh
npm run site:dev
```

The build writes `.site/`, takes the displayed/install version from `website/site.json`'s explicit `stableVersion` (0.7.0), not the prerelease workspace version, and uses relative asset URLs for the `/Rydantic/` project path. The `Documentation and homepage` workflow checks both projects and publishes the homepage from `main` through the `github-pages` environment. No deploy token is stored in the repository.
