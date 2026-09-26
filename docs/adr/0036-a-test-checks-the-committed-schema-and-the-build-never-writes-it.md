---
status: accepted
---

# A test checks the committed schema and the build never writes it

The configuration schema stays committed, because it is published: the dotfiles repository's
editor settings read it from `main` and this repository's read it from the working tree. It is
still derived from the types the tool reads configurations with. What changes is who derives it:
a test renders `schema_for!(Configuration)` and fails when the committed file differs, and the
same test writes the file when `UPDATE_SCHEMA` is set. The build script no longer touches it.

This supersedes [ADR 0031](0031-the-configuration-module-tree-compiles-under-two-crate-roots.md),
which had the build script write the schema into the source tree on every build. Nothing then
compared the committed copy with the types: a commit that changed the types without staging the
regenerated file left CI rebuilding, rewriting the tracked file, and passing. Every build also
mutated a tracked file. The objection 0031 raised against a test — that nothing regenerates the
schema as part of a build, so it goes stale until a later run notices — is answered by the test
failing: a stale schema fails the ordinary test run, locally and in CI, rather than going
unnoticed.

With the build script no longer compiling `src/configuration.rs`, the configuration module tree
compiles under one crate root. The rule that nothing under `src/configuration/` reaches
`crate::machine` stays as the direction ADR 0006 already pushes, but no compiler checks it.

## Considered options

- **Generate into `OUT_DIR` and fail the build on a difference.** It breaks every build,
  rust-analyzer's included, midway through a type change.
- **Keep the in-place write and check cleanliness afterwards.** It keeps a build that mutates
  tracked files.
- **`expect-test`.** The same comparison with its own update switch, but its last commit and
  release were 2024-12-21.

## Consequences

- **Regenerating the schema is a deliberate act.** A type change fails the schema test until the
  file is regenerated and committed with it.
- **The build script sets the Windows icon and nothing else**, and has no build dependencies
  beyond the Windows-only resource compiler.
- **A configuration module importing from `crate::machine` compiles.** Keeping the tree
  machine-free is a review concern until something checks it again.
