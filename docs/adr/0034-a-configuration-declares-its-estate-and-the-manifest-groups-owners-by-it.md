---
status: accepted
---

# A configuration declares its estate, and the machine manifest groups owners by it

Tools outside this program need to know which repository owners are this machine's, which estate
each one belongs to, and which account answers for it: a sweep reaches only the checkouts of owners
the machine declares, an estate's profile is resolved by name, and a request about an owner's
repository acts as its estate's account. The owner is not the estate. Work repositories are owned
by one organisation, the work dotfiles by a second account, and personal repositories by a third,
so the grouping has to be declared rather than inferred from any one owner.

A configuration may declare an estate: a name, and the folder of its dotfiles repository holding
the estate's `estate.json`, the profile the estate's own tools read. The estate's owners are the
keys of that file's top-level `owners` object and nothing else, so the file is their one home. This
program reads it as a foreign source, decoding those keys as owners and ignoring every other field
and value, and refuses the apply, naming the file, where it is absent or unreadable, holds no
`owners` object, or keys something GitHub would not name. It is read when the configurations are
loaded, before anything is read from the machine, and from the same revision as the configuration:
a local source reads it from its checkout, and a GitHub source through the contents API from the
source's repository, with the folder taken from that repository's root. The account the
configuration acts as and the owners of the workspaces it builds are owners of its estate, so an
apply is refused, naming the configuration, the owner and the file, where the file does not key
one of them. The owner of a repository a configuration only clones is implied by nothing, since a
clone of a third party's repository is not work of the machine's. A configuration declaring no
estate contributes no owner and is not checked. An estate acts as the account its configuration
acts as. Its folder is linked at `.dotconfig/estates/<name>`, resolved against the configuration
source's files root and converged as a symlink is. The machine manifest ADR 0030 introduced carries
the resolved result as `estates`, a map from each estate's name to its owners and its account,
written from every applied configuration. The flat set of the machine's owners is the union of
those groups, derived by its reader and never stored.

Each estate is declared by exactly one configuration, and an owner belongs to exactly one estate.
Either collision is refused when the configurations are loaded, before anything is read from the
machine, because a precedence rule between two documents would make the answer depend on which one
happened to win. Owners compare without regard to case, as GitHub compares them, and keep the
spelling they were declared with. An estate's name is also the name of its directory, so it is
letters, digits, `-` and `_` alone, and never a name Windows reserves for a device.

The generation moves, per ADR 0028: a document can name the new field. The migration to generation
12 takes an estate's folder from the symlink resource linking `.dotconfig/estates/<name>`, removes
that resource, and drops the owners a generation 11 estate named once the folder's `estate.json`
keys every one of them. It refuses an estate with no such link, and one naming an owner the file
does not key.

## Considered options

- **A flat `owners` list on each configuration**, grouped later. Rejected because the manifest
  would then change shape twice, and its readers with it.
- **Owners derived from the account and the workspaces, with any others named in the
  configuration.** Rejected because the estate's `estate.json` already keys every owner for the
  tools reading it, so a list in the configuration is a second copy that can disagree with it.
- **Adding the account and the workspaces' owners to the file's keys** rather than refusing a
  configuration whose file lacks one. Rejected because the manifest would then place an owner in an
  estate whose own profile does not name it.
- **Reading a GitHub source's `estate.json` from its local clone.** Rejected because a first apply
  has no clone to read, and this program never pulls one, so a clone's file can lag the
  configuration read beside it without bound.
- **Merging two declarations of one estate.** Rejected for the same reason as a precedence rule:
  which document holds an estate would stop being answerable from either document alone.

## Consequences

- **A manifest written before this change carries no estates**, and its readers refuse it and name
  an apply as the fix; this program always writes `estates`, empty when nothing declares one.
- **An estate written as a bare list of owners, naming no account, is refused** when the manifest
  is read, rather than read as an estate that acts as nobody.
- **Two estates on one machine need two configurations that apply there**, since a configuration
  declares at most one.
- **A configuration declaring an estate loads only where its `estate.json` can be read**, from the
  checkout or from GitHub, so a schema change to that file elsewhere cannot break a load unless it
  moves or retypes the `owners` object itself.
