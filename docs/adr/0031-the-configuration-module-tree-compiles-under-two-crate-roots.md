# The configuration module tree compiles under two crate roots

Status: accepted (2026-09-13, comment-home review of the configuration module tree)

The configuration schema is derived from the same types the tool reads configurations with, so it
cannot describe a document the tool would refuse. Deriving it means the build script compiles
`src/configuration.rs` itself, through `#[path]`, into a compilation unit of its own — one whose
crate root knows `configuration` and `version` and nothing else.

That unit and the crate proper are two roots over one file, and the file has to compile identically
under both. Two constraints follow, and neither root states either of them.

A `#[path]`-loaded module's children resolve against the directory of the file that included it,
not against a directory named after the module. Left to the default, `pub mod context;` in
`src/configuration.rs` names `src/configuration/context.rs` to the crate and `context.rs` beside
the build script to the build script. Every child is therefore named outright, as
`#[path = "configuration/context.rs"]`, which resolves to the same file from either root.

Nothing under `src/configuration/` may name anything outside `configuration` and `version`.
`crate::machine` exists in the crate and not in the build script's unit, so an import reaching for
it compiles under one root and not the other. That is the direction the dependency should run
anyway: a configuration describes what a machine ought to be and never reads one, which is why
`version` is the only other module the build script has to load.

A violation fails the build script rather than the crate — an import that cannot resolve as
`E0432`, a module named without its path as a file that does not exist — and it fails on every
build and every `cargo check`, before the crate is compiled at all. The constraint is enforced, not
remembered. What it is not is self-evident: from inside the crate the explicit `#[path]` attributes
read as redundant, because under that root alone they are.

## Considered options

- **Hand-write the schema.** It drifts from the types the moment either moves, and nothing stops it
  describing a document the tool would refuse — which is the one property deriving it buys.
- **Move the configuration types into a crate of their own, depended on by both the build script
  and this one.** The shape that removes the constraint rather than recording it, at the cost of a
  second crate and a published boundary serving a single consumer. Worth revisiting if anything
  under `src/configuration/` ever genuinely needs to name the machine.
- **Generate the schema from a test or a binary rather than a build script.** Nothing then
  regenerates it as part of a build, so the committed schema goes stale between an edit and
  whichever later run notices.
- **`include!` the file instead of loading it as a module.** The same two roots over one file, with
  diagnostics that name the including line rather than the offending module.

## Consequences

- **Every module under `src/configuration/` carries an explicit `#[path]`**, the ones added later
  included. One added without it breaks the build script, so the error names a file its author was
  not editing.
- **The configuration tree cannot reach the rest of the crate.** A fact about a machine that
  configuration needs is passed to it rather than read by it, in the same direction ADR 0006's
  read-only capability split already pushes.
- **`version` is load-bearing in the build script.** It is there because the configuration types
  name `Version`, so removing that import is what would let the second `#[path]` module go, and
  adding an import to a third module is what would make a third necessary.
- **The build script compiles a second copy of the configuration types on every build.** They are
  small, and the alternative is a crate boundary.
- **`#[allow(dead_code, unused_imports)]` is permanent on both modules the build script loads.**
  Only `Configuration` is reachable from `schemars::schema_for!`, so everything the crate itself
  uses reads as dead from that root.
