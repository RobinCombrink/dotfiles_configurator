---
status: accepted
---

# A symlink may be sourced in a repository its configuration declares

A symlink links a path in its configuration's dotfiles repository, declared as `source_path`. It may
instead name a repository the same configuration declares, as
`source: {repository: {owner, repository}, path}`, and links into that repository's clone in the
configuration's repositories directory. A link declares exactly one of the two. Linking a directory
another repository owns otherwise needed a `command` resource running `ln` or `mklink`, which has no
identity, is not assessed as a link, and is never withdrawn when it dangles.

The source repository must be declared as a `repository` resource of the same configuration, and a
configuration naming one it does not declare is refused when it is read. Declaring the clone is what
puts it on the machine, so a link into an undeclared repository is one that a fresh machine can
never converge, and the only honest answer is refusal rather than a link blocked forever. The link
is blocked until the clone holds `.git`, so the clone and the link arrive in one apply. `path` is a
path within that repository, refused when the configuration is read if it is rooted or climbs out
through `..`, by the type that already holds an estate's folder to the same rule.

Once resolved, such a link is assessed and converged exactly as one into the dotfiles repository,
and the withdrawal of dangling links beside a declared one covers the clone of every repository a
link is sourced in, as it covers the dotfiles repository.

## Why ADR 0025's rejected option differs

ADR 0025 rejected symlinks resolving against a declared repository rather than the source, because
that declaration always equals the repository the configuration was read from, a copy of evidence
checked against none of it. A source repository here is a different repository, which nothing else
names: its declaration is the only home of the fact, not a copy of one. The dotfiles repository is
still never declared, so `source_path` is unchanged.

## Considered options

- **An `in` tag on `source`, shaped like a search path directory's.** The dotfiles source stays
  `source_path`, so `source` has one shape and the tag would discriminate nothing.
- **One tagged `source` for both, `source_path` becoming `{"in": "dotfiles", "path": ...}`.**
  Uniform, at the price of rewriting every existing link declaration for no behaviour.
- **Registering the repository as a Claude plugin marketplace.** `claude plugin marketplace add`
  and `install` write `extraKnownMarketplaces` and `enabledPlugins` into `~/.claude/settings.json`,
  a file the dotfiles repository owns and links.
- **Cloning an undeclared source repository implicitly.** A clone nothing declares, at a depth and
  as an account nothing states, which the claim on that clone's directory could then contradict.

## Consequences

- **A link names its repository twice in one document**: once as the clone and once as its source.
  The second is checked against the first when the configuration is read.
- **A repository-sourced link displays as `<path> in <owner>/<repo>`**, and a converge that finds
  nothing at its source names the repository rather than the dotfiles repository.
- **It is part of generation 13**, so a configuration declaring one states generation 13.
