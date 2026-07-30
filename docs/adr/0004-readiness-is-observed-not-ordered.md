# Readiness is observed, not ordered

Status: accepted (2026-07-31, grilling session on the Phase D domain model)

A resource kind states which tools it needs, and those needs are a property of the kind rather
than something an author writes down — a package installed by Cargo needs Cargo, and no
configuration can express one that forgets. Whether those tools are present is **read from the
machine** before a resource is assessed or converged. A resource whose tools are absent is
neither converged nor failed; it is unassessable, and says so.

Apply therefore repeats: converge what is ready, read again, converge what has since become
ready, and stop when a pass changes nothing. Termination is structural rather than a limit —
the set of unconverged resources strictly shrinks on every productive pass. Plan does not
repeat, because it changes nothing and so can learn nothing by looking twice.

Ordering survives for a different reason. Kinds are converged in a fixed order — repository,
application, package, symlink, registration, command — and that order is **load-bearing for
safety, not presentation**: a program initialising its configuration for the first time will
write through a symlink into the dotfiles repository, so applications must be installed before
anything links into their configuration directories. Readiness cannot discover that constraint,
because nothing observable about a machine reveals that acting later would destroy something.

## Considered options

- **Fixed kind order alone.** It satisfies every dependency in the current configuration, but
  only by luck of which kinds those dependencies happen to span. Installing Rust the ordinary
  way on Windows — `winget install Rustlang.Rustup` — makes the dependency package-to-package,
  which a kind-level order cannot express, and that is one configuration edit away.
- **Declared dependencies between resources.** Expresses anything, at the cost of a graph, cycle
  detection, and a coupling every author has to maintain correctly. Rejected as more machinery
  than the evidence justifies; it remains available if a dependency ever appears that readiness
  genuinely cannot see.
- **Declaring what provides a tool**, so a configuration naming cargo packages without naming
  Rust could be rejected before touching the machine. Rejected because it contradicts the
  machine being the source of truth: cargo is installed on the author's machine already, and a
  provider check would refuse work the machine is perfectly able to do on the strength of a
  missing declaration.

## Consequences

- Plan is not a complete prediction on an unconverged machine. It reports what it cannot yet
  assess instead of guessing, which is a weaker promise than "plan shows exactly what apply will
  do" and a more truthful one.
- A run that ends with resources still unassessable exits non-zero and names them. The machine
  is not converged and the tool should not imply otherwise.
- Ordering being a safety property rather than a convenience means reordering kinds is a
  behavioural change to what happens to a machine, not a cosmetic one.
- The safety argument for application-before-symlink holds only while applications are reliably
  assessable in the first pass. They are today, because their presence checks need nothing that
  a fresh machine lacks. A kind whose assessability became conditional would weaken it.
