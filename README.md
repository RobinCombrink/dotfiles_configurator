# dotfiles_configurator

Plan-and-execute CLI for managing dotfiles, programs, and binaries from GitHub.

## What it does

Reads declarative JSON configuration files (local or fetched from GitHub repositories) that describe what programs to install, which dotfiles to symlink, and what shell commands to run. Builds an execution plan from the configuration and either executes it or performs a dry run.

Supports:
- Downloading application binaries from URLs
- Downloading release assets from GitHub repositories (by exact name, contains, or suffix match)
- Cloning git repositories
- Symlinking dotfiles (files and directories)
- Running shell commands (Bash, PowerShell, Command Prompt, WSL)
- Multiple configuration sources (local files, single remote, multiple remotes)

## Tech Stack

- **Rust** — core language
- **clap** — CLI argument parsing with subcommands
- **octocrab** — GitHub API client for fetching configs and release assets
- **git2** — repository cloning
- **schemars** — JSON Schema generation from Rust types
- **tokio** — async runtime for concurrent downloads
- **indicatif** — progress bars for long-running operations

## Running locally

Requires the GitHub CLI (`gh`) to be installed and authenticated.

```bash
# Dry run from a remote config
cargo run -- remote -o RobinCombrink -r dotfiles dry-run

# Execute from local config files
cargo run -- local -d config/

# Execute from multiple remotes
cargo run -- remotes --remote owner,repo,config/path.json
```

## Design Decisions

- **Plan/execute separation with dry-run support**: Configuration is parsed into an execution plan before any side effects occur. Dry-run mode shows what would happen without making changes — useful for validating configs on new machines.
- **Declarative JSON configuration with schema**: Uses `schemars` to generate a JSON Schema from Rust types, enabling editor autocompletion and validation in config files.
- **Platform-aware shell commands**: Shell commands are tagged with their target shell (Bash, PowerShell, etc.), allowing cross-platform configs that execute the right variant per OS.
