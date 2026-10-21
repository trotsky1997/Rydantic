# Rydantic documentation

Read the published guides at [Rydantic Docs](https://rydantic.mintlify.site/introduction). This directory is the [Mintlify](https://www.mintlify.com/) source project. These sources describe stable 0.7.0, published on 2026-10-21, with breaking changes and migration responsibilities called out explicitly. See the [release record](https://github.com/trotsky1997/Rydantic/releases/tag/v0.7.0) for verification details; use the [versioned 0.6.0 source](https://github.com/trotsky1997/Rydantic/tree/v0.6.0) for older deployments.

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
npm ci
npm run docs:dev
npm run docs:check
```

`docs:dev` starts the official Mintlify preview. `docs:check` runs the official build validation and broken-link checker. The CLI may download its preview client on the first run.

## Connect Mintlify

Connect the existing `trotsky1997/Rydantic` repository in the Mintlify dashboard. Set the production branch to `main` and the documentation directory to `docs`. Keep the GitHub App installation limited to the intended repository. Pushes to `main` then publish the docs through Mintlify.

The public documentation origin is configured as `docs` in `website/site.json`. The homepage build uses it for every documentation link. Update that one value when changing the documentation hostname.

## Homepage

`website/` contains the GitHub Pages homepage, independent of Mintlify hosting:

```sh
npm run site:dev
```

The build writes `.site/`, derives the displayed version from `Cargo.toml`, and uses relative asset URLs for the `/Rydantic/` project path. The `Documentation and homepage` workflow checks both projects and publishes the homepage from `main` through the `github-pages` environment. No deploy token is stored in the repository.
