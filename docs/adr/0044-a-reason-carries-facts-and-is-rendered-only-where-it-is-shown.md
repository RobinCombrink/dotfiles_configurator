---
status: accepted
---

# A reason carries facts and is rendered only where it is shown

Every reason an assessment reports about a resource — the drift reason for a change, the impediment
that blocks it, the finding a converged resource carries — is an enum of the facts the site that
raised it held: the paths, versions and names involved, and the text a foreign program emitted.
None converts from a string. Text is composed only by the `Display` implemented beside each type, as
it already is for `Notice`, `NamedAssets` and `Staleness`, and `reporting` goes on taking
`impl Display`. A test at any layer, a Gherkin step included, asserts the variant and its facts, so
what a reason says is checked without pinning how it reads.

The compiler holds the rule for the reason types themselves: with no conversion from a string, no
convergence site can hand one prose. That a remaining `String` field holds only foreign text — a
child's standard error, a version cell from a listing, a line a tool printed — sits on discipline.

## The one admitted exception

Every `ReadMachine` method returns `anyhow::Result`, so a machine read that fails reaches convergence
only as an error chain, and convergence can hold it only as text. That text is held by `ReadFailure`,
whose single constructor takes `&anyhow::Error` and keeps the whole chain. It appears in
`UnreadableReason` as the one variant `ReadFailed`, and in a workspace binary's staleness where the
commit it reports or its own `--version` could not be read. No convergence site composes prose
around it.
The chain still carries this program's own sentences, and no test can tell one read failure from
another. What a read failure is — a fact enum per reader, one shared vocabulary, or the chain kept as
the reader's report — retypes the `ReadMachine` seam, and is owned by the dotfiles issue
[a machine read's failure crosses into convergence as a rendered
chain](https://github.com/RobinCombrink/dotfiles/issues/1375), as ADR 0024 leaves widening a failure
vocabulary to the work that owns it.

## Considered options

- **`Display` in `reporting`, one rendering per reading type.** The facts would be the same, but the
  words for a drift would live in a module that does not define it, so a new variant needs an edit
  in two modules and the type a reader opens shows no rendering at all. Rendering beside the type is
  the shape every other rendered value in this crate already has.
- **String newtypes behind named constructors.** Each site would call a constructor naming what it
  found, and the constructor would compose the sentence. A test could still reach the reason only
  through its text, so every assertion on a reason would pin wording.

## Consequences

- **A rendered reason may read differently where the old text was produced by joining**: the read
  failures that were prefixed by their site now render as the chain alone, a failed `--version`
  request of a workspace binary names the argument rather than the binary, which the line already
  names, and a version cargo lists that cannot be read is shown without the parser's account of
  why. Every resource name, version and path a reported line showed it still shows.
- **A new kind of drift is a new variant**, with its rendering beside it and its tests on its facts.
