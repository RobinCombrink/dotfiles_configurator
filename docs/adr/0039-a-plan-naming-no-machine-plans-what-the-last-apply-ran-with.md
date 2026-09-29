---
status: accepted
---

# A plan naming no machine plans what the last apply ran with

The machine manifest records the machine class and the configuration sources the apply that wrote
it ran with. A `plan` naming no machine reads both from the manifest and plans exactly that. A
`plan` naming a machine keeps its meaning, with the default source where it names none, and a
source named beside no machine replaces the recorded ones while the recorded class still applies.
A machine whose manifest records no run refuses a plan naming no machine, naming the manifest it
read. It never assumes a class.

A caller that has not been told what this machine is could not plan it before: the class was an
argument of every run and observable afterwards only as the leaf of the repositories directory
(ADR 0030), a formula a reader would have had to invert, and the sources were observable nowhere.
The dotfiles repositories report drift after every push from a hook that knows neither, and on a
work machine the answer to both differs from the personal one. The run that knows them is the
apply, so the apply records them, in the document it already writes for programs it does not own.

Recording them in the manifest makes a plan run for another class or other sources report the
manifest as drifted, which is true: an apply with those arguments would rewrite it.

## Considered options

- **Reading the class back from the repositories directory's leaf.** Needs no new field. Rejected:
  it inverts a formula this program owns in every reader, and it cannot recover the sources.
- **Falling back to a default class when nothing is recorded.** Rejected: planning a work machine
  as a personal one produces a plausible change set that is wrong, which is worse than a refusal.
- **A record of its own beside the manifest.** Rejected: it is a second document a machine can
  hold without the other, written by the same run and read by the same readers.

## Consequences

- **A bare machine refuses a plan naming no machine until its first apply.** The refusal names the
  manifest and the arguments that plan without it.
- **The manifest's path and field names become a contract with one more reader**, a command line
  rather than a program, and the generation does not move, for the reason ADR 0030 gives.
