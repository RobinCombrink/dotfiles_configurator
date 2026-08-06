# A configuration is named, not located

Status: accepted (2026-08-06, grilling session on previewing, convergence triggers and the
push window)

An invocation names the configurations it wants — `everywhere`, `personal`, `work` — and names
separately where they are read from. The two were one string, and a local directory source read
every file it found, so choosing where configurations came from silently chose which ones loaded.
Measured 2026-08-03: a run whose source was a local directory holding both files loaded
`everywhere` and `personal` together, and had begun installing personal applications before it was
killed.

The named set is parsed into the context the invocation is for, which admits only combinations a
machine could be. `personal` and `work` together describe no machine, and neither does naming
nothing; both are unconstructable rather than validated against.

## Considered options

- **Narrow a local directory to the `*.dotconfig.json` suffix its own files follow.** The smallest
  change, and it makes the accident less likely without making it impossible: the directory that
  caused it holds both manifests and that is what the suffix matches.
- **Name paths explicitly at every invocation.** Puts an owner, a repository and a path into every
  hook and every caller, which is the copied derived value ADR 0007 exists to remove — the location
  of a named configuration follows from its name.
- **A machine-local declaration of which configurations this machine applies, read by this tool.**
  Rejected because it puts a notion of machine class inside a tool whose model is that the inputs
  determine the context, and because it makes one value settable in two places.

## Consequences

- **An invocation naming no configuration is refused**, rather than falling back to a compiled-in
  default. The default named `everywhere` and `personal`, which is the wrong set on a work machine
  and was reachable by omitting an argument.
- Pointing at a local checkout is safe for development again: it changes where configurations are
  read from and can no longer change which ones are read.
- The context is a value this program holds, so behaviour that should differ between a personal and
  a work machine has somewhere to attach. Nothing attaches to it yet.
- Where the configurations are read from is a property of that reading, not of the machine: a run
  naming only local sources needs no GitHub credentials, which the reading path does not yet honour.
