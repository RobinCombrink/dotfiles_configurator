# Plan cannot change the machine

Status: accepted (2026-07-31, grilling session on the Phase D domain model)

Producing a change set and enacting one are given different capabilities. Plan holds capabilities
that can only read; apply holds those and the ones that write. "Plan had no side effects" is
therefore a property the compiler checks, not a convention that survives only as long as everyone
remembers it — and the tool's entire claim on a person's trust is that plan tells them what apply
will do.

Two things were needed to make that guarantee real rather than nominal.

**What may be run is typed, not just who may run it.** Collapsing "run a process" into one
general capability would have kept the surface small and handed plan the ability to run an
installer. Instead there are two closed sets: invocations this crate defines for reading state,
which cannot be constructed from outside it, and presence checks, which come only from a
resource's declaration. Plan has no capability that accepts anything else.

**Presence checks are structured rather than arbitrary shell.** An author-declared shell command
cannot be proven side-effect-free, so a check taking any form would have made the guarantee a
slogan. Checks are chosen from a fixed set, most of which — asking whether a path exists — cannot
change anything by construction. What remains is a narrow, deliberate escape hatch.

So the guarantee is precise, and stating it precisely matters: **plan cannot change a machine
through anything the tool decides; it can change one only through a check the configuration's
author wrote and declared as a check.**

## Considered options

- **Convention, enforced by review**: costs nothing to adopt and nothing to violate. Rejected —
  the property is the product's central claim, and the review that catches its erosion is the one
  nobody runs on a quiet refactor.
- **One capability per kind of read** (`winget_state`, `installed_packages`, and so on), with the
  parsing of each tool's output behind it. Rejected: the capability surface would grow with every
  resource kind, and output parsing is exactly where this codebase's bugs have been — the review
  found it answering "is this installed?" by looking for an installer file. Parsing behind the
  capability means the tests that matter most exercise a substitute instead of the parser.
- **A general process capability, with a lint or a naming convention** keeping plan honest: the
  narrow surface without the guarantee, which is the wrong half of the trade.

## Consequences

- Reads go through capabilities rather than reaching for the filesystem or spawning processes
  directly. That is a real constraint on how kinds are written, and the point.
- Each kind receives only what it uses — a symlink is handed no way to run anything — so least
  privilege between kinds falls out of the same mechanism rather than needing its own.
- Substituting a machine in tests stops being a special mode of the program. This retires the
  `--debug` flag's relocation of the home and download directories, which existed to make the
  tool testable and could silently converge a machine into the wrong home.
- Parsing a tool's output belongs to the kind that understands it, which is what makes the
  encoding of that output the kind's problem to get right.
