---
status: accepted
---

# A binary a workspace member stops declaring is removed

`plan` names, and `apply` removes, a binary that a cargo install record of a declared workspace
member names but that the member's manifest, at the revision the workspace is read at, no longer
declares. It is the configurator's first uninstall, and the exception ADR 0005 left room for:
ownership the machine itself reveals, without a receipt of past runs.

Ownership is derived, never declared. A record belongs to a member when it carries the member's
name and was installed from git out of the repository the workspace is cloned from, whichever
account fetched it. The binaries to remove are the record's binaries less the ones the member
declares now, resolved as ADR 0021 resolves them. Nothing else in cargo's bin directory is named
or touched, and a crate from the registry is out of scope: its record carries no revision, so
two records of one package at two revisions cannot arise for it.

Measured 2026-10-05 against cargo 1.99.0 in a throwaway `CARGO_HOME` on Windows 11:

| What was done | What cargo did |
| --- | --- |
| Reinstalled `--force` at a revision whose binaries overlap the earlier install's | Kept one record, and deleted the binary the new revision dropped |
| Reinstalled `--force` at a revision whose binaries are disjoint from the earlier install's | Kept both records, each naming its own binaries |
| `cargo uninstall --bin` a binary of a record one of whose files had been deleted | Refused: "corrupt metadata, `…` does not exist when it should" |
| The same, after an empty file was written at the deleted path | Removed the binary and its entry in the record |
| `cargo uninstall --bin` a binary that was running | Removed it, and the process kept running |

So a removal runs `cargo uninstall --bin <name>` with the record's whole specification,
`git+<source as cargo lists it>#<name>@<version>`, which is what tells two records of one package
apart. Where cargo refuses the record as corrupt, an empty placeholder is written for each binary
the record names that is absent, and the uninstall is run again. The record is never edited: it
is cargo's file, in cargo's format. A running binary is removed as any other is, since cargo was
not refused, and ADR 0022 displaces only on a refusal.

## Considered options

- **Leave it where it is**, as ADR 0021 did. Rejected: a binary a member no longer declares stays
  on the search path under a name nothing builds any more, and a record whose files are already
  gone can be repaired only by a person who knows the placeholder trick.
- **Edit cargo's install record.** Rejected: it is cargo's state in a format cargo owns, and
  cargo already offers an uninstall that keeps both of its record files consistent.
- **Let machine-reaper remove it.** Rejected: only the configurator knows what is declared.

## Consequences

- **A removal is a change of its own kind**, beside the change a resource makes, with its own
  count. A pending removal leaves the plan unconverged and the exit status reflects it, as any
  change does.
- **ADR 0021's consequence that a binary a member stops declaring is left where it is no longer
  holds.** A member withdrawn from the workspace altogether is still left alone, since nothing
  then declares it.
- **A record whose files are all gone is still removable**, and removing its last binary
  removes the record.
