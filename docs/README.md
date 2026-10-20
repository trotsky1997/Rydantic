# Rydantic documentation

Read the published guides at [Rydantic Docs](https://qq-261ce204.mintlify.site/introduction). This directory is the [Mintlify](https://www.mintlify.com/) source project. These sources target the 0.7.0-beta.1 candidate, with prerelease installation and breaking changes called out explicitly. Version 0.6.0 remains the latest stable release until promotion. Candidate version metadata is not proof of publication.

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
