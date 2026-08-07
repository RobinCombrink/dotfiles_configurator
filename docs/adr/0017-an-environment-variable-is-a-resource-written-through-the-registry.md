# An environment variable is a resource, written through the registry

Status: accepted (2026-08-07, design session on self-provisioning)

An environment variable is a resource kind, in two shapes. A **variable** has a whole value, and
its desired state is that value. A **search path entry** is a directory the search path contains,
and its desired state is membership rather than the whole path — additive, as ADR 0005 requires,
because this tool never owns the entirety of a value the machine and other installers also write.

The promotion is the one the glossary describes, and the instances have accumulated. `EDITOR` and
`VISUAL` are set by a command with no presence check, so they drift on every run. A search path
entry is a notice asking a person to add a directory by hand, which is a message about something
the tool cannot do. The directory a released binary installs into is a third, and it is the one
that makes the tool's own provisioning close rather than end at an instruction.

The value is written through the user's environment key in the registry rather than through
`setx`. `setx` takes its value from the process environment, which is the machine's entries and
the user's already merged and already expanded, so writing a search path with it copies the
machine's entries into the user's. It can store an expandable value as a literal one, which
discards placeholders permanently. It is reported to truncate long values, at a limit commonly
cited as 1024 characters and not measured here. Microsoft's own guidance is to use something else
for the search path. The write reads the key raw, preserving both the unexpanded data and the
value type, appends, writes back, and broadcasts the environment-change message.

Actual state is read from the registry and never from this process's environment, which is a
snapshot taken at launch and answers for the machine as it was rather than as it is — the same
reason **Actual state** is defined as read from the machine rather than from a record.

## Considered options

- **A search path entry alone**, without the variable shape. The smallest change that closes the
  tool's own provisioning. Rejected because the registry handling is the substance and it is
  identical for both, so scoping it to one shape leaves the command that sets `EDITOR` drifting on
  every run and leaves the next reader to re-derive why the code is written the way it is.
- **Keep `setx` behind a presence check.** Rejected because a presence check makes the command
  idempotent without making it correct: the failure modes above are in the write itself.
- **A destination-on-the-path option on the released binary shape.** Rejected because it couples
  two independent facts and serves nothing but the one resource, leaving the notice a notice.

## Consequences

- **A variable this tool sets is never unset.** Convergence is additive, so withdrawing a
  declaration ends the tool's interest in a variable rather than removing it from the machine.
- **The change is invisible to every process already running**, including the shell that launched
  the tool, which is what broadcasting the change can and cannot do.
- **Only the user's environment is written.** A directory present on the machine-wide path is
  still absent from the user's, so deciding whether that is drift requires reading both keys, and
  appending without reading both produces a duplicate that works and should not exist.
- **The implementation is specific to this platform**, in a program that does not otherwise depend
  on one.
