# An application is a category, and a released binary is one shape of it

Status: accepted (2026-08-07, design session on self-provisioning). Refines ADR 0008.

An application is a program on the machine, and how it arrives is a variant rather than a set of
fields. Two shapes exist. An **installer** is downloaded and run, and its actual state is read
through a presence check, which is what an application has always been. A **released binary** is
an executable inside a release archive, and its actual state is the version it reports.

ADR 0008 called the configurator an application at a time when the word meant installer-driven and
opaque, and it is neither: the release carries an archive rather than an installer, and the
machine can be asked the version outright. The refinement keeps 0008's decision — the configurator
arrives as the latest release of its own repository — and moves the word underneath it.

Making it a variant rather than a kind of its own follows from what an application is. Kind order
is load-bearing for safety, so a new kind buys a slot in that order and has to justify one; a
released binary needs no slot, because it is a program on the machine and is installed exactly
where applications already are. Making it a variant rather than fields follows from what the
current shape admits: a name, any source and any presence check pair freely, so a release-sourced
archive with a path check and a version-reporting binary with a check that can never see a version
are both writable. The variants tie the way something arrives to the way its state is read.

A released binary declares the repository, the pattern that picks its asset, the entry within the
archive, and the arguments that make it report its version, which default to `--version`. It
installs under the entry's own name, so what it installs as is derived rather than declared a
second time. Desired state is the version of the latest release; actual state is what the
installed binary reports. Both parse into one version value whose conversion accepts an optional
leading `v`, so a tag and a reported string compare without a normalising step at the comparison,
and drift is inequality rather than ordering — which is what **Drift** already means, a difference
and not a deficit.

The version arguments are declared rather than fixed by this program, so ADR 0006 stands
unamended: plan runs only what a configuration's author wrote down. Reading the version invokes
the binary at the path it was installed to rather than resolving it on the search path, so a
different copy earlier on the path cannot answer for it.

## Considered options

- **Widen the existing shape** to admit non-installer sources and a version-based read. Cheapest
  in schema, and rejected on the model: every other application is installer-driven and opaque, so
  widening loses both halves of what the word means to serve the one case that is neither.
- **A resource kind of its own.** Rejected because kind order is a safety property rather than
  presentation, and a released binary needs no position in it that an application does not already
  have.
- **A command, until instances accumulate.** The stated route for a new capability, and rejected
  because the only currency check available to a command tests output against a written-down
  version string — the copied derived value ADR 0007 exists to remove, one release behind from the
  moment it is written.
- **Read the version from the executable's file metadata**, which launches no process at all and
  would leave ADR 0006 stronger rather than merely intact. Rejected because it requires the build
  to stamp that metadata, is specific to one executable format, and cannot be assumed of a
  third-party release binary — which is fatal to a shape meant to serve more than this program.

## Consequences

- **Only zip archives are read.** Compressed tar is undecided and belongs with the decision to
  support another platform, where the assets that need it appear.
- **A locally built binary in the install directory is drift and is replaced.** The version the
  build stamps when nothing overrides it is a development placeholder, so a hand-built copy never
  equals a release.
- **A presence check stays required on an installer and is unavailable on a released binary.** The
  two shapes read their actual state by different means, and neither can borrow the other's.
- **Replacing a running executable renames it aside first**, as ADR 0008 records from measurement,
  and the upgrade takes effect on the following run.
