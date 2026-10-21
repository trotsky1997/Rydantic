# Fullstack forum example

A small Rust discussion forum built with Dioxus Fullstack and Rydantic. The app demonstrates validated model input and SQLite persistence in a complete web workflow.

## Run

Install the [Dioxus CLI](https://dioxuslabs.com/learn/0.7/getting_started/), then run from this directory:

```sh
FORUM_MODERATOR_INVITE='choose-a-long-random-value' dx serve --web --fullstack
```

Open <http://127.0.0.1:8080>. The first launch creates `forum.sqlite` in this directory. Register with the configured invite to create the initial moderator account; regular registration creates a member account.

To compile either side without starting the app:

```sh
cargo check --no-default-features --features server
cargo check --no-default-features --features web
```

## Rydantic integration

`PostRow` and `ReplyRow` derive `Model` and `SqlModel`. Their fields define title, body, author, and category constraints. The server validates request data with `parse_json`, persists validated rows with `insert`, and calls `migrate` to create or upgrade the SQLite tables. Accounts, sessions, reports, and rate-limit counters use SQLx directly alongside those models.

The UI includes a small local Dioxus `Button` component and CSS. Its component API follows the pattern shown in [Rust/UI's Dioxus Button](https://rust-ui.com/docs/components/button).

## Included workflows

- Topic categories, search, ordering, replies, and PNG/JPEG/WebP attachments up to 512 KiB
- Argon2 password hashes and seven-day HttpOnly session cookies
- Owner-only content deletion and moderator review, hide, restore, and dismiss actions
- Account-scoped limits for topics, replies, reports, registration, and login

This example is intended for local development. Set `FORUM_COOKIE_SECURE=1` when serving over HTTPS. Rate limits use account or username buckets; public deployments also need IP-based limits, backups, and operational monitoring.
