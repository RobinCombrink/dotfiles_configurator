# dotfiles_configurator

Brings a machine into the state a declarative configuration describes — programs present, dotfiles linked, repositories cloned — by reading what is already true and changing only what is not.

## What it does

Reads declarative JSON configuration files (from a local directory, or from paths in a GitHub repository) that declare the **desired state** of a machine as a set of **resources**. It reads each resource's **actual state** from the machine, compares the two to produce a **change set**, and applies only that. Applying to a machine with no drift changes nothing, which is what makes repeated runs safe.

Resource kinds:
- **Repository** — cloned from GitHub
- **Application** — installed by downloading and running an installer, from a URL or a GitHub release asset
- **Package** — installed by a package manager (winget, cargo)
- **Symlink** — a file or directory in the dotfiles repository, linked into place
- **Registration** — a record this machine holds in another tool's configuration
- **Command** — an arbitrary shell invocation (Bash, PowerShell, Command Prompt, WSL), the escape hatch

Alongside them, **notices** carry messages about things the tool cannot do. They never converge and are reported by every change set.

## Tech Stack

- **Rust** — core language
- **clap** — CLI argument parsing with subcommands
- **octocrab** — GitHub API client for fetching configs and release assets
- **git2** — repository cloning
- **schemars** — JSON Schema generation from Rust types
- **tokio** — async runtime
- **cucumber** — the feature scenarios under `tests/features`

## Running locally

Requires the GitHub CLI (`gh`) to be installed and authenticated wherever a run reaches GitHub: reading configurations from a `github:` source, cloning a repository, or installing an application from a release. Credentials are obtained at the moment they are needed, so planning against a `local:` source asks for none.

Every invocation names which class of machine it is on — `personal` or `work`. Each configuration declares which machines it is for — `everywhere`, `personal` or `work` — so where they are read from cannot change which of them apply.

```bash
# Report what would change on a personal machine, without touching it
cargo run -- plan --machine personal

# Read a local directory of configurations instead of the default remote
cargo run -- plan --machine personal --source local:config

# Read several sources, merged in the order given
cargo run -- apply --machine work --source github:owner/repo/config --source local:config
```

A source is a directory: every `*.dotconfig.json` directly in it is loaded — subdirectories are not descended into — and each one applies only if it declares `everywhere` or the class named. A run reads one configuration for every machine and exactly one for this class, and refuses a set missing either.

`plan` and `apply` exit non-zero when the machine is left unconverged — whether because something drifted, failed, is held open by whatever is executing it, or could not be read at all.

## Design Decisions

Recorded in full under [`docs/adr/`](docs/adr/); the vocabulary is in [`CONTEXT.md`](CONTEXT.md). In short:

- **Configuration is a desired state, applied as a change set** (ADR 0001). Idempotency is not implemented per resource kind — it is what remains when nothing has drifted. The machine is the only source of truth, so there is no state file to lose or to be lied to by.
- **Readiness is observed, not ordered** (ADR 0004). A resource whose tools are absent is unassessable rather than failed, and apply repeats until a pass changes nothing. Kind order survives as a safety property: applications are installed before anything links into their configuration directories.
- **Convergence is additive** (ADR 0005). Withdrawing a declaration ends the tool's interest in a resource; it does not undo it.
- **Plan cannot change the machine** (ADR 0006). Reading a machine and changing one are separate capabilities, so plan's guarantee is checked by the compiler rather than by review. Presence checks come from a fixed set of forms rather than arbitrary shell.
- **A configuration declares which machines it is for** (ADR 0014). An invocation names the class of machine it is on and nothing else; a directory of configurations composes by what its files declare, so pointing at a local checkout cannot change which of them apply.
- **A resource's location is derived, never declared** (ADR 0025). A clone's directory is a per-platform root joined with a leaf that follows the configuration's context; a configuration's dotfiles come from the source it was read from — a GitHub source from that repository's clone, a local source from the checkout containing it. The only setting a configuration still declares is the account it acts as.
- **An origin is a sealed witness, not a field** (ADR 0027). A resource carries the configuration it came from as a value with no accessor, so it can only be observed through the answers that origin produces — its files root, its identity, its requirements — and no resource can be read against another configuration's settings.
- **Every unreadable configuration is reported, and none is applied** (ADR 0009). A run that cannot read every configuration it was given names all of them at once, each with the path within it that failed, and converges nothing — a partial desired state is a different one, not a smaller one.
- **A build reads one generation back and rewrites what it can** (ADR 0026). A document stating the generation below this build is migrated as it is read, and an apply writes the migrated shape back where the source is a local directory. Anything older is refused as too old, which a person resolves by running an intervening build once.
- **The schema is generated from the types.** `build.rs` writes [`schema/configuration_schema.json`](schema/configuration_schema.json) on every build, so it cannot describe a shape the tool would refuse.
