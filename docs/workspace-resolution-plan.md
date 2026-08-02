# Workspace resolution implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** Replace the four hand-written `revision` declarations with a workspace declaration whose
membership and revision are read from the repository, so a stale declaration cannot report as
converged.

**Architecture:** `plan` reads every source once, then resolves each declared workspace into one
cargo package resource per binary-producing member, then assesses. The revision lives only in the
readings, never in a resource and never in a configuration. Membership and content are read
in-process through `git2` behind a new `ReadMachine` capability.

**Tech Stack:** Rust 2024, `git2` 0.20.2 (already a dependency), `toml` 0.8.23 (already in
`Cargo.lock` transitively), `schemars` 1.0.4, `cucumber` 0.23.0.

## Global constraints

- Warrant is `docs/adr/0007-workspace-membership-and-revision-are-resolved.md`. This is execution,
  not design; the two decisions that go past it are amended into that ADR in Task 4.
- ADR 0010 holds: each state source is read once per change set. One `git2::Repository::open` per
  declared workspace per change set.
- `matches!` is banned in production code (`clippy.toml`). Integration test files carry
  `#![allow(clippy::disallowed_macros)]` at the top — each is its own crate root.
- `tests/convergence.rs` is `harness = false`; plain `#[test]` functions there never run. Non-Gherkin
  integration tests go in their own file, pulling the fake in via
  `#[path = "common/fake_machine.rs"] mod fake_machine;`.
- Comment policy: a line comment must be a dated external measurement, a tool-consumed annotation,
  an ADR or symbol citation, a structural test marker, or a literal annotation. Rationale goes in
  an ADR.
- Gate is `lefthook run stop-gate`, run while changes are unstaged. It is file-scoped, so a clean
  tree reports success without checking anything.
- Conventional Commits, imperative subject, 72 columns including body. Body only for a measured
  number with its comparand or a `BREAKING CHANGE:` footer. No heredoc — write a body file and use
  `git commit -F <path>`.

## File structure

| File | Responsibility |
|---|---|
| `src/configuration/names.rs` | gains `Revision` and `ObjectHash` newtypes |
| `src/configuration/workspace.rs` (new) | `CargoWorkspace`, the declared form |
| `src/configuration/resource.rs` | `CargoSource` loses `Git`, gains a resolution-only `Workspace` |
| `src/configuration.rs` | `Configuration.workspaces`, `DesiredState.workspaces`, merge |
| `src/machine/workspace_reading.rs` (new) | `WorkspaceReading`, `MemberReading`, `Fingerprint`, manifest parsing, member filtering |
| `src/machine.rs` | `ReadMachine::read_cargo_workspace` |
| `src/machine/local.rs` | the `git2` implementation |
| `src/convergence/assess.rs` | third `SourceReadings` field; workspace member assessment |
| `src/convergence/resolve.rs` (new) | expands workspaces into members; re-checks identity |
| `src/convergence.rs` | `plan` returns `Result<ChangeSet>`; `ChangeSet` carries the readings |
| `src/convergence/converge.rs` | installs a member at the revision from the readings |
| `tests/workspace_repository.rs` (new) | git2 reading against a real temporary repository |
| `tests/common/fake_machine.rs` | in-memory workspace state |
| `tests/features/planning.feature` | user-observable scenarios |

---

### Task 1: Read a cargo workspace from its repository

Additive: nothing is removed, so the tree stays green throughout.

**Files:**
- Create: `src/machine/workspace_reading.rs`
- Modify: `src/configuration/names.rs`, `src/machine.rs`, `src/machine/local.rs`, `Cargo.toml`
- Test: `src/machine/workspace_reading.rs` (unit), `tests/workspace_repository.rs` (integration)

**Interfaces:**

Produces, for every later task:

```rust
pub struct Revision(String);      // a full 40-character commit id
pub struct ObjectHash(String);    // any git object id

pub struct Fingerprint {
    pub crate_subtree: ObjectHash,
    pub workspace_manifest: ObjectHash,
    pub lockfile: ObjectHash,
}

pub struct MemberReading {
    pub desired: Fingerprint,
    /// `None` when cargo has not installed it, or when the commit it recorded is
    /// absent from the local clone.
    pub installed: Option<Fingerprint>,
}

pub struct WorkspaceReading {
    pub revision: Revision,
    pub members: BTreeMap<CrateName, MemberReading>,
}

// on ReadMachine
fn read_cargo_workspace(
    &self,
    repository_path: &Path,
    installed: &BTreeMap<CrateName, Revision>,
) -> Result<Option<WorkspaceReading>>;   // Ok(None) = the repository is not cloned
```

- [ ] **Step 1: Add the newtypes**

`Revision` and `ObjectHash` in `src/configuration/names.rs`, following the existing
`#[repr(transparent)]` `From<&str>`/`Display` pattern the other names in that file use. Re-export
both from `src/configuration.rs`.

- [ ] **Step 2: Write the failing unit tests for manifest reading**

In `src/machine/workspace_reading.rs`, against two pure functions over strings:

```rust
#[test]
fn a_workspace_manifest_lists_the_paths_of_its_members() { /* members = [...] */ }

#[test]
fn a_member_path_holding_a_glob_is_refused_by_name() { /* "tools/*" */ }

#[test]
fn a_member_whose_manifest_declares_a_binary_section_is_installable() { }

#[test]
fn a_member_holding_a_main_file_is_installable() { }

#[test]
fn a_member_holding_only_a_library_is_not_installable() { }
```

- [ ] **Step 3: Run them and watch them fail**

`cargo test --lib workspace_reading`

- [ ] **Step 4: Implement the two pure functions**

`member_paths(manifest: &str) -> Result<Vec<String>>` deserialising `[workspace] members` with
`toml`, refusing any entry containing `*` by name. `member_is_installable(manifest: &str,
tree_holds_main: bool, tree_holds_bin_directory: bool) -> bool`. Add `toml = "0.8.23"` to
`[dependencies]` — it is already in `Cargo.lock`, so no new tree is pulled.

- [ ] **Step 5: Run them and watch them pass**

- [ ] **Step 6: Write the failing integration test**

`tests/workspace_repository.rs`, building a throwaway repository with `git2` in a temp directory:
a root `Cargo.toml` and `Cargo.lock`, one member under `tools/alpha` with `src/main.rs`, one
library-only member under `tools/beta`, a commit, and a `refs/remotes/origin/main` ref with the
local branch tracking it. Three behaviours:

```rust
#[test]
fn the_desired_revision_is_the_commit_the_tracked_remote_branch_names() { }

#[test]
fn a_commit_touching_nothing_the_crate_depends_on_leaves_its_fingerprint_unchanged() { }

#[test]
fn a_commit_changing_the_lockfile_changes_every_members_fingerprint() { }

#[test]
fn a_member_that_builds_no_binary_is_not_reported_as_a_member() { }

#[test]
fn a_branch_with_no_tracked_remote_is_refused_rather_than_read_from_the_local_commit() { }
```

Put `#![allow(clippy::disallowed_macros)]` at the top of the file.

- [ ] **Step 7: Run and watch them fail**

- [ ] **Step 8: Implement `read_cargo_workspace` on `LocalMachine`**

One `Repository::open`. `repo.head()` → shorthand → `find_branch(.., BranchType::Local)` →
`upstream()` → `peel_to_commit()` gives the desired `Revision`; a branch with no upstream is an
error naming the branch. Read the root `Cargo.toml` and `Cargo.lock` blob ids from that commit's
tree via `Tree::get_path`; a missing `Cargo.lock` is an error. For each member path, take the
subtree id, decide installability from the tree, and take the crate name from the member's own
manifest. For each installed revision, `find_commit` — a commit absent locally yields
`installed: None` rather than an error.

- [ ] **Step 9: Run and watch them pass**

- [ ] **Step 10: Teach `FakeMachine` the capability**

An in-memory `BTreeMap<PathBuf, WorkspaceReading>` plus a setter, and a counter entry so Task 3 can
assert the read count.

- [ ] **Step 11: Gate and commit**

```
lefthook run stop-gate
git add src/configuration/names.rs src/configuration.rs src/machine.rs \
        src/machine/workspace_reading.rs src/machine/local.rs \
        tests/workspace_repository.rs tests/common/fake_machine.rs Cargo.toml Cargo.lock
git commit -m "feat(machine): read a cargo workspace's members and their content"
```

---

### Task 2: Declare the workspace and resolve its members

The swap. `CargoSource::Git` cannot be removed without `Workspace` arriving, so this is one commit.

**Files:**
- Create: `src/configuration/workspace.rs`, `src/convergence/resolve.rs`
- Modify: `src/configuration.rs`, `src/configuration/resource.rs`, `src/convergence.rs`,
  `src/convergence/assess.rs`, `src/convergence/converge.rs`, `src/main.rs`
- Test: `tests/features/planning.feature`, `tests/convergence.rs`, `tests/reading.rs`

**Interfaces:**

Consumes Task 1's `WorkspaceReading`, `MemberReading`, `Revision`, `read_cargo_workspace`.

Produces:

```rust
pub struct CargoWorkspace { pub repository: GitHubRepository }

pub enum CargoSource {
    Registry,
    Path { path: PathBuf },
    #[serde(skip_deserializing)]
    #[schemars(skip)]
    Workspace { repository: GitHubRepository },
}

// SourceReadings gains
workspaces: BTreeMap<GitHubRepository, Result<WorkspaceReading, DriftReason>>,

pub fn plan(desired_state: &DesiredState, machine: &impl ReadMachine) -> Result<ChangeSet>;
pub struct ChangeSet { /* .. */ pub readings: SourceReadings }
```

- [ ] **Step 1: Write the failing Gherkin scenarios**

In `tests/features/planning.feature` — what a person using the tool observes:

```gherkin
  Scenario: A workspace crate whose content matches the repository is converged
  Scenario: A workspace crate whose content differs from the repository is a change
  Scenario: A commit touching neither the crate nor the lockfile leaves it converged
  Scenario: A crate added to the workspace is planned without editing the configuration
  Scenario: A workspace crate cargo has never installed is a change
  Scenario: A workspace whose repository is not cloned yet contributes no crates
```

- [ ] **Step 2: Add the step definitions to `tests/convergence.rs`**

Backed by `FakeMachine`'s workspace state from Task 1.

- [ ] **Step 3: Run and watch them fail**

`cargo test --test convergence`

- [ ] **Step 4: Add the declared surface**

`CargoWorkspace` in `src/configuration/workspace.rs`, wired into `configuration.rs` as
`Configuration.workspaces` and `DesiredState.workspaces` (both `#[serde(default)]`), merged the
way notices are and deduplicated by repository. Swap `CargoSource::Git` for the skipped
`Workspace` variant.

- [ ] **Step 5: Read the workspaces in `SourceReadings::read_for`**

After the cargo listing, so the installed revisions are available. A declared workspace marks cargo
as needed. Build the `BTreeMap<CrateName, Revision>` from the listing by reusing
`installed_crate_source`, then one `read_cargo_workspace` per declared workspace.

- [ ] **Step 6: Write `resolve`**

`resolve(desired_state, readings) -> Result<Vec<Resource>>`: every declared resource, plus one
`Resource::Package(Package::Cargo(CargoPackage { crate_name, source: Workspace { repository } }))`
per member of each successfully-read workspace. A workspace whose reading is `Err` fails the
resolution by name; a workspace that read as `Ok(None)` — not cloned — contributes nothing. Then
re-check `Identity` across the whole resolved set and reject a conflict naming both claims, the
way `merge_configurations` does at load.

- [ ] **Step 7: Make `plan` fallible and assess the members**

`plan` returns `Result<ChangeSet>`, resolving between reading and assessing, and carries the
readings on the returned `ChangeSet`. `assess_cargo_package` gains the `Workspace` arm: look the
crate up in `readings.workspaces[repository]`, then compare `desired` against `installed` —
converged when equal, drifted naming what differs when not, drifted with "cargo has not installed
it" when `installed` is `None`.

- [ ] **Step 8: Install a member at the resolved revision**

`converge` takes the readings and `converge_cargo_package` reads the revision from them, building
`cargo install --locked --force --git <url> --rev <revision> <crate>`. The URL comes from a new
`GitHubRepository::clone_url()`. Update `apply` to pass `change_set.readings` through, and
`main.rs` for the `Result`.

- [ ] **Step 9: Run the whole suite**

`cargo clippy --workspace --all-targets -- -D warnings` then `cargo test`.

- [ ] **Step 10: Regenerate and inspect the schema**

Confirm by reading the generated `schema/configuration_schema.json` that `git` is gone, that
`workspace` is absent from `CargoSource`, and that `workspaces` is present at the top level. The
`schemars(skip)` behaviour on a variant is asserted by this diff, not assumed.

- [ ] **Step 11: Add the read-count test**

In `tests/reading.rs`: a change set with several workspace crates opens the repository once.

- [ ] **Step 12: Gate and commit**

```
lefthook run stop-gate
git commit -m "feat(config): resolve cargo workspace membership and revision"
```

---

### Task 3: Amend ADR 0007

**Files:** Modify `docs/adr/0007-workspace-membership-and-revision-are-resolved.md`

- [ ] **Step 1: Add the two consequences that go past the accepted decision**

An unreadable workspace — cloned, but with no tracked remote branch, an unparseable manifest, a
glob member or no lockfile — refuses the whole run rather than reporting drift, per ADR 0009,
because it decides which resources exist. A workspace whose repository is not cloned contributes no
members, because they cannot be named; the repository resource already reports the clone as the
change, so bootstrap is unaffected. Membership is the members that build a binary, since a
library-only member cannot be installed at all and would otherwise fail on every run forever.

- [ ] **Step 2: Mark the amendment date in the status line, as the existing amendment does**

- [ ] **Step 3: Commit**

```
git commit -m "docs(adr): record what an unreadable workspace does to a run"
```

---

### Task 4: Rewrite the dotfiles declarations

A different repository, so a different commit. Run from a temporary worktree if the shared checkout
is on another session's branch.

**Files:** Modify `C:\Repositories\Personal\dotfiles\config\everywhere.dotconfig.json`

- [ ] **Step 1: Replace the four git-sourced declarations with one workspace entry**

Remove the `stop-gate`, `ci-checks`, `claude-workspace` and `session-mining` resources; add
`"workspaces": [{ "repository": { "owner": "RobinCombrink", "repository": "dotfiles" } }]`. The
registry crates (`committed`, `cargo-llvm-cov`) are untouched.

- [ ] **Step 2: Confirm what changes on this machine**

`adr-numbering` and `claude-statusline` are workspace members that were not declared, so they
become newly installed. Confirm that is wanted before committing.

- [ ] **Step 3: Commit in that repository**

```
git commit -m "chore(config): install the workspace tools by resolution"
```

---

### Task 5: Verify against the live machine

- [ ] **Step 1: Record the comparand**

Run `plan` from `main` and from the branch against the same machine in the same session, and record
which of the four crates each reports as converged. The old code reported all four converged on
2026-08-01 while two of them named older content than the repository held; the new code has to
disagree with it on exactly the crates whose content differs.

- [ ] **Step 2: Record what the added repository read costs a change set**

Same host, same session, before and after — a wall-clock pair, not a claim.

- [ ] **Step 3: Run the gate with the changes unstaged, then confirm CI on the pushed commit**

`lefthook run stop-gate`, then the three required checks (gitleaks, CI, conventional-commits) via
the `dotfiles:ci-results` skill. A grep that found nothing is not evidence.

- [ ] **Step 4: Remove this plan once its phases have landed**

`git rm docs/workspace-resolution-plan.md`, committed as `docs(plan): remove the workspace
resolution plan whose phases have landed`, following the precedent of `bc5b936`.
