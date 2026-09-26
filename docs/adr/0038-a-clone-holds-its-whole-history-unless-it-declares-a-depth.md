---
status: accepted
---

# A clone holds its whole history unless it declares a depth

A repository resource declares how much history its clone holds. Absent a depth, the clone holds all
of it; a declared depth is a count of commits back from the tip. A clone declaring no depth that is
already shallow has drift, and apply closes it by deepening the clone to its whole history.

Every clone this program made was shallow, at a depth of one, and nothing recorded that choice. The
shallowness buys the program nothing: it reads trees only at the upstream tip (ADR 0007), which a
clone at any depth holds, so the whole cost of a shallow clone falls on the person working in it.
`../flutter` held one commit, and a clone of the configurator itself held nine
and was deepened by hand on 2026-09-07.

Depth is a setting because it genuinely varies by repository rather than by policy. A repository a
person authors in needs its history; a vendored SDK does not, and `flutter/flutter` beside
`wow_ui_backup` is that difference among the repositories declared today. The default is the whole
history because it is the one that costs the person nothing.

Shallowness joins the converged test because changing the depth alone reaches no machine that has
already cloned: a clone was converged whenever its directory held a `.git`, so apply would neither
clone again nor deepen. Deepening is additive, per ADR 0005: it adds history and removes nothing
from the clone.

## Considered options

- **Keeping every clone shallow.** Cheapest to fetch, and rejected because the program gains
  nothing from it and the person working in the clone pays for all of it.
- **Changing the depth without changing the converged test.** Correct for every clone made from then
  on, and reaches none of the clones that already exist.

## Consequences

- **The generation moves**, per ADR 0028: a document can name the new field.
- **A clone declaring a depth is converged whatever its depth.** Deepening is the only direction
  apply moves a clone, so a declared depth governs the clone as it is first made and never makes an
  existing clone shallower.
- **Whether a clone is shallow is read from the machine**, like every other actual state, and a
  clone whose history cannot be read is unassessable rather than drifted.
