---
status: accepted
---

# A path check holds candidates, and the report names the one that answered

An installer declares where its application would be found if it were installed. A Windows
application installs machine-wide or per-user, and the two locations differ by more than their
root, so one declared path forces the author to choose one and be wrong about the other. The path
declared for Docker Desktop was rewritten twice, each time from one location that is true
somewhere to another, in a document that applies to every machine.

So the path form holds a non-empty set of candidates and is satisfied by any of them. A location
is still declared rather than derived, but a set of locations is a fact about the application,
which is what a configuration applying everywhere can hold, where a single location is a fact
about one disk.

A converged path check names the candidate that answered. A single wrong path fails loudly, which
is how a machine reports that nothing was found; one wrong candidate among several never fires,
and nothing says so. Naming the candidate that answered is what keeps a set of locations from
accumulating ones no machine has ever satisfied.

The field is renamed rather than joined by a second form, because one concept keeps one spelling.
A check form admitting a list would put two ways to write a single path into the schema, and a
field accepting either a string or a list would put two ways into the field.

## Considered options

- **Deriving presence from the machine's installed-application inventory**, which lists
  applications it did not itself install and would remove the declaration entirely. It replaces a
  declared path with a declared identity, and that identity is not reliably better: `winget list`
  shows the inventory's name agreeing with the name a configuration gives an application for some
  and not others, and shows applications identified only by a product code. The derivation is also
  a larger change than the failure asks for.
- **Contradicting a declared check against that inventory** rather than replacing it, reporting
  the disagreement where a path is missing and an application of that name is installed. Cheaper,
  and it leaves the configuration still holding a fact about one machine's disk.
- **A check form that composes other forms**, satisfied when any of them is. More general, and it
  buys generality across forms that nothing needs while making a single path writable two ways.
- **A field accepting either a string or a list**, which keeps every existing document readable
  and costs no coordinated edit. Rejected for the second spelling it would leave in the schema
  permanently, to save an edit of two documents.

## Consequences

- **The generation moves**, per ADR 0028: a document can name the new field.
- **A document at the previous generation naming a path check is migrated, not refused.** The
  reader frozen at that generation keeps the singular `path` and rewrites each one into a
  one-candidate `paths`, so a document written before the rename reads under this build without a
  person editing it.
- **The fixture pair carries the rename.** The previous generation's fixture holds `path` and the
  current one `paths`, so the rewrite is exercised against what the previous generation actually
  wrote. The pair holds every resource kind, so the next rename of a resource field fails loudly
  rather than slipping past a pair that happened not to hold it.
- **A check with no candidates cannot be written**, the set being non-empty by construction.
- **The plurality is on the path form alone.** A command on the path and a command's output are
  single answers and stay that way.
