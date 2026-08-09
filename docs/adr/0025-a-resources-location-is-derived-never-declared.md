# A resource's location is derived, never declared

Status: accepted (2026-08-10, grilling session on the settings a configuration owns)

`MachineSettings` names a repositories directory and a dotfiles repository, and a configuration
declares both. Every real value of both turns out to be a hand-typed copy of something the run
already holds: the repositories directory repeats one root across every configuration on the
machine, and the dotfiles repository names the repository the configuration was written in, which
is the source it was read from. A copy of a value is executable in a way a copy of prose is not —
it can disagree, and nothing reads the disagreement.

So no location is declared. A clone's directory is a platform constant joined with a leaf derived
from the configuration's context, and a configuration's files come from the source it was read
from: a GitHub source resolves to that repository's clone, and a local source to the checkout
containing it, found by walking up to a `.git`. A local directory inside no checkout is refused
rather than resolved against a guessed parent.

The leaf is `Work` for a work configuration and `Personal` for a personal one and for
`everywhere`, which is the one part of this that is a constant rather than a derivation.
`everywhere` is written in a personally-owned repository, and commit identity on a machine holding
both kinds of work routes by repository directory — so cloning that repository under the work tree
would author every commit to a personal repository as a work identity.

## Considered options

- **A whole absolute path declared per configuration**, which is today's shape. The root repeats
  identically in every configuration, and a work configuration written in an employer's repository
  would declare the root of a disk that repository does not own.
- **A leaf declared per configuration, checked against its context.** Makes a configuration
  declaring `work` with a leaf of `Personal` expressible, so something has to reject it — the
  illegal state manufactured by making a derivable value settable.
- **A repositories root as an invocation argument.** The honest home for a fact about the machine,
  and rejected because the value has never varied. A setting for something that does not vary
  manufactures the state it then has to validate against.
- **`everywhere`'s leaf following whichever variant applied.** Survives a third context without a
  privileged constant, and clones a personally-owned repository under the work tree, which is the
  identity-routing failure above.
- **Symlinks resolving against a declared repository rather than the source.** Keeps a
  configuration self-contained, at the price of a declaration that is always equal to available
  evidence and is checked against none of it.

## Consequences

- **A symlink's target depends on the source the run named.** A checkout outside the canonical
  clone path links against itself, so alternating that source with a GitHub one re-points every
  link on each run. The re-pointing is idempotent and appears as drift in every change set; the
  alternative is silently linking against a copy nobody is editing.
- **A configuration must live in a repository whose clone belongs in its context's tree.** One
  source yielding configurations of two different leaves is refused, because it would otherwise
  clone that one repository into two trees.
- **The repositories root reaches the code as a parameter, not by reading the constant.** The
  constant is passed in at the top so that a test can supply its own, which is what keeps path
  assertions from differing between a Windows machine and a Linux build agent.
- **`dotfiles_repository` and `repositories_directory_path` leave the schema**, and the account is
  the only setting a configuration still declares.
- **Every configuration in a loaded set must be readable as a whole set**: `everywhere` and exactly
  one variant, since the leaf derivation is total only when a variant is present and the invocation
  names a machine that is one of the two classes rather than a class of its own.
