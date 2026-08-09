# An account is this program's word, and crosses to a credential source as a string

Status: accepted (2026-08-09, design session on the credential crate's surface)

A GitHub account is the account a configuration acts as, which makes it part of what a
configuration says rather than part of how a credential is fetched. It is declared in a
configuration file, so it is deserialised and appears in the generated schema, and it is
distinguished from a repository's owner, which is an address. Every one of those properties
belongs to this program. A credential source needs none of them: it needs a value to hand to a
subprocess.

So the account is declared here, alongside the other domain identifiers, and reaches a credential
source as a string. A name is a string only at the wire boundary, and a subprocess argument is
that boundary.

## Considered options

- **The credential source owns the type.** Deserialisation and schema generation would have to
  follow it into a crate that reads no configuration, so that a schema this program publishes
  would derive from a type it does not own. The account is also the narrower of two concepts the
  glossary already separates, and the separation is meaningless where there are no repositories to
  address.
- **Each owns one.** Two types under one name, both executable, drifting independently.

## Consequences

- **A credential source's typed refusal is narrowed to a remedy by a function of the refusal
  alone**, so the words a person reads are exercised without a machine that lacks the tool. The
  remedies differ by the act they ask for — install the tool, or log in as the account — and a
  scenario asserting either would need a machine in a state neither this one nor the build agent
  can be put into, which is a test that could only ever be skipped.
- **A configuration source that cannot be read still reports a fault in the repository it was read
  from**, even when the cause is an account the machine does not hold. The cause is typed by the
  credential source and erased where the configuration source is read; a cause that fits an
  environment failure is a change to the vocabulary of unreadability rather than to this path, and
  is left to the work that owns that vocabulary.
- **Two sibling programs keep obtaining credentials the discarded way.** They are not consumers of
  this decision, are not updated, and are not pinned; the change reaches them as a compile error
  if and when either updates.
