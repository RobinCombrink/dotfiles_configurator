# Apply shows its change set and reports as it acts

Status: accepted (2026-08-06, grilling session on previewing, convergence triggers and the
push window)

`apply` prints the change set before it enacts anything, and asks once — at the first change set —
whether to proceed. `--yes` skips the asking and never the printing. Where there is no terminal and
no `--yes`, the run is refused rather than proceeding unseen. Both subcommands render what they are
doing while they do it, and every child process's output is written to a log for the run.

The preview is what makes a wrong configuration visible; the confirmation is what makes it
stoppable. A separate subcommand cannot do that job, because it has to be remembered and has to be
run against an identical source, and the source is what goes wrong.

Refusal rather than silent proceeding is what keeps "nobody saw this change set" out of reach by
accident. Detection decides whether asking is possible at all, so a prompt cannot appear where it
would hang: `post-rewrite` receives the rewritten commits on its standard input, and a confirmation
reading that stream would take a commit hash for an answer.

Progress needs no instrument built here. Reading state captures a child's output because that output
is parsed, and enacting a change was capturing it too — so a crate that takes minutes to compile
reported nothing while cargo was reporting continuously. What the child already emits is written to
the log as it arrives.

A resource that has gone silent is reported and never cut short. Bounding the run by time was
rejected in `dotfiles` ADR 0008, which accepted that a fresh machine blocks for as long as
provisioning takes, and no threshold has a principled value against a single crate measured at
6m23s.

## Considered options

- **A separate subcommand as the preview.** Already exists, and did not prevent the run it would
  have caught.
- **Write each outcome as it is reached, and nothing else.** Fixes output arriving in one batch at
  the end. Leaves the case that motivated this untouched, because a resource taking minutes produces
  no outcome to write during them.
- **A global time budget**, deferring the remainder to the next trigger. Rejected above.
- **A periodic elapsed-time heartbeat.** Invents an instrument to approximate output the child
  already produces.
- **A second flag disabling interactivity.** The prior art carries one because it spans many
  commands that prompt for many things. There is one prompt here, and a caller that wants none
  already has `--yes`.

## Consequences

- **The confirmed change set is the first pass's.** Later passes converge resources that became
  ready once earlier ones landed, and are not confirmed again.
- **A converged run prints its summary and nothing more.** These triggers fire on every commit, and
  a tool that speaks on every commit is a tool whose hooks get removed. The log is written anyway:
  the run that turns out to have been interesting cannot be re-instrumented afterwards.
- **The log holds the arguments of every invocation, verbatim.** Redacting them was considered and
  rejected: a configuration is fetched from a repository, so a value written into one is already
  committed before any log exists, and a log that hides it would make the tool tolerant of a
  declaration nobody should be able to write. The constraint belongs to what a configuration may
  hold, not to what the log may print.
- **Logging is process infrastructure and sits outside the capability split**, so producing a change
  set writes a log without acquiring any ability to change a resource. ADR 0006 states the guarantee
  about the machine's declared resources.
- A run refused for want of a terminal is a run that did nothing, which is neither converged nor
  failed, and the exit status has to say so.
