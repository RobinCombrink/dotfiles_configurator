# A configuration's settings belong to it, not to the machine

Status: accepted (2026-08-08, design session on configurations spanning two GitHub accounts)

`MachineSettings` names the GitHub account, the dotfiles repository and the repositories
directory, and none of the three is a fact about the machine. Each is a property of the repository
the configuration was written in, which is the only thing that can know its own answer. A
configuration declares them, and every resource it declares inherits them.

The account is the case that forces it. Authenticating as one account for a whole run means a set
of configurations spanning two accounts cannot converge, and the failure is unreadable: GitHub
answers an unauthorised private repository with the same absence it gives a misspelt one, so a
wrong account is indistinguishable from a repository that does not exist.

An owner is an address rather than a credential, which is what rules out matching a resource's
owner to an account. A public repository is read by any account, and a private one by any account
holding access to it, so owners do not map onto accounts as a function — a configuration declaring
third-party sources names owners corresponding to no account at all. Inheritance gives every one
of those a defined answer, with no field on a resource and no table to maintain.

Reading a configuration source is the one place an owner does select the account, because nothing
in the source has been parsed at the moment the credential is needed. A source is read as the
account that owns it, and a configuration declaring an account other than the owner of the source
it was read from is refused. The owner is evidence and the declaration is a claim, which is what
separates this from one fact stored twice.

## Considered options

- **An account named on each resource.** Restates on every resource what its configuration already
  knows, and puts a credential concern into the schema of every resource kind.
- **An account per owner, declared once and matched against a resource's owner.** Needs no new
  field, since a resource already carries its owner, but rests on an owner mapping to exactly one
  account. That does not hold: the owners of third-party releases and clones correspond to no
  account, so each would need an entry naming one that does not exist, or a fallback — at which
  point the fallback is doing the work and the table is not.
- **One credential spanning both accounts.** Moves the problem to GitHub rather than answering it
  here, and buys the span by granting one identity access to the other's private repositories.

## Consequences

- **Configurations loaded together no longer agree on machine settings.** The check refusing
  disagreement is deleted rather than relaxed, because disagreement becomes the ordinary case
  rather than the fault it is treated as today.
- **A resolved resource carries its configuration's settings**, and the declared and resolved forms
  become distinct types, so a change set holding a resource with nothing attached does not compile.
- **A configuration's files resolve inside the repository it was written in.** Symlinks join onto
  the declaring configuration's dotfiles repository rather than onto a single global one, which is
  what lets configurations from two repositories both contribute them.
- **Credentials for more than one account are expected on a single machine.** A configuration with
  universal reach is owned by whoever wrote it, so a machine applying it alongside a locally-owned
  one holds both accounts and converges each configuration's resources under its own.
- **An account is not part of a resource's identity.** It is a credential rather than something a
  resource claims about a machine, so two clones sharing a name are told apart by the directory
  each claims, never by which account reads them.
- **A configuration source owned by an organisation cannot be read**, because its owner names no
  account. The source declaration gains an account of its own when that case arrives.
- **A git-sourced package install carries its account in the fetch URL.** Cargo's git database has
  no configured remote and lives outside any repository tree, so neither directory nor remote-URL
  routing reaches it, and the URL is the only channel that does.
