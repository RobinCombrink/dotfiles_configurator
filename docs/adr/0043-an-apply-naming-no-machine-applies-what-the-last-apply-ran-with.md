---
status: accepted
---

# An apply naming no machine applies what the last apply ran with

An `apply` resolves its machine class and configuration sources exactly as a `plan` does under ADR
0039, through the one resolver both share: named flags override the record in the machine manifest,
and with neither named the run is the one the last apply recorded. A machine whose manifest records
no run refuses a bare apply, naming the manifest, before anything on it is read or changed.

This supersedes the dotfiles repository's [ADR
0045](https://github.com/RobinCombrink/dotfiles/blob/2a2a8b087f6d9e7cebbae3e6026ce77e5f6df052/docs/adr/0045-the-configurations-a-machine-converges-are-named-in-the-orchestrator.md),
which placed each machine's set of configurations in an uncommitted lefthook overlay. None of that
overlay was built, and the value it would have held already has a home: the manifest's `class` and
`configuration_sources`, which every apply writes. A second machine-local copy in the orchestrator
would be one value settable in two places.

## Considered options

- **ADR 0045's lefthook overlay.** Rejected: it duplicates what the manifest records, and it rests on
  lefthook composing an overlay with a job's arguments, which was never verified.
- **A `machine.json` of its own under `~/.dotfiles_configurator/`.** Rejected for the reason ADR 0039
  rejected a record beside the manifest: a second document written and read by the same runs.

## Consequences

- **The first apply on a machine names its class**, with `--machine` and any sources it needs; every
  later apply, and every hook calling one, may name nothing.
- **A self-replacing apply hands its successor the arguments it was given**, which resolve against
  the same manifest to the same run.
