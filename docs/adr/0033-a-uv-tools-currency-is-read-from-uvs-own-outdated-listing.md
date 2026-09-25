---
status: accepted
---

# A uv tool's currency is read from uv's own outdated listing

A uv tool is a package whose desired state is the newest version that resolves, so an assessment
needs two facts: the version installed, and whether a newer one resolves. Both are read once per
change set, as ADR 0010 requires of a source that answers for a whole set: `uv tool list` for what
is installed, and `uv tool list --outdated --no-offline` for the tools a newer version resolves
for, each listed with that version. A tool the first names and the second does not is converged.

Measured 2026-09-25 with uv 0.10.12 on Windows 11, against serena-agent installed at 1.5.3 with
1.7.0 on PyPI:

| Reader | Answer | Read-only | Index unreachable | Cost |
| --- | --- | --- | --- | --- |
| `uv tool list --outdated` | `[latest: 1.7.0]` | yes | exits 2 | 0.6s for every tool |
| `uv pip install --dry-run --upgrade --python <tool environment>` | `+ serena-agent==1.7.0` | yes | exits 2 | 2.8s per tool |
| `uv pip compile -` with the name on standard input | `serena-agent==1.7.0` | yes | exits 2 | 3.2s per tool |
| PyPI JSON `info.version` | `1.7.0` | yes | HTTP error | one request per tool |
| `uv tool upgrade --dry-run` | refused: no such flag | — | — | — |

The outdated listing asks uv the question uv's upgrade answers, against the index and interpreter
the tool was installed with, for every tool in one launch. Offline — `--offline` or `UV_OFFLINE` —
it leaves out every tool it cannot look up and exits 0, which would read as converged; the dry-run
reader does the same, answering "Would make no changes". `--no-offline` overrides the environment,
so the invocation carries it.

## A running launcher

uv upgrades a tool's environment and then copies each launcher from the environment's `Scripts`
directory into its bin directory. A launcher that is running refuses that copy with os error 32,
after the environment already holds the new version, and uv exits 1. Where that happens, the
refused launcher is compared byte for byte with the environment's copy: identical bytes mean the
running launcher already is what the upgrade would have written, and the tool is converged; any
difference is `held`, in ADR 0022's category. Any other failure stays a failure.

Measured 2026-09-25 with uv 0.10.12 on Windows 11, in a scratch tool and bin directory, with
`serena.exe start-mcp-server` running from the scratch bin:

- The running `serena.exe` could not be renamed, by MSYS `mv` or by .NET `File.Move`, while the two
  idle launchers beside it could. A running copy of `PING.EXE` in the same directory renamed
  freely, as ADR 0022 measured; uv's launcher holds its own file open in a way that forbids it.
- Moving the idle launchers aside before `uv tool upgrade` still failed on the running one.
- A name freed after a refusal was not written again by a retried `uv tool upgrade`, which answered
  "Nothing to upgrade" and left the launcher missing from the bin directory.
- After the refused upgrade from 1.5.3 to 1.7.0, every launcher in the bin directory hashed
  identical to its copy in the environment, and an upgrade through this program reported the tool
  converged, left the running process in place, and `uv tool list --outdated` then listed nothing.

## Considered options

- **Displace the launchers, as ADR 0022 does for cargo**, either ahead of the upgrade or on its
  refusal. Rejected on the measurement above: the running launcher cannot be moved, so displacing
  ahead fails where it matters, and a retry after displacing leaves the name empty.

- **A dry-run install into the tool's own environment.** The most exact, since it resolves the
  whole environment as an upgrade would. Rejected for cost, a launch per tool at over four times
  the whole-set read, and for needing the environment's path, which is uv's layout rather than
  anything it reports.
- **`uv pip compile` against standard input.** Resolves against the default index and whatever
  interpreter uv finds rather than the tool's own, and needs a standard-input capability the read
  invocations do not have.
- **PyPI's JSON API.** Reports the newest release, not the newest that resolves: a release whose
  `requires-python` excludes the tool's interpreter reads as drift that no upgrade closes, and a
  tool installed from another index is read against the wrong one.

## Consequences

- **A tool installed with an exact pin stays drifted.** `uv tool list --outdated` reports 6.1 for
  a tool pinned to 5.0 while `uv tool upgrade` answers "Nothing to upgrade" and exits 0, so an
  apply reports the resource as not having taken. Only a hand install carries a pin, since this
  program installs by name alone.
- **A tool whose newest version uv cannot look up for a reason other than the network is read as
  converged**, because the listing leaves it out rather than failing. Nothing this program
  installs is sourced from anywhere but an index.
- **The interpreter is declared, not derived.** uv does not choose one from a package's
  `requires-python`: serena-agent 0.1.4, declaring `<3.12,>=3.11`, installed onto CPython 3.14.2
  when that was the default (measured on the same date). A tool may therefore name the interpreter
  its environment is first built with, and an upgrade keeps the one the environment has.
- **A launcher that changes while it runs is held until it stops**, since the new copy cannot reach
  the name, and it is reported in ADR 0022's category rather than as a fault.
