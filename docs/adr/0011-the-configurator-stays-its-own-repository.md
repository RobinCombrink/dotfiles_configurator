# The configurator stays its own repository

Status: accepted (2026-07-29, convergence planning session); recorded here 2026-08-05, once the
plan that had been carrying it was spent.

This crate is not a member of the `dotfiles` Cargo workspace and is not going to become one. It
builds, versions and releases here, and reaches a machine as a release asset.

A cycle forecloses membership. This tool installs the Rust toolchain — the dotfiles
configuration declares rustup as an application, presence-checked by `rustup` on the search path
— so any route that installs the tool with Cargo asks a machine for the thing the tool exists to
supply. ADR 0008 follows that cycle as far as how the binary arrives. The repository boundary is
the same cycle read one step earlier: every crate in the dotfiles workspace is installed with
`cargo install --git` against that workspace (dotfiles ADR 0007), so membership is not a neutral
packaging detail there. It is the one route this crate cannot take.

`workspace-marginal-cost` pulls the other way, which is why the boundary needs stating at all.
The convention holds that another small crate in an existing workspace is near-free — one
lockfile, one `cargo clippy`, one `cargo test` — and that N separate projects is the real cost
driver; dotfiles ADR 0007 applied it to reject a repository per tool. It prices maintenance
correctly and still does not decide this, because what is at stake is reachability rather than
maintenance. Applied here without the cycle in hand, the convention yields the conclusion that
the split is an oversight.

## Considered options

- **Fold the crate into the `dotfiles` workspace alongside `tools/`.** The uniformity
  `workspace-marginal-cost` asks for, and the arrangement every other tool in that repository
  already has. Rejected for the cycle: the workspace's tools are reached through Cargo, and a
  machine has Cargo because this tool put it there.
- **Share that workspace across the boundary, by git submodule or a vendored copy.** A workspace
  member must be a path inside the workspace, so keeping both the separate repository and the
  membership takes one of these. Rejected on two counts: neither removes the cycle, only
  relocates it behind a checkout; and a submodule pins a revision by hand, which is a second home
  for the version ADR 0002 gives the manifest.
- **Move the Rust install out of this tool.** The cycle exists only because this tool installs the
  toolchain; a bootstrap step that installed Rust first would dissolve it and make membership
  ordinary. Rejected because it grows the out-of-band first step ADR 0008 exists to keep small,
  trading a declared resource for a script no configuration mentions.
- **Record nothing, since the split is cheap to reverse.** How the convergence plan left it, and
  right about the reversibility. Rejected on the reading it leaves standing: being easy to reverse
  is a reason not to fear the decision, not a reason for the next reader to re-derive it from a
  cycle spanning two repositories.

## Consequences

- A second lockfile, and a second `cargo clippy`/`cargo test` invocation — precisely the cost
  `workspace-marginal-cost` prices, paid deliberately. This repository consumes the same shared
  lefthook jobs the dotfiles workspace does, so what runs twice is the same set of checks rather
  than a divergent one.
- Staleness is answered differently on each side of the boundary. A `tools/` crate is fingerprinted
  by its subtree hash because nothing bumps versions there (dotfiles ADR 0007); here the manifest
  owns the version (ADR 0002) and the build stamps it (ADR 0003), so the comparison is between two
  versions.
- The boundary settles where the tool lives, not how it arrives. ADR 0008 declares the tool as a
  resource sourced from its own release, and as of 2026-08-05 the dotfiles configuration carries no
  such declaration, so nothing on a machine installs or updates this binary.
- The decision stays cheap to reverse, because nothing is built on it. Should the Rust install ever
  leave this tool, the cycle leaves with it and folding the crate in becomes a packaging change.
