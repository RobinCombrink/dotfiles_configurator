# A new resource kind moves the generation

Status: accepted (2026-08-10, design session on making an environment variable a resource)

A generation is the lowest build that can read a configuration, stated by the configuration itself.
A resource kind is additive to the document shape — every document written before it deserialises
unchanged, and nothing needs migrating — so a kind can be added without moving the generation, and
that is the first instinct.

What it costs is the floor. An author declaring the new kind has no number that means "this needs
the build that knows it": every generation at or below the build's own is read, so the document
states the number every older document already states, and an older build reads it. That build meets
an unknown kind and refuses the document as malformed, whose closure is a person resolving a fault
in the repository — and the repository holds no fault. Stating the generation above makes the same
document too new, whose closure is the program updating itself, which is the remedy that works.

So the generation moves whenever a kind, shape or field is added that a document can name. It does
not move for a change no document can express.

## Considered options

- **Leaving the generation where it is**, since no existing document changes and the migration has
  nothing to do. Rejected because it leaves the floor unstateable and routes the failure to the one
  party that cannot act on it.
- **Moving the generation only once a document declares the new kind.** Ties a property of the build
  to the contents of repositories the build does not own, and leaves the first document to declare
  the kind unable to state a floor either.
- **Refusing an unknown kind as too new rather than malformed**, which would let the generation stay
  put for good. An unknown kind is indistinguishable from a misspelled one, so every typo would
  report that the program should update itself.

## Consequences

- **Most generations from here are a migration with nothing to migrate.** The document shape is
  unchanged, so the transform re-stamps the version and copies the rest, and the fixture pair
  guarding it differs in one field.
- **The frozen struct loses a little more of its freezing each time.** ADR 0026 records that it
  reuses the live resource types; a kind added afterwards widens what it claims that generation's
  documents could hold, so a document stating the older generation while naming the newer kind is
  read rather than refused. The checked-in fixture catches a change to a resource the fixture names,
  and nothing beyond it.
- **A configuration repository moves after the release, never with it.** A document stating the new
  generation is unreadable to every installed build until that build has updated itself, so the
  documents are rewritten once a build that reads them has been published.
