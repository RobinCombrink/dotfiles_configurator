# A workspace member's actual state is its binaries on disk

Status: accepted (2026-08-09, grilling session on locked binaries and partial installs). Refines
ADR 0007.

A workspace member is read by asking which of its declared binaries are present in the directory
cargo installs into, alongside the fingerprint comparison ADR 0007 established. A member missing
any declared binary drifts, and the drift names the ones absent.

Actual state is what is observably true of the machine, read from the machine itself rather than
from any record of past runs. `cargo install --list` is such a record, and it was the only thing
this kind consulted. The two disagree the moment an install is rolled back part-way: a denied move
aborts the install, cargo removes the binaries it had already written, and the install record
survives naming every one of them. The fingerprint then matches forever and no run repairs it.

Measured 2026-08-06 on `ROBIN-NTB`: three of `session-mining`'s declared binaries resolved nowhere
on PATH while four siblings were present, and `plan` closed `8 to change, 0 blocked, 46 already
converged` without naming the workspace. Cargo's record agreed with the configurator rather than
with the disk. The state was escapable only by hand, with `cargo install --force`.

Which binaries a member declares is resolved from the manifests at the tracked revision, from the
same three places ADR 0007 already consults to decide whether a member builds a binary at all: the
manifest's `[[bin]]` sections, `src/main.rs` named for the package, and the files under `src/bin/`,
each named for its stem or, where it is a directory holding `main.rs`, for that directory. That
reading changes from a question answered yes or no into one answered with names.

The three compose the way cargo composes them rather than as a union: a `[[bin]]` section suppresses
the target that would otherwise be inferred at its name or at its path. Measured 2026-08-09 against
the workspace this program installs from, `cargo metadata` resolves `session-mining` to `sweep` and
`tool-use-statistics` and to nothing else. A union would have added `tool_use_statistics`, read off
the file stem that the second section already claims, and drifted that crate forever on a binary no
build has ever produced — the failure this decision exists to close, inverted.

The unit of drift stays the crate. ADR 0007 chose per-crate fingerprints on cost, and repair is a
crate-level act regardless; the absent names travel as the reason the crate drifts rather than as
resources of their own.

## Considered options

- **Treat it as the install path's problem** — never leave a partial install behind, and keep
  convergence revision-only. Cheaper by a stat per binary and by the name resolution above.
  Rejected because it leaves convergence assertable against a machine the tool never looked at, and
  because this failure is silent and self-perpetuating: the instruments were missing for an unknown
  period, and what surfaced it was a sweep trying to run them, not a run of this program.
- **Read the declared names from the install record's own `bins` list.** It is already correct
  about what should be on disk and needs no manifest parsing. Rejected as the record again, and it
  holds nothing at all for a crate that has never been installed, so a fresh machine would learn
  the names only after the first install had already succeeded.
- **One resource per declared binary**, each claiming the path it occupies. It reports precisely
  which are missing and permits `cargo install --bin`. Rejected because identity is the fact a
  declaration claims, and the configuration declares a member of a workspace, not seven
  executables; it also splits an act cargo performs atomically.

## Consequences

- **Planning costs a stat per declared binary per member.** Nothing else about reading a member
  changes.
- **A binary deleted by hand is reinstalled.** It is drift like any other, and closing it makes a
  declared thing true rather than an undeclared thing false, so ADR 0005 is untouched.
- **Repair rebuilds every binary the member declares**, because the reinstall is per crate. A
  member missing one of seven pays for all seven.
- **A binary that a member stops declaring is left where it is.** Withdrawal ends the tool's
  interest without removing anything, per ADR 0005, so a renamed binary leaves its predecessor on
  disk and undeclared.
- **A member whose manifest cannot be read still refuses the whole run**, as ADR 0007 records.
  Resolving names rather than a boolean adds no new way for that reading to fail.
