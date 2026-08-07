# One name for the tool and everything it owns

Status: accepted (2026-08-07, design session on self-provisioning)

The crate, the binary, the release asset and the directory the tool owns on a machine all spell
`dotfiles_configurator`. An invocation reads `dotfiles_configurator plan --context personal`, and
the tool's own files live under `~/.dotfiles_configurator`, which holds `bin` and `logs`.

The binary was `dotfiles`, which is the name of a different thing. `dotfiles_repository` is a
field of every configuration and **Dotfiles repository** is a term in the glossary, so a program
called `dotfiles` and a directory called `~/.dotfiles` both name the repository this tool reads
rather than the tool that reads it. One of the two has to move, and the repository is named by
every configuration while the binary is named by nothing yet: measured 2026-08-07, neither
`dotfiles` nor `dotfiles_configurator` resolves on this machine's search path, so no installed
machine depends on the old spelling.

The tool owning a directory follows from having somewhere to put a released binary. A destination
must be on the search path without presupposing anything the tool itself installs, which rules out
the directory Cargo installs into, and it must not depend on another installer having established
it. One root the tool owns satisfies both, and `logs` joins `bin` inside it rather than staying
where a platform data directory put it, so that where this tool keeps its things has one answer.

## Considered options

- **Rename only the directory, keeping `dotfiles` as the binary.** Rejected because the collision
  is in the name rather than in any one use of it: a program named after the repository it reads
  misdescribes itself wherever it appears, including in the bootstrap instructions where a person
  meets it first.
- **Rename everything and keep a short `[[bin]]` alias.** Rejected because two spellings of one
  program is the duplication this decision exists to end, and the longer one is typed at a
  bootstrap and rarely after.
- **Install released binaries into `~/.local/bin`.** Rejected because that directory's membership
  of the search path is owed to whichever installer established it — on this machine, the tools
  that put `uv` and `claude` there — which is a dependency on something undeclared rather than a
  convention this tool can rely on.

## Consequences

- **The published release asset changes name.** Nothing consumes the old one: no configuration
  declares it, and no machine has it installed.
- **The invocation is longer**, at every use, in exchange for a name that says what the program is.
- **Runs already written under the platform data directory are orphaned.** The retention limit
  prunes within the directory being written to, so logs left at the old path are pruned by nothing
  and are removed by hand or not at all.
