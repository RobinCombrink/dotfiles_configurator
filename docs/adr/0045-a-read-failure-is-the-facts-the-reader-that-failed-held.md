---
status: accepted
---

# A read failure is the facts the reader that failed held

A read of the machine that fails reaches convergence as a value its reader declared, never as an
error chain. Each fallible `ReadMachine` method returns its reader's failure type, one hand-written
enum per body of reading code its methods share: the workspace reader, the Dart package reader,
the registry reader, the release reader, the crate-index reader and the clone reader. Each variant
names what the reader found. Two facts several readers meet are shared leaves rather than variants
repeated in each: a program that could not be started, and a file that could not be read, each
holding its path and the `std::io::Error`. A method whose only failure is a leaf returns the leaf,
and a reader that meets one embeds it.

A foreign library's error is held typed in an `Arc`, exposed through `std::error::Error::source`,
and rendered by its own `Display` only inside the variant's. A `String` field holds only text a
foreign program emitted, and a child that ran and exited non-zero is held as `FailedExit`. Where a
configuration name type refuses text it was handed, the variant holds the text it refused.
`ReadFailure` is an exhaustive enum with one variant per reader and leaf, `UnreadableReason`
holds it, and a holder only one reader can feed holds that reader's type directly. No read-failure
type converts from `anyhow::Error` or from a string, and none implements `PartialEq`, so a test
matches the variant and its own facts rather than comparing a foreign error.

The compiler holds the shape: a reader's `Result` makes an undeclared failure unrepresentable, a
single-reader holder cannot hold another reader's failure, and with no conversion from a string no
convergence site can receive prose. That a `String` field holds only foreign program output sits
on discipline.

This supersedes the read-failure exception of
[ADR 0044](0044-a-reason-carries-facts-and-is-rendered-only-where-it-is-shown.md). The write side,
`WriteMachine`'s errors, stays `anyhow`.

## Considered options

- **One shared vocabulary by shape.** A single enum of the shapes failures take — a file, a
  program, a request — would serve every reader, but the facts each reader holds differ beyond
  those shapes, and the remainder would need a free-text variant to carry them, which is the chain
  again under another name.
- **The chain kept as the reader's report.** Leaves the reader's own sentences in the value and
  leaves every test unable to tell one failure from another, which is the state this replaces.
- **One enum per method.** Splits a reader's shared reading code across types that hold the same
  facts, so a helper two methods call needs a conversion for each.
- **One crate-wide enum.** Lets a holder fed by one reader hold every reader's failure, so the
  type no longer says which reader could have failed.
- **A hand-written equality that ignores the foreign source.** Keeps `assert_eq!` working by
  making two failures with different causes compare equal, which is a test passing on a value it
  did not check.
- **A boxed transport error.** Erases which library failed, so a test cannot reach the cause and
  a reader cannot tell a refusal it should report from one it should treat as absence.

## Consequences

- **A rendered read failure may read differently where the old text was produced by joining.**
  Every resource name, path, program, repository and remedy a line showed it still shows.
- **A clone checked out at a commit rather than a branch is reported as such**, naming the clone,
  rather than as a missing branch named `HEAD`.
- **A type holding a read failure cannot be compared whole**, so a test comparing two change sets
  compares their debug renderings.
- **A new way for a reader to fail is a new variant**, with its rendering beside it and its tests
  on its facts.
