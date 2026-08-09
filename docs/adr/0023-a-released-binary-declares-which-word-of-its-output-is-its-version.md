# A released binary declares which word of its output is its version

Status: accepted (2026-08-09, implementation session on the released-binary shape). Refines ADR
0016.

A released binary declares which whitespace-separated word of the output its version arguments
produce holds the version. The word is required and has no default, and the word selected must
still convert to a version, so an index pointing at anything else is refused rather than read as
one.

ADR 0016 has desired state and actual state "both parse into one version value whose conversion
accepts an optional leading `v`", which holds for a release tag and describes nothing a binary
prints. Measured 2026-08-09: `gh` prints `gh version 2.80.0 (2025-09-23)`, `ripgrep` prints
`ripgrep 15.1.0`, `git` prints `git version 2.52.0.windows.1`, and this program prints
`dotfiles_configurator 0.0.0-dev`. None is a bare version, and the version sits in a different
word in three of the four. A comparison against a tag therefore needs something to say which part
of the line is the version, and 0016 left that unsaid.

Declaring it keeps the program out of the business of guessing. What a binary prints is a fact
about that binary, known to whoever declared it and to nobody else; a rule inferring it from the
text is right until it meets output shaped like `Foo 2 CLI, version 1.4.0`, and when it is wrong
it is wrong invisibly, with no way for a configuration to correct it. The arguments are already
declared for exactly this reason, and the word is the same kind of fact as the arguments that
produce it.

The declaration is checked rather than trusted. A word index is the sort of thing that is written
down once from memory and never re-read, so the selected word is converted to a version and a
failure names both the word and the invocation that produced it — an index pointing at `version`
or at `(2025-09-23)` makes the resource report that it cannot be read, not that it has drifted to
a version nobody released.

## Considered options

- **Scan for the first version-shaped word**, taking the leftmost word that starts with a digit
  once an optional leading `v` is dropped. It is correct on all four binaries measured and asks a
  configuration for nothing. Rejected because it answers a question about a specific binary by
  guessing, and cannot be overridden where the guess is wrong.
- **Require the whole trimmed output to be a version.** The strictest reading of 0016, and the one
  its wording implies. Rejected on measurement: it converges no binary that exists, including this
  one, which is fatal to a shape 0016 states is meant to serve more than this program.
- **Test whether the output contains the desired version**, which needs no extraction at all.
  Rejected because actual state would then only be testable against desired state rather than
  readable on its own, so drift could not name the version installed; and because containment
  false-positives, with `1.1` found inside `1.10.2`.

## Consequences

- **Every released binary declares a word**, including one printing a bare version, which declares
  the first.
- **A version word is a per-resource setting rather than a policy**, so the configurable surface
  grows by one field on the shape, and the field cannot be derived from anything else the
  configuration holds.
- **An index that is wrong reports the resource as unreadable on every run**, naming the word it
  read, and never installs over a binary that was already current.
