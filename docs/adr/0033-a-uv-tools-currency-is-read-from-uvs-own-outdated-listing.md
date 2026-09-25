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

## Considered options

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
- **An upgrade fails while the tool is running.** uv upgrades the environment and then cannot copy
  the executable over the running one, exiting 1 (measured on the same date). The environment
  already holds the new version, so the next run reads it as converged.
