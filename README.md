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

Every invocation names which machine it is on. Each configuration declares which machines it is for, so where they are read from cannot change which of them apply.

```bash
# Report what would change on a personal machine, without touching it
cargo run -- plan --context personal

# Read a local directory of configurations instead of the default remote
cargo run -- plan --context personal --source local:config

# Read several sources, merged in the order given
cargo run -- apply --context work --source github:owner/repo/config --source local:config
```

A source is a directory: every `*.dotconfig.json` directly in it is loaded — subdirectories are not descended into — and each one applies only if it declares `everywhere` or the machine named.

`plan` and `apply` exit non-zero when the machine is left unconverged — whether because something drifted, failed, is held open by whatever is executing it, or could not be read at all.

## Design Decisions

Recorded in full under [`docs/adr/`](docs/adr/); the vocabulary is in [`CONTEXT.md`](CONTEXT.md). In short:

- **Configuration is a desired state, applied as a change set** (ADR 0001). Idempotency is not implemented per resource kind — it is what remains when nothing has drifted. The machine is the only source of truth, so there is no state file to lose or to be lied to by.
- **Readiness is observed, not ordered** (ADR 0004). A resource whose tools are absent is unassessable rather than failed, and apply repeats until a pass changes nothing. Kind order survives as a safety property: applications are installed before anything links into their configuration directories.
- **Convergence is additive** (ADR 0005). Withdrawing a declaration ends the tool's interest in a resource; it does not undo it.
- **Plan cannot change the machine** (ADR 0006). Reading a machine and changing one are separate capabilities, so plan's guarantee is checked by the compiler rather than by review. Presence checks come from a fixed set of forms rather than arbitrary shell.
- **A configuration declares which machines it is for** (ADR 0014). An invocation names the machine it is on and nothing else; a directory of configurations composes by what its files declare, so pointing at a local checkout cannot change which of them apply.
- **Every unreadable configuration is reported, and none is applied** (ADR 0009). A run that cannot read every configuration it was given names all of them at once, each with the path within it that failed, and converges nothing — a partial desired state is a different one, not a smaller one.
- **The schema is generated from the types.** `build.rs` writes [`schema/configuration_schema.json`](schema/configuration_schema.json) on every build, so it cannot describe a shape the tool would refuse.
