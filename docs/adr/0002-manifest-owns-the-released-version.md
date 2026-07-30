# The Cargo manifest owns the released version

Status: superseded by ADR 0003 (2026-07-29) — never implemented. Two of its premises did not
survive contact: the write-back plugins are unavailable to callers, and a binary built before
the release cannot carry the version that release cuts.

`Cargo.toml` is the single home of this crate's version; semantic-release writes it back as
part of the release, and the tag is derived from it rather than competing with it. This is
delivered by a caller-local `.releaserc.json` in this repo, which the `actions-release` fleet
workflow already prefers over its bundled default — so no fleet change and no effect on other
callers.

The defect this fixes was measured, not suspected: git tags had reached `v1.0.3` while
`Cargo.toml` still said `0.2.0`, fourteen releases apart. Because clap's `#[command(version)]`
reads `CARGO_PKG_VERSION`, a binary shipped as `v1.0.3` reported itself as `0.2.0`. A version
number is a fact, and a drifted copy of a fact that the program prints is an executable lie.

## Considered options

- **Stamp the git description in at build time** via `build.rs`, leaving the manifest version
  meaningless. Keeps `main` free of release commits and needs no config. Rejected: it fixes
  what `--version` prints but not what any Cargo-level consumer sees, since those read the
  manifest.
- **Manifest is the source of truth, CI enforces agreement**: bump by hand in the feature
  commit, fail the release when the computed version and the manifest disagree. Rejected: it
  reintroduces a manual step per release, reversing the no-release-friction requirement that
  ADR 0003 in dotfiles used to reject release-please.
- **Leave it**: acceptable only while nothing downstream reads the version. It stops being
  acceptable the moment a machine has to answer "is the installed version the released one?".

## Consequences

- A release commit now lands on `main` for every release.
- **The three overlapping release paths must be reduced to one first.** `build.yml` (tags,
  Windows, `action-gh-release@v1`), `ci.yml`'s release job (tags, three platforms,
  `@v2`) and the fleet `release.yml` (push to main) currently coexist, and two of them publish.
  Version write-back cannot be correct while two publishers race for the same release.
- Because this repo ships as a downloaded binary rather than through Cargo, `--version` is the
  only way a person can identify what they are running. That is what makes this worth doing
  here specifically.
