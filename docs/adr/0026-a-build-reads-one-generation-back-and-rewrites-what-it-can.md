# A build reads one generation back and rewrites what it can

Status: accepted (2026-08-10, grilling session on the settings a configuration owns)

A generation is the lowest build that can read a configuration, and a build has always been
described as reading anything at or below its own by migrating older shapes forward. Nothing
migrated anything. The description held because no generation had ever changed the shape of a
document incompatibly, so every older generation happened to deserialise as the current one. This
change is the first that does not: two fields leave the schema and a third is renamed.

A build therefore reads its own generation and the one below it, converting the older shape through
a struct frozen at that generation. Anything older is refused as too old, which is a third cause of
unreadability alongside malformed and too new, and the only one whose remedy is to run an
intervening build.

One generation deep, because the whole population is two repositories belonging to one person, and
a coordinated edit is available where a chain of transforms for a population of zero is not. The
migration is written back to its source when that source is a local directory, and announced
otherwise: a GitHub source cannot be written without committing to it, and this program reads the
configurations it converges against rather than authoring them. Without a writeback nothing ever
advances a document, so every transform would be permanent and the chain could only grow.

A writeback happens on apply and never on plan. Plan holds only the capabilities that read a
machine, which is what makes "plan changed nothing" a property the compiler checks rather than a
convention — but configurations are loaded before any machine exists, so a write placed there is the
one side effect that guarantee cannot see. A plan reports the pending migration and performs none of
it.

## Considered options

- **No migration; refuse the older generation outright.** Cheapest, correct for a population this
  size, and rejected because a mechanism deferred at the first opportunity to build it is a
  mechanism that never exists.
- **A chain reaching the first generation.** Every step below this one is an identity transform
  written for no document that exists.
- **Transforms over raw JSON rather than a struct per generation.** Keeps exactly one live
  configuration struct and stays cheap at the fifth bump, at the price of an untyped step whose only
  check is the deserialise that follows it.
- **Writing back on plan as well as apply.** Rewrites a tracked file in a checkout during a command
  documented as changing nothing, and does it in the one place the read-only capability split cannot
  reach.
- **Never writing back, announcing in both cases.** Leaves the document at its old generation
  forever, so the transform can never be deleted.

## Consequences

- **Unreadability has three causes and three closures.** Malformed is a fault in the repository and
  a person resolves it; too new is a fault in the build and the program resolves it by updating
  itself; too old is a document the reader has outgrown, and a person resolves it by running an
  intervening build once or by rewriting the document.
- **The oldest readable generation is derived from the build's own** rather than declared, so the
  two cannot drift apart.
- **The frozen struct reuses the live resource types**, since only the head of the document changed.
  That makes it frozen by convention rather than by construction: a later generation that changes a
  resource silently changes what this one claims a generation-4 document looked like. A checked-in
  document of that generation, migrated and compared against an expected value, is what makes that
  fail loudly instead.
- **A migration is reported in its own right, as neither a change nor a notice.** A change closes
  drift on a declared fact about a machine, and a configuration's generation is neither declared nor
  about the machine; a notice is a message about something the tool cannot do, which is true of a
  GitHub source and false of a local one it is about to rewrite.
- **The announcement for a GitHub source is the first notice this program raises rather than
  reads.** Every other notice is written by a configuration.
