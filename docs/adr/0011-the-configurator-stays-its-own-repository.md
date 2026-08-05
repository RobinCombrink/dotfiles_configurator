# The configurator stays its own repository

Status: accepted (2026-07-28, grilling session on convergence); recorded here 2026-08-05

This crate is not a member of the `dotfiles` Cargo workspace and is not going to become one. It
builds, versions and releases here, and reaches a machine as a release asset.

A cycle forecloses membership. This tool installs the Rust toolchain — the dotfiles configuration
declares rustup as an application, presence-checked by `rustup` on the search path — and a crate
in the dotfiles workspace is installed from that workspace with `cargo install --git` (dotfiles
ADR 0007; ADR 0007 here resolves which crates those are). Membership is therefore not a neutral
packaging detail: it is the one route this crate cannot take, because a machine has Cargo only
once this tool has run. ADR 0008 carries the same cycle to how the binary arrives instead.

`workspace-marginal-cost` pulls the other way. It holds that another small crate in an existing
workspace is near-free — one lockfile, one `cargo clippy`, one `cargo test` — and that N separate
projects is the real cost driver; dotfiles ADR 0007 applied it to reject a repository per tool. It
prices maintenance correctly and still does not decide this, because what is at stake is
reachability rather than maintenance.

## Considered options

- **Fold the crate into the `dotfiles` workspace alongside `tools/`.** The uniformity
  `workspace-marginal-cost` asks for, and the arrangement every other tool in that repository
  already has. Rejected for the cycle: those tools are reached through Cargo, and a machine has
  Cargo because this tool put it there.
- **Share that workspace across the boundary, by git submodule or a vendored copy.** A workspace
  member must be a path inside the workspace, so keeping both the separate repository and the
  membership takes one of these. Rejected on two counts: neither removes the cycle, only relocates
  it behind a checkout; and a submodule pins a revision by hand, which is the declared-revision
  copy ADR 0007 exists to remove — measured stale against four crates the day it was written.
- **Move the Rust install out of this tool.** The cycle exists only because this tool installs the
  toolchain; a bootstrap step that installed Rust first would dissolve it and make membership
  ordinary. Rejected because it grows the out-of-band first step ADR 0008 keeps small, trading a
  declared resource for a script no configuration mentions.
- **Record nothing, since the split is cheap to reverse.** Reversibility is real and it is not the
  test. The cycle forcing the split spans two repositories, so neither one states it alone.

## Consequences

- A second lockfile, and a second `cargo clippy`/`cargo test` invocation — precisely the cost
  `workspace-marginal-cost` prices, paid deliberately. This repository consumes the same shared
  lefthook jobs the dotfiles workspace does, so what runs twice is the same set of checks rather
  than a divergent one.
- Staleness is answered differently on each side of the boundary. A `tools/` crate is fingerprinted
  by its subtree hash because nothing bumps versions there (dotfiles ADR 0007); here the committed
  manifest holds a placeholder and the build stamps the released version into the asset (ADR 0003),
  so the comparison is between the installed binary's version and the latest release's.
- The boundary settles where the tool lives, not how it arrives. ADR 0008 declares the tool as a
  resource sourced from its own release, and as of 2026-08-05 the dotfiles configuration carries no
  such declaration, so nothing on a machine installs or updates this binary.
- The decision stays cheap to reverse, because nothing is built on it. Should the Rust install ever
  leave this tool, the cycle leaves with it and folding the crate in becomes a packaging change.
