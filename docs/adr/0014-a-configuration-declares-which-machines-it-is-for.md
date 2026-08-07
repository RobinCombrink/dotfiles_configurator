# A configuration declares which machines it is for

Status: accepted (2026-08-07). Supersedes ADR 0012.

A configuration declares the machines it applies to, and an invocation names the machine it is
running on. A configuration applies when it declares `everywhere`, or declares the machine the
invocation named. Where configurations are read from is a separate argument that reads a
directory whole and cannot change which of them apply.

ADR 0012 separated the two by having an invocation name each configuration it wanted. That
removed the accident but put the composition on the command line: every caller repeated the set,
and adding a configuration meant editing the tool, its default, and every hook that named the
old set. Moving the declaration into the file inverts it — the set is composed by dropping a
file into the directory, and the invocation says only what the machine is.

The named machine is one value rather than a set, so naming none and naming two that describe
different machines are both unconstructable. `everywhere` is one of the three a machine can be:
an invocation naming it applies only what is universal, which is what a machine belonging to
neither class needs.

## Considered options

- **An invocation names each configuration, as ADR 0012 decided.** Rejected on composition: the
  set of configurations is written down once in the dotfiles repository and again in every
  caller, and the two drift.
- **`applies_to` as a list of machines.** Rejected because `everywhere` then becomes a synonym
  for "listed them all", which has to be restated as machine classes are added — a state
  validated against rather than one that cannot be written.
- **A machine-local declaration of which configurations this machine applies.** Rejected as in
  ADR 0012: it puts a notion of machine class inside a tool whose model is that the inputs
  determine the context, and makes one value settable in two places.

## Consequences

- **A configuration that cannot be read refuses the run even when it is for another machine.**
  Applicability is declared inside the file, so it can only be honoured after parsing, and
  ADR 0009 refuses every configuration when any is unreadable. A malformed `work` configuration
  therefore stops a `personal` run. This is a real narrowing of what a broken file affects
  compared with naming exact paths, accepted because a configuration that does not parse is a
  fault in the repository the run reads, not in the run.
- **A source reads only `*.dotconfig.json`.** A directory holds other files, and reading them
  whole is what makes the suffix load-bearing rather than conventional.
- **The configuration format takes a breaking revision to "3".** `applies_to` is required, so a
  file written for "2" declares no machine and could only be guessed at. The version gate
  refuses it by name instead.
- **An invocation naming a machine that nothing applies to is refused**, rather than converging
  against an empty desired state. On a work machine reading a directory holding only personal
  configurations, the empty set and a misnamed machine are indistinguishable, and converging
  against nothing is the more expensive of the two to be wrong about.
- The context remains a value this program holds, so behaviour that should differ between a
  personal and a work machine has somewhere to attach. Nothing attaches to it yet.
