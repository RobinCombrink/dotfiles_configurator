# The machine manifest is written by the run that knows the class

Status: accepted (2026-08-11, design session on a manifest nothing wrote)

Tools outside this program read `~/.dotconfig/machine.json` for where this machine keeps its
repositories. No program wrote that file. Each reader treats its absence as an optional reading
rather than a fault, so the machine degraded quietly and separately in each of them — one narrowing
its scope in silence, another refusing outright — and a machine had no way to reach the state where
any of them worked.

The value is one this program already holds: a per-platform constant joined with a leaf, which is
how a resource's location is derived rather than declared (ADR 0025). The configurator therefore
writes the manifest, carried in every change set as a resource no configuration declares — the shape
ADR 0019 established for the program's own currency and ADR 0017 for the directory it installs
binaries into. The variant cannot be deserialised and is absent from the generated schema, so no
document can name it.

**The manifest names a fact about the machine, and the repositories directory is a fact about a
configuration.** The two share a formula and differ in subject. A run reads one configuration for
every machine and one for the class it names, and those resolve to different leaves on a work
machine: a configuration for every machine clones under the personal leaf wherever it is applied,
because it is written in a personally-owned repository. So a work machine has two repositories
directories, and only one of them is the machine's. The manifest names the one belonging to the
class the invocation names, and never a configuration's own.

Deriving it per configuration would compile, apply cleanly, and be wrong only on work machines —
where the readers accept the value and resolve against a tree that exists, so nothing reports a
fault.

The generation does not move. It moves for a kind, shape or field a document can name (ADR 0028),
and this is a change no document can express.

## Considered options

- **A registration declared in the configuration for every machine.** The kind already means a
  record this machine holds in another tool's configuration, and the plumbing exists. Rejected
  because the value is derived and a declaration could only restate what the run already knows,
  against ADR 0025 — and a declared leaf is one a document could get wrong, which is the failure
  above made expressible rather than impossible.
- **Declaring the personal leaf on every machine**, which needs no new derivation and is what a
  configuration for every machine already resolves to. Rejected because it answers a different
  question from the one the field asks. It is indistinguishable from the decision above for a reader
  that only wants the directory one level up, and opposite for one that wants to know which of the
  two trees is this machine's.
- **Storing the directory one level up instead of a leaf.** The readers that exist today all take
  the parent, so the leaf is stripped immediately. Rejected because it discards which tree is the
  machine's, which is the half a reader distinguishing work from personal needs, and the readers
  that strip it lose nothing by stripping it.
- **A new resource kind for machine-local generated documents.** Rejected: it moves the generation
  for a document shape nobody can write, and buys nothing over the kind that already covers records
  held for other tools. Worth revisiting if a second such document is ever wanted.
- **A person writing the file on each machine.** Rejected: nothing converges it, a machine that
  loses it has no way to notice, and a new machine starts without it.

## Consequences

- **A bare machine has a manifest after its first apply**, before anything else is authored, so the
  tools reading it work from the start rather than after a step nobody documented.
- **A manifest that is absent or names the wrong directory is drift**, reported by plan and
  rewritten by apply. Convergence stays additive (ADR 0005): withdrawing the resource would end this
  program's interest in the file rather than delete it.
- **The class of a machine becomes observable on disk**, where before it was only an argument to a
  run.
- **The readers keep treating the manifest as optional.** A machine that has not applied since this
  build degrades exactly as it did before, so nothing outside this program has to be sequenced with
  it.
- **The path and field name are a contract with repositories this program does not own**, held in
  two places at once and checked by neither compiler. A reader renaming the field reads a manifest
  that parses and says nothing.
