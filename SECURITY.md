# Security Policy

Thanks for helping keep the cozy loadout and its users safe.

## Reporting a vulnerability

Please report vulnerabilities by email to **security@minimal.dev**. We
will acknowledge the report and coordinate a fix and disclosure with you
privately.

Please do **not** report security issues via public GitHub issues,
discussions, or pull requests — that discloses the problem to everyone
before a fix exists.

When reporting, include what you can of: the affected file or recipe,
the commit of this repository, reproduction steps or a proof of concept,
and your assessment of the impact. If you'd like to encrypt your report,
say so in your first email and we'll coordinate a key exchange.

## No bug bounty

This project does not operate a paid bug bounty program. We genuinely
appreciate good-faith reports and are glad to credit researchers in the
disclosure, but we do not offer monetary rewards.

We reserve the right to disregard low-effort, unsolicited "beg bounty"
reports — for example, automated scanner output or theoretical findings
with no demonstrated impact on the components in scope below. For
background on why, see [Troy Hunt on beg
bounties](https://www.troyhunt.com/beg-bounties/).

## Supported versions

This repository has no release channels. Security fixes land on `main`;
rebuild and reinstall the loadout to pick them up:

```shell
git pull && just theme && just install
```

Reproduce against the current `main` before reporting — the issue may
already be fixed.

## Scope

What this repository ships is a renderer and a set of configuration
files. The interesting attack surface is therefore what a *scheme* or a
*template* can make the renderer produce, and what the generated files
then do inside a session.

In scope:

- **The renderer** (`tools/cozy-theme`): anything that makes it write
  outside its `--out` directory, or that turns a scheme file into
  arbitrary content in an unexpected place. Scheme files are the one
  untrusted input — the upstream collection is fetched from a third
  party by `just fetch-schemes`.
- **The generated loadout**: shell metacharacters, path traversal, or
  command injection reaching `hooks/on-activate.sh`, `fish/config.fish`,
  or a patch destination by way of a scheme's metadata or filename.
- **The `just` recipes**, particularly `install`, which deletes and
  replaces a directory under `~/.config/minimal/loadouts/`.

Out of scope:

- Vulnerabilities in the tools this loadout configures (helix, zellij,
  bat, delta, atuin, lazygit and the rest) — report those upstream.
- Vulnerabilities in minimal itself, its sandbox, or its package
  registry — those belong to
  [gominimal/minimal](https://github.com/gominimal/minimal/security),
  under the same address.
- Colour choices that fail a contrast target. Those are real bugs and we
  want them filed, but as issues rather than security reports.
- Issues that require an already-compromised host.

## Safe harbor

We will not pursue or support legal action against researchers who:

- Make a good-faith effort to comply with this policy.
- Avoid privacy violations, destruction of data, and interruption or
  degradation of others' use of this project.
- Give us reasonable time to investigate and address a reported issue
  before any public disclosure.
