# Convergence is additive

Status: accepted (2026-07-31, grilling session on the Phase D domain model)

Convergence makes declared things true. It never makes undeclared things false. Withdrawing a
declaration ends the tool's interest in that resource; it does not undo it. Removing something
from a machine is a human action.

This follows from ADR 0001 rather than adding to it. Knowing that a symlink was created by an
earlier run, and is now undeclared, requires a record of what past runs did — the receipt that
ADR 0001 rejected because it can lie and because it becomes a file that must not be lost. With
the machine as the only source of truth, an undeclared resource and a resource that was never
declared are indistinguishable, which is the honest consequence of that choice.

The trap is that removal is *partially* achievable without a receipt, and partial is worse than
none. A symlink whose target lies inside the dotfiles repository is self-evidently this tool's
work, so orphaned links could be found and removed. Nothing equivalent exists for a package: an
inventory of installed packages cannot distinguish one this tool installed from one installed by
hand years earlier. Shipping removal for the kind where it happens to be discoverable teaches a
mental model that silently fails everywhere else — someone who deletes a declaration and watches
a symlink disappear will reasonably conclude the same thing happened to their applications.

## Considered options

- **Full removal via a state file**: what most convergence tools do. Rejected with ADR 0001, and
  for its reasons rather than new ones.
- **Removal only for kinds that can prove ownership from the machine**: sound where it applies,
  and the reason this decision is reversible later. Rejected now because one kind out of seven
  is not a guarantee, and a guarantee that holds sometimes is more dangerous than none.
- **Treating an undeclared resource as an error** so a person is at least told: rejected because
  the tool cannot tell that a resource is undeclared, which is the whole problem.

## Consequences

- A configuration is a set of claims about what must be true, not a complete description of the
  machine. That distinction belongs in the glossary and it does, under *converged*.
- What would unlock removal later is not a receipt but **readable ownership** — a resource whose
  provenance the machine itself reveals. That could be introduced per kind, without ever writing
  state, if the need arises.
- A machine accumulates whatever it was ever told to have. That is a real cost, accepted because
  the alternative is a record that drifts per machine and is trusted more than the machine.
