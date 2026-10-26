# Contributing to Proksi

Thanks for helping. These rules are light: they keep reviews quick and the code consistent.

## Before you start

- Small fixes: open a pull request.
- New features or bigger changes: open an issue first (or ask on [Discord](https://discord.gg/b22TRMew2m)), so we agree on the approach before you write it.
- Changes that reshape how Proksi works (how routes are loaded, how config is modelled, the store) get an ADR in `docs/adrs` first. Features and plugins don't.

## Making a change

- Branch from `main`. Don't name branches `docs/...` (a `docs` branch already exists).
- Commit messages and pull request titles follow [Conventional Commits](https://www.conventionalcommits.org): `feat:`, `fix:`, `docs:`, `refactor:`, `ci:`, `chore:`. They become the changelog, so keep them short and say what changed for the user.
- Run `make lint` and `make test` before pushing. Store tests run against Redis when `PROKSI_TEST_REDIS_URL` is set (e.g. `redis://127.0.0.1:6379`). For the admin UI, run `npm test` in `crates/proksi-admin-ui`.
- Run `rustfmt` on the code you change only, not on whole files you didn't touch.
- Update the docs (`docs/`) when behavior or configuration changes. When the configuration changes, its schema (`docs/schema/proksi.schema.json`) does too: CI commits it on branches of this repository; from a fork, run `make schema` and commit it.

## How the code is written

- **Performance and memory come first.** The request path doesn't allocate: hot paths have tests with `assert_no_allocations`. Measure changes that could affect speed or memory, and say what you measured in the pull request.
- **One feature, one module,** with few call sites. A feature that isn't configured costs nothing.
- **Errors are enums,** each variant with its message (see `proksi_store::StoreError`), not strings built where they happen.
- **`proksi_store` is an adapter:** it offers generic primitives to every backend, and plugin logic stays in the plugin.
- **`if`/`match`/`for`/`while`/`loop` nest twice at most.** A third level becomes a function (checked by `tests/nesting.rs`).
- **No new patches to Pingora.** Build on its public hooks.

## Pull requests

- Open it as a draft until it's ready for review.
- Keep the description short: what changes and why, and anything a reviewer should look at closely.
- One change per pull request: unrelated fixes go in their own.

## Using AI agents

AI coding agents are welcome, with three rules:

1. **No attribution.** No `Co-Authored-By` trailers, "Generated with ..." lines, or session links in commits, pull requests, code or docs.
2. **Disclose it.** Tick the box in the pull request template and say which agent you used.
3. **Review it.** You're the author: read every line the agent wrote, and tick the box confirming you have. Don't submit code you can't explain.
