---
status: accepted
---

# An apply that updates the configurator hands the run to the new build

When apply installs a newer release of this program, nothing else in that run is converged by the
build it replaced. Apply starts the installed build with the arguments it was given and exits with
that build's exit status; the new build plans afresh, finds itself current, and carries on.

The build that plans is the build that enacts. Carrying on in the old image applies a change set
computed by code the run has just declared outdated, so a fix to planning or linking lands one run
late — and that run can do the damage the fix prevents. On 2026-09-25 on the personal machine,
3.18.0 installed 3.18.1, which carries the fix for relative link sources, and then rewrote every
file symlink with a relative target, breaking `~/.gitconfig` and `~/.claude/settings.json`.

The build started is the one at the installed path in the tool directory, which is where the update
was written whatever location the old build was started from. The path the running process reports
for itself is not that: measured 2026-09-26 on Windows 11 Home 10.0.26200, `current_exe()` of a
process whose image had been renamed aside still reported the original name, which holds the new
build only when the old one happened to run from the tool directory.

One run replaces the configurator at most once. The successor is started knowing which build it
replaced, and a successor that still reads as behind its latest release reports its own update as
failed, naming the build it replaced, rather than installing and restarting again: a build whose
reported version disagrees with its release tag would otherwise restart without end.

The operator's answer travels with the run. The successor is started answered in advance, so a
change set confirmed by the build it replaced is not put to the operator a second time; `--yes`
passes through as any other argument does.

What the successor is told travels as a hidden argument rather than an environment variable. An
environment variable is inherited by every process the successor starts, including a declared
command that runs this program, which would then begin a run that believes it has already
replaced itself.

## Considered options

- **Continue on the image the run started with**, as ADR 0008 and ADR 0019 recorded: the ordinary
  meaning of replacing a running program. Rejected on the incident above.

## Consequences

- **The old process lives until its successor exits**, still executing its superseded image, so
  the successor's sweep cannot delete that image and the next run's does.
- **Plan never hands over.** It enacts nothing, so it installs nothing to hand to (ADR 0006).
- **A held or failed self-update leaves the run in the build that started it**, which converges the
  rest of the change set as before; only an update that was placed hands over.
