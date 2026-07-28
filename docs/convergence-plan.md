# Convergence plan

Where this crate is going, and in what order. Decided in a grilling session on 2026-07-28,
following a multi-angle review of the repository. The review itself was a session artifact and
is not held in this repo; every finding this plan depends on is restated here.

The decisions are recorded in `docs/adr/`; the vocabulary is in `CONTEXT.md`. This document
holds only the sequence, which is the part neither of those should carry.

## What changed between the review and this plan

The review's route assumed a codebase being repaired. It isn't — it is being re-modelled. One
reframe did it: **idempotency is not four patches, it is one property.** A tool that reads what
is already true and changes only what is not is idempotent by construction, and the
per-item-kind fixes the review proposed become either unnecessary or state readers.

A second reframe came from refusing an X-Y problem. The staleness question — "is this installed
binary older than its source?" — looked like it needed timestamps or a state file. It needed
neither. It needed the version to stop lying.

## Decisions

| Decision | Recorded in |
|---|---|
| Configuration is a declared desired state, applied as a change set | ADR 0001 |
| Applications keep download-and-run, gain a declared presence check | ADR 0001 |
| Commands stay as the escape hatch — they are where resource kinds are born | ADR 0001 |
| Breaking v2 format; `Configuration.version` finally gates the load | ADR 0001 |
| The Cargo manifest owns the released version | ADR 0002 |
| `tools/*` install from a git revision; drift is the crate's subtree hash | dotfiles ADR 0007 |
| `post-merge` + `post-rewrite`, applying everything, synchronously, unbounded | dotfiles ADR 0008 |

Two decisions are recorded only here, because neither is hard to reverse.

**This crate stays its own repository and ships as a release binary.** The reason is a cycle:
it installs Rust, so it cannot itself be installed by Cargo. That rules out folding it into the
`dotfiles` Cargo workspace alongside the other tools, which `workspace-marginal-cost` would
otherwise favour.

**The quality gate is repaired before the model is built.** Three of the four signals a change
relies on are currently wrong, and correcting that is four small commits — cheap enough that
building on top of a lying baseline is not worth it.

## Phase A — Make the signal honest

Four atomic commits. Nothing else starts until this is done.

1. Add `CI / Build, lint & test` to the required checks, in `.github/fleet-ruleset.json` **and**
   the live ruleset. It is absent from both, which is why a red `main` still merges.
2. `cargo fmt --all` — 17 hunks across 10 files. This is what CI dies on today, before it
   reaches clippy or the tests at all.
3. Fix the six `useless_borrows_in_formatting` errors in `src/execution/item_progress.rs`.
4. Delete the five unused `Default` impls. Not cleanup: one is the `unix` `ShellCommand` impl
   missing a field, so **deleting them repairs the Linux and macOS builds**.

**Done when** `ci-checks verify head` returns `passed` for the pushed commit — the verdict, not
an impression that the tests look green.

## Phase B — Make the version true

1. **Reduce three release pipelines to one.** `build.yml`, `ci.yml`'s release job and the fleet
   `release.yml` coexist, and two of them publish. Write-back cannot be correct while two
   publishers race for the same release. This comes first.
2. Add a caller-local `.releaserc.json` with manifest write-back. `actions-release` already
   prefers a caller's config over its bundled default, so this is a per-repo change with no
   fleet impact.
3. Verify against a real release: the tag moved, `Cargo.toml` moved with it, and
   `dotfiles --version` agrees with both.

## Phase C — Prove the one unverified assumption

Before any code depends on it.

1. Install one small crate with `cargo install --git` and inspect `~/.cargo/.crates2.json`.
   **dotfiles ADR 0007 rests entirely on Cargo recording `git+<url>#<sha>`**, and no
   git-sourced install existed to confirm it. If it records something else, that decision needs
   rework — cheaper to learn now than after the diff engine is built on it.
2. Time a cold `cargo install --git` for the three `dotfiles/tools` crates. An unbounded apply
   was accepted; this is the number that says what it costs on a pull.

## Phase D — The domain model

The bulk of the work.

1. v2 configuration types — the seven resource kinds, presence checks, notices, version gate.
2. **Emit the JSON Schema.** `schemars` is derived on 14 types and `schema_for!` is called
   nowhere, which is why one absent file has three broken references: the README's design
   decision, `.vscode/settings.json` pointing outside the repo, and `build.yml` uploading it.
   Generating it repairs all three.
3. State readers, one per kind. A kind that cannot answer "how is this read back?" stays a
   command until it can.
4. Change-set construction — ordered, deterministic, printable. The `HashMap` goes; it is what
   makes today's dry-run output unstable and therefore undiffable.
5. Apply engine — continue and collect on failure, ending with a clear summary of what failed.
6. `plan` and `apply` subcommands, and **remove the blocking `stdin` read**; a hook has nobody
   to press a key.
7. Migrate the 41 live items in the `dotfiles` repository's `config/`.

`.feature` files precede or accompany the tests, for user-observable behaviour only. The change
-set engine's internals get plain unit tests.

## Phase E — The hook

In the `dotfiles` repository: `post-merge` and `post-rewrite` jobs in `lefthook.yml` invoking
`apply`. Both are required — with `pull.rebase = true`, a fast-forward pull fires `post-merge`,
and a pull replaying local commits fires `post-rewrite` and never `post-merge`.

## What the review found, and what becomes of it

**Dissolved by the model** — these stop being defects and become behaviour: symlink
idempotency; `HashMap` execution ordering; the missing plan/effect seam; `ItemProgress`'s
ignored path parameter; the `ExecutionItem`/`DownloadType` duplication; the `ExecutorSync` IOU;
`require_output` honoured by one shell variant of four; and the two tests that pass for the
wrong reason together with the four obsolete fixtures, since the loader they exercise is
replaced.

**Survives, because applications keep downloading** — still owed as real work: the
asset-matching `unwrap` that panics on a mistyped key; the `metadata()?.len() - 1` underflow;
resume appending a duplicate byte with no ranged-response status check; `octocrab::instance()`
running unauthenticated beside an authenticated clone; fetch-without-checkout, which becomes
the repository state reader; the WSL variant that does not use WSL, `bash` invoked without
`-c`, and PowerShell's broken quoting; and `exit(0)` four frames deep.

**Promoted to blocking, because the hook runs unattended:**

- `--debug` silently relocating the home and download directories. A hook running this would
  write symlinks into `C:\Test\Home`.
- The blocking `stdin` read — a hook cannot answer it.
- Shell errors reporting `stdout:` and `stderr:` as always empty because the pipes were nulled.
  Doubly blocking now, since captured output is what a command's presence check reads.

## Open items

- **Nothing installs this binary.** The hook shells out to a tool whose presence is unmanaged.
  Staying a separate repo settled where it lives, not how it arrives.
- **Windows remains the only real target.** Phase A makes it *compile* on Linux and macOS; the
  WSL and Command Prompt shells, `winget`, and the hardcoded `windows_amd64.msi` bootstrap mean
  it does not *work* there.
