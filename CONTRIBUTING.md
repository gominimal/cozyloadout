# Contributing to the Cozy Loadout (https://github.com/gominimal/cozyloadout)

Thank you for your interest in contributing! This project is developed by **Minimal.dev Software Inc.** and released under the [Apache License 2.0](./LICENSE).

We welcome contributions of all kinds — bug reports, documentation improvements, new tools, better colours, and feedback. This document explains how to contribute and what you need to know about our Contributor License Agreement (CLA).

## TL;DR

1. Fork the "main" branch and commit your changes to your fork
2. Submit your PR as normal
3. Before we can accept your pull request, you'll need to sign our **Contributor License Agreement (CLA)**.
4. This happens automatically on your first PR — a bot will comment with a link. It takes about 30 seconds.

## The one rule that matters most

**Edit `templates/`, never `build/`.** `build/` is deleted and rewritten on every
render and is gitignored, so a change made there evaporates the next time anyone
runs `just theme`. The same goes for `cozy.toml`: it is generated, and its
`patches` list comes from `templates/manifest.toml`.

[AGENTS.md](./AGENTS.md) is the working guide to the repository — the render
pipeline, the template grammar, the per-tool notes, and a record of what has
been verified versus assumed. Read it before changing a template.

## Building and testing

See [README.md](./README.md) for what you need installed. Before opening a PR:

```shell
just check
```

which runs the whole local gate: the renderer's unit tests, `cargo fmt --check`,
pedantic clippy with warnings denied, the `cargo-deny` supply-chain audit, both
checked-in schemes rendered, `dash -n` over the generated hook, and
`fish --no-execute` over the generated shell config. `cargo-deny` is skipped with
a message if it isn't installed (`cargo install cargo-deny`); CI always runs it. The renderer also
parses its own generated TOML, XML and YAML before writing it, so a successful
render is already a validated one.

If you touched a template or the renderer, also run:

```shell
just check-schemes
```

which renders every scheme in the upstream collection (a few minutes) and
expects zero failures. This is the check that catches a template
which happens to work for the two schemes in this repository and breaks on a
light scheme, a scheme with an apostrophe in its name, or a legacy-format one.

## Commit messages

This repository uses [Conventional Commits
v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) — `type(scope): summary`,
imperative, lower-case, no trailing period — enforced on PR commits by
**commitlint** (`.github/workflows/commitlint.yml`, config in
`commitlint.config.cjs`). "Why" is more useful than "what".

Note that `config-conventional` also rejects a capitalised subject, so
`chore: Preparing for X` fails where `chore: prepare for X` passes.

## What we look for in contributions

- **Small, focused PRs.** Easier to review, faster to merge.
- **Evidence.** Say what you ran. For a colour change, say which scheme you
  rendered and what you looked at; for a renderer change, a test.
- **Tests for the renderer.** New behavior needs a test, and a bug fix should
  include a regression test. The existing suite covers both scheme formats, the
  parsing hazards, the template grammar, and the manifest parser.
- **Discuss before large changes.** For anything substantial — a new tool, a
  change to the template grammar — please open an issue first so we can align.

### Adding a tool to the loadout

Drop the template in `templates/`, add a `[[file]]` block to
`templates/manifest.toml`, run `just theme`, and read the result in `build/`.
The patches list follows automatically, so there is no second place to update.
The package itself needs to exist in the [Minimal Public
Registry](https://github.com/gominimal/pkgs) and be added to `packages` in
`templates/cozy.toml`.

## Why do we require a CLA?

A CLA is a standard instrument in many large open source projects (the Apache Software Foundation, Google, Microsoft, the CNCF, and many others all require one). It clearly defines the terms under which intellectual property has been contributed. By having these instruments in place, it supports the growth and sustainability of this open source project.

Full text: [ICLA](./legal/ICLA.md) · [CCLA](./legal/CCLA.md)

## Individual vs. Corporate

- **Contributing as yourself, on your own time, with code you own?** Sign the **Individual CLA (ICLA)**. The bot will walk you through it.
- **Contributing as part of your job, or with code your employer might own?** Your employer needs to sign the **Corporate CLA (CCLA)** and list you as an authorized contributor. Then you'll also sign the ICLA. If you're unsure whether your employer has rights to your contribution, it's worth a conversation with them before contributing. Email **security@minimal.dev** if you need help getting a CCLA in place.

## How signing works

We use [CLA Assistant](https://cla-assistant.io/). When you open your first pull request:

1. A bot will comment with a link to the CLA.
2. Click the link, review the document, and click "I Agree" after signing in with your GitHub account.
3. The bot will re-check your PR and mark the CLA status as satisfied.
4. Once signed, you're covered for all future contributions to any of our repositories.

Your signature record (GitHub username, email, timestamp, CLA version) is stored and available for your records.

## Development workflow

1. Fork the repository.
2. Create a feature branch: `git checkout -b your-feature-name`.
3. Make your changes in `templates/` or `tools/`. Add tests where appropriate.
4. Run `just check` (and `just check-schemes` if you touched rendering).
5. Commit following [Conventional Commits](#commit-messages); "why" is more useful than "what."
6. Push to your fork and open a pull request against `main`.
7. Sign the CLA if prompted.

## Reporting security issues

Please do **not** file security issues as public GitHub issues. Email **security@minimal.dev** instead. See [SECURITY.md](./SECURITY.md) for scope and what to include.

## Code of Conduct

This project follows the [Contributor Covenant](./CODE_OF_CONDUCT.md). By participating, you agree to uphold it.

## Questions?

Open a [discussion](https://github.com/gominimal/cozyloadout/discussions), file an issue, or email **security@minimal.dev**.
