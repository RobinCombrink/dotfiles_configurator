# The build stamps the released version

Status: accepted (2026-07-29), supersedes ADR 0002, which was never implemented

The committed manifest version is a placeholder — `0.0.0-dev`. CI asks `actions-release` for
the version it is about to cut, stamps that into `Cargo.toml` and `Cargo.lock` immediately
before the release build, and the binary carrying it becomes the release asset. Nothing is
written back to the repository.

ADR 0002 decided the opposite, and its own reasoning refutes it. It rejected build-time
stamping because stamping "fixes what `--version` prints but not what any Cargo-level consumer
sees, since those read the manifest" — while recording, four paragraphs later, that this repo
"ships as a downloaded binary rather than through Cargo". There are no Cargo-level consumers to
protect. The objection describes a crate published to a registry, which this is not.

Two things measured while implementing 0002 confirmed it:

- **Write-back is not free.** 0002 asserted it needed "no fleet change and no effect on other
  callers". `@semantic-release/exec` and `@semantic-release/git`, which perform the write-back,
  are not bundled with semantic-release, and callers cannot add plugins — so the fleet workflow
  had to change before a caller-local config could do anything a bundled plugin cannot.
- **A build cannot carry a version that does not exist yet.** The binary compiles from the tree
  as checked out, so write-back at release time would have attached a binary reporting the
  *previous* version to each release — precisely the drift 0002 was written to end.

## Considered options

- **Write the version back into the manifest** (ADR 0002): rejected above. It also puts a
  release commit on `main` for every release, which 0002 recorded as a cost.
- **Stamp from `git describe` in `build.rs`**: no authority over the number. The tag does not
  exist when the build runs, so the build would guess at the version the release will choose
  rather than being told it.
- **Accept the lag** — ship each release with the previous release's version embedded:
  rejected. It is the bug, not a mitigation of it.

## Consequences

- The manifest version in the repository is meaningless, and deliberately looks it. A binary
  that escaped without being stamped reports `0.0.0-dev`; the build fails if the stamped
  manifest or the built binary disagrees with the version it was given.
- The version is computed twice per release — once under `dry_run` to stamp, once for real to
  publish — from one set of release rules in the fleet's bundled config. This repo carries no
  `.releaserc`, so there is no second copy of that policy to drift.
- `cargo build --locked` is usable in the release build, because the stamp updates the manifest
  and the lockfile together.
- Because this repo ships as a downloaded binary, `--version` is the only way a person can
  identify what they are running. That is what makes any of this worth doing here.
