# A version is the generation its reader must meet

Status: accepted (2026-08-07, design session on self-provisioning)

A configuration's `version` states the lowest generation of this program that can read it, rather
than the generation it was written against. A build reads any configuration whose stated
generation is at or below its own, migrating older shapes forward as it reads them, and refuses
one above by naming both generations.

The field was an exact identity, and an exact identity describes the wrong thing. A configuration
using only long-standing shapes is readable by every build from the generation that introduced
them onward; only one using a new shape needs a new build. What the number bounds is the reader,
not the document.

That makes generations unequal in the way they actually differ. A change that only adds a shape
raises the stated generation of the documents that use it and of no others. A change that
restructures an existing shape is migrated on read, and no file is edited by hand. A change to the
meaning of an unchanged shape is the remaining flag-day, and is now the rare case rather than
every case. Three generations have existed and two migrations were performed by hand, on every
configuration at once, for changes of the first two sorts.

No library supplies this. The crates that carry the name are layered loaders, which merge several
sources into one value — a problem this program does not have and has three decisions against
having, since a configuration is read whole, applicability is declared by the document, and any
unreadable file refuses the run. The crates that do version a document are a thin field: measured
2026-08-07 on the registry, the most-used one serves a binary format only, and every candidate
that reads JSON was either last released more than two years earlier or has a single release. The
migration chain is a sum type of document generations and conversions between neighbours, on the
serialisation library already in use.

## Considered options

- **Keep the exact match and migrate by hand.** It has cost two edits of two files across three
  generations, which is genuinely small. Rejected because the cost is paid by every configuration
  simultaneously at a moment chosen by the program rather than by their author, an older checkout
  can never be read at all, and a stated generation above the build reads as a parse failure
  rather than as an instruction.
- **A versioning crate.** Rejected on maintenance: the field is either stale or unproven, and a
  format this program's every input is expressed in is the wrong place to depend on either.
- **Ignore shapes a build does not recognise**, which removes the refusal entirely and lets an old
  build converge what it understands. Rejected because it reverses ADR 0009 on the grounds that a
  partial desired state is a different desired state rather than a smaller one.

## Consequences

- **Every migration is permanent.** A conversion written once is carried forever and is exercised
  by a test, because the documents it exists for are the ones nobody is looking at.
- **Two configurations read together may state different generations**, and both are read when
  both are at or below the build. A floor is a property of each document rather than of the set.
- **A refusal names the generation required and the generation available**, which is what makes it
  actionable rather than a parse error.
