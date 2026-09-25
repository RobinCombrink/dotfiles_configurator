---
status: accepted
---

# A configuration declares its estate, and the machine manifest groups owners by it

Tools outside this program need to know which repository owners are this machine's, and which
estate each one belongs to: a sweep reaches only the checkouts of owners the machine declares, and
an estate's profile is resolved by name. The owner is not the estate. Work repositories are owned
by one organisation, the work dotfiles by a second account, and personal repositories by a third,
so the grouping has to be declared rather than inferred from any one owner.

A configuration may declare an estate: a name, and optionally owners besides its own. Its owners
are the account the configuration acts as, the owners of the workspaces it builds, and those it
names; the owner of a repository it only clones is never one, since a clone of a third party's
repository is not work of the machine's. A configuration declaring no estate contributes no owner.
The machine manifest ADR 0030 introduced carries the resolved result as `estates`, a map from each
estate's name to its owners, written from every applied configuration. The flat set of the
machine's owners is the union of those groups, derived by its reader and never stored.

Each estate is declared by exactly one configuration, and an owner belongs to exactly one estate.
Either collision is refused when the configurations are loaded, before anything is read from the
machine, because a precedence rule between two documents would make the answer depend on which one
happened to win. Owners compare without regard to case, as GitHub compares them, and keep the
spelling they were declared with. An estate's name is also the name of its directory, so it is
letters, digits, `-` and `_` alone, and never a name Windows reserves for a device.

The generation moves, per ADR 0028: a document can name the new field.

## Considered options

- **A flat `owners` list on each configuration**, grouped later. Rejected because the manifest
  would then change shape twice, and its readers with it.
- **Only the owners an estate names**, with no implicit ones. Rejected because the account a
  configuration acts as and the owners of the workspaces it builds are its own by construction, and
  restating them in every document is a second copy that can disagree with the first.
- **Merging two declarations of one estate.** Rejected for the same reason as a precedence rule:
  which document holds an estate would stop being answerable from either document alone.

## Consequences

- **A manifest written before this change carries no estates**, and its readers refuse it and name
  an apply as the fix; this program always writes `estates`, empty when nothing declares one.
- **Two estates on one machine need two configurations that apply there**, since a configuration
  declares at most one.
