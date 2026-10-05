#![allow(clippy::disallowed_macros)]

use {
    dotfiles_configurator::{
        configuration::{BinaryName, CrateName},
        machine::{local::workspace, workspace_reading::Revision},
    },
    git2::{IndexAddOption, Repository, Signature},
    std::{fs, path::Path},
};

struct TemporaryRepository {
    repository: Repository,
    directory: tempfile::TempDir,
}

impl TemporaryRepository {
    fn create() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path()).unwrap();
        repository
            .remote("origin", "https://example.invalid/dotfiles.git")
            .unwrap();
        Self {
            repository,
            directory,
        }
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn commit(&self, message: &str) -> Revision {
        let mut index = self.repository.index().unwrap();
        index.add_all(["*"], IndexAddOption::DEFAULT, None).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = self.repository.find_tree(tree_id).unwrap();
        let signature = Signature::now("Alice", "alice@example.invalid").unwrap();
        let parents = match self
            .repository
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok())
        {
            Some(parent) => vec![parent],
            None => Vec::new(),
        };
        let parents: Vec<&git2::Commit> = parents.iter().collect();

        let id = self
            .repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &parents,
            )
            .unwrap();
        Revision::from(id.to_string())
    }

    fn push(&self) {
        let head = self.repository.head().unwrap();
        let branch = head.shorthand().unwrap().to_owned();
        let commit = head.peel_to_commit().unwrap();

        self.repository
            .reference(
                &format!("refs/remotes/origin/{branch}"),
                commit.id(),
                true,
                "pushed",
            )
            .unwrap();

        let mut configuration = self.repository.config().unwrap();
        configuration
            .set_str(&format!("branch.{branch}.remote"), "origin")
            .unwrap();
        configuration
            .set_str(
                &format!("branch.{branch}.merge"),
                &format!("refs/heads/{branch}"),
            )
            .unwrap();
    }

    fn head_revision(&self) -> Revision {
        Revision::from(
            self.repository
                .head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
                .to_string(),
        )
    }
}

fn member_entry(name: &str, dependencies: &[&str]) -> String {
    format!(
        "[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\ndependencies = [{}]\n",
        quoted(dependencies)
    )
}

fn registry_entry(name: &str, version: &str, dependencies: &[&str]) -> String {
    format!(
        "[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
         checksum = \"{}\"\ndependencies = [{}]\n",
        "ab".repeat(32),
        quoted(dependencies)
    )
}

fn quoted(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn lock_of(entries: &[String]) -> String {
    format!("version = 4\n\n{}", entries.join("\n"))
}

fn workspace_holding_a_binary_and_a_library() -> TemporaryRepository {
    let repository = TemporaryRepository::create();
    repository.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"tools/alpha\", \"tools/beta\"]\n",
    );
    repository.write(
        "Cargo.lock",
        &lock_of(&[member_entry("alpha", &[]), member_entry("beta", &[])]),
    );
    repository.write(
        "tools/alpha/Cargo.toml",
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n",
    );
    repository.write("tools/alpha/src/main.rs", "fn main() {}\n");
    repository.write(
        "tools/beta/Cargo.toml",
        "[package]\nname = \"beta\"\nversion = \"0.1.0\"\n",
    );
    repository.write("tools/beta/src/lib.rs", "pub fn beta() {}\n");
    repository.commit("the workspace");
    repository.push();
    repository
}

fn built_from_what_the_workspace_holds(
    repository: &TemporaryRepository,
    crate_name: &str,
    built_from: &Revision,
) -> bool {
    let crate_name = CrateName::from(crate_name);
    let reading = workspace::read(repository.path()).unwrap().unwrap();
    let built = workspace::read_at(repository.path(), built_from).unwrap();

    built.get(&crate_name) == Some(&reading.members[&crate_name].desired)
}

fn alpha(repository: &TemporaryRepository, built_from: &Revision) -> bool {
    built_from_what_the_workspace_holds(repository, "alpha", built_from)
}

#[test]
fn the_desired_revision_is_the_commit_the_tracked_remote_branch_names() {
    let repository = workspace_holding_a_binary_and_a_library();

    let reading = workspace::read(repository.path()).unwrap().unwrap();

    assert_eq!(reading.revision, repository.head_revision());
}

#[test]
fn a_commit_that_has_not_been_pushed_is_not_what_a_crate_is_installed_from() {
    let repository = workspace_holding_a_binary_and_a_library();
    let pushed = repository.head_revision();
    repository.write(
        "tools/alpha/src/main.rs",
        "fn main() { println!(\"new\") }\n",
    );
    let unpushed = repository.commit("work in progress");

    let reading = workspace::read(repository.path()).unwrap().unwrap();

    assert_eq!(reading.revision, pushed);
    assert_ne!(reading.revision, unpushed);
}

#[test]
fn a_commit_touching_nothing_the_crate_is_built_from_leaves_it_converged() {
    let repository = workspace_holding_a_binary_and_a_library();
    let installed_from = repository.head_revision();
    repository.write("README.md", "a change to something else entirely\n");
    repository.commit("unrelated");
    repository.push();

    assert!(alpha(&repository, &installed_from));
}

fn alpha_through_left_pad_and_pad_core_on_pad_bytes(pad_bytes_version: &str) -> String {
    lock_of(&[
        member_entry("alpha", &["left-pad"]),
        member_entry("beta", &[]),
        registry_entry("left-pad", "1.3.0", &["pad-core"]),
        registry_entry("pad-core", "1.0.0", &["pad-bytes"]),
        registry_entry("pad-bytes", pad_bytes_version, &[]),
    ])
}

#[test]
fn a_new_version_of_a_dependency_several_levels_beneath_it_drifts_it() {
    let repository = workspace_holding_a_binary_and_a_library();
    repository.write(
        "tools/alpha/Cargo.toml",
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n\n[dependencies]\nleft-pad = \"1\"\n",
    );
    repository.write(
        "Cargo.lock",
        &alpha_through_left_pad_and_pad_core_on_pad_bytes("1.0.0"),
    );
    repository.commit("alpha pads");
    repository.push();
    let installed_from = repository.head_revision();
    repository.write(
        "Cargo.lock",
        &alpha_through_left_pad_and_pad_core_on_pad_bytes("1.0.1"),
    );
    repository.commit("bump pad-bytes");
    repository.push();

    assert!(!alpha(&repository, &installed_from));
}

fn workspace_of_two_tools() -> TemporaryRepository {
    let repository = TemporaryRepository::create();
    repository.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"tools/alpha\", \"tools/delta\"]\n",
    );
    repository.write(
        "tools/alpha/Cargo.toml",
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n",
    );
    repository.write("tools/alpha/src/main.rs", "fn main() {}\n");
    repository.write(
        "tools/delta/Cargo.toml",
        "[package]\nname = \"delta\"\nversion = \"0.1.0\"\n",
    );
    repository.write("tools/delta/src/main.rs", "fn main() {}\n");
    repository.write(
        "Cargo.lock",
        &lock_of(&[member_entry("alpha", &[]), member_entry("delta", &[])]),
    );
    repository.commit("two tools");
    repository.push();
    repository
}

#[test]
fn a_dependency_added_to_one_tool_leaves_another_tool_converged() {
    let repository = workspace_of_two_tools();
    let installed_from = repository.head_revision();
    repository.write(
        "tools/delta/Cargo.toml",
        "[package]\nname = \"delta\"\nversion = \"0.1.0\"\n\n[dependencies]\nleft-pad = \"1\"\n",
    );
    repository.write(
        "Cargo.lock",
        &lock_of(&[
            member_entry("alpha", &[]),
            member_entry("delta", &["left-pad"]),
            registry_entry("left-pad", "1.3.0", &[]),
        ]),
    );
    repository.commit("delta pads");
    repository.push();

    assert!(alpha(&repository, &installed_from));
}

fn tools_converged_once_the_workspace_manifest_reads(
    manifest: &str,
    locked_members: &[&str],
) -> Vec<bool> {
    let repository = workspace_of_two_tools();
    let installed_from = repository.head_revision();
    repository.write("Cargo.toml", manifest);
    repository.write(
        "tools/epsilon/Cargo.toml",
        "[package]\nname = \"epsilon\"\nversion = \"0.1.0\"\n",
    );
    repository.write("tools/epsilon/src/main.rs", "fn main() {}\n");
    let entries: Vec<String> = locked_members
        .iter()
        .map(|name| member_entry(name, &[]))
        .collect();
    repository.write("Cargo.lock", &lock_of(&entries));
    repository.commit("the workspace manifest changes");
    repository.push();

    ["alpha", "delta"]
        .iter()
        .map(|name| built_from_what_the_workspace_holds(&repository, name, &installed_from))
        .collect()
}

#[test]
fn a_member_added_to_the_workspace_leaves_every_existing_tool_converged() {
    let converged = tools_converged_once_the_workspace_manifest_reads(
        "[workspace]\nresolver = \"2\"\nmembers = [\"tools/alpha\", \"tools/delta\", \"tools/epsilon\"]\n",
        &["alpha", "delta", "epsilon"],
    );

    assert_eq!(converged, vec![true, true]);
}

#[test]
fn a_release_profile_change_drifts_every_tool() {
    let converged = tools_converged_once_the_workspace_manifest_reads(
        "[workspace]\nresolver = \"2\"\nmembers = [\"tools/alpha\", \"tools/delta\"]\n\n[profile.release]\nlto = true\n",
        &["alpha", "delta"],
    );

    assert_eq!(converged, vec![false, false]);
}

fn refusal_of(lock: &str) -> String {
    let repository = workspace_holding_a_binary_and_a_library();
    repository.write("Cargo.lock", lock);
    repository.commit("a lock cargo did not write");
    repository.push();

    let error = workspace::read(repository.path()).unwrap_err();
    format!("{error:#}")
}

#[test]
fn a_lock_naming_a_dependency_it_holds_no_entry_for_refuses_the_workspace() {
    let error = refusal_of(&lock_of(&[
        member_entry("alpha", &["ghost 1.0.0"]),
        member_entry("beta", &[]),
    ]));

    assert!(
        error.contains("Cargo.lock") && error.contains("ghost"),
        "expected the message to name the lock and the dependency, got: {error}"
    );
}

#[test]
fn a_lock_holding_no_entry_for_a_member_refuses_the_workspace() {
    let error = refusal_of(&lock_of(&[member_entry("beta", &[])]));

    assert!(
        error.contains("Cargo.lock") && error.contains("alpha"),
        "expected the message to name the lock and the member, got: {error}"
    );
}

fn workspace_where_alpha_declares(dependency_sections: &str) -> TemporaryRepository {
    let repository = workspace_holding_a_binary_and_a_library();
    repository.write(
        "tools/alpha/Cargo.toml",
        &format!("[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n\n{dependency_sections}"),
    );
    repository.commit("alpha declares its dependencies");
    repository.push();
    repository
}

fn alpha_once_beta_changes(repository: &TemporaryRepository) -> bool {
    let installed_from = repository.head_revision();
    repository.write(
        "tools/beta/src/lib.rs",
        "pub fn beta() { println!(\"changed\") }\n",
    );
    repository.commit("change beta");
    repository.push();

    alpha(repository, &installed_from)
}

#[test]
fn a_commit_changing_a_workspace_crate_it_depends_on_by_path_drifts_it() {
    let repository =
        workspace_where_alpha_declares("[dependencies]\nbeta = { path = \"../beta\" }\n");

    assert!(!alpha_once_beta_changes(&repository));
}

#[test]
fn a_commit_changing_a_crate_its_dependency_depends_on_drifts_it() {
    let repository =
        workspace_where_alpha_declares("[dependencies]\ngamma = { path = \"../gamma\" }\n");
    repository.write(
        "tools/gamma/Cargo.toml",
        "[package]\nname = \"gamma\"\nversion = \"0.1.0\"\n\n[dependencies]\nbeta = { path = \"../beta\" }\n",
    );
    repository.write("tools/gamma/src/lib.rs", "pub fn gamma() {}\n");
    repository.commit("gamma sits between alpha and beta");
    repository.push();

    assert!(!alpha_once_beta_changes(&repository));
}

#[test]
fn a_commit_changing_a_crate_inherited_from_the_workspace_dependencies_drifts_it() {
    let repository =
        workspace_where_alpha_declares("[dependencies]\nbeta = { workspace = true }\n");
    repository.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"tools/alpha\", \"tools/beta\"]\n\n[workspace.dependencies]\nbeta = { path = \"tools/beta\" }\n",
    );
    repository.commit("the workspace declares beta");
    repository.push();

    assert!(!alpha_once_beta_changes(&repository));
}

#[test]
fn a_commit_changing_a_crate_it_depends_on_for_one_platform_drifts_it() {
    let repository = workspace_where_alpha_declares(
        "[target.'cfg(windows)'.dependencies]\nbeta = { path = \"../beta\" }\n",
    );

    assert!(!alpha_once_beta_changes(&repository));
}

#[test]
fn a_commit_changing_a_crate_it_depends_on_to_build_drifts_it() {
    let repository =
        workspace_where_alpha_declares("[build-dependencies]\nbeta = { path = \"../beta\" }\n");

    assert!(!alpha_once_beta_changes(&repository));
}

#[test]
fn a_commit_changing_a_crate_only_its_tests_depend_on_leaves_it_converged() {
    let repository =
        workspace_where_alpha_declares("[dev-dependencies]\nbeta = { path = \"../beta\" }\n");

    assert!(alpha_once_beta_changes(&repository));
}

#[test]
fn a_member_that_builds_no_binary_is_not_reported_as_a_member() {
    let repository = workspace_holding_a_binary_and_a_library();

    let reading = workspace::read(repository.path()).unwrap().unwrap();

    assert!(reading.members.contains_key(&CrateName::from("alpha")));
    assert!(!reading.members.contains_key(&CrateName::from("beta")));
}

#[test]
fn a_branch_that_tracks_no_remote_is_refused_rather_than_read_from_the_local_commit() {
    let repository = TemporaryRepository::create();
    repository.write("Cargo.toml", "[workspace]\nmembers = [\"tools/alpha\"]\n");
    repository.write("Cargo.lock", "version = 4\n");
    repository.write("tools/alpha/Cargo.toml", "[package]\nname = \"alpha\"\n");
    repository.write("tools/alpha/src/main.rs", "fn main() {}\n");
    repository.commit("never pushed");

    let error = workspace::read(repository.path()).unwrap_err();

    assert!(
        format!("{error:#}").contains("tracks no remote branch"),
        "expected the message to say the branch tracks nothing, got: {error:#}"
    );
}

#[test]
fn an_abbreviated_commit_still_finds_the_content_it_names() {
    let repository = workspace_holding_a_binary_and_a_library();
    let installed_from = repository.head_revision();
    let abbreviated = Revision::from(&installed_from.as_ref()[..9]);
    repository.write("README.md", "a change to something else entirely\n");
    repository.commit("unrelated");
    repository.push();

    assert!(alpha(&repository, &abbreviated));
}

#[test]
fn a_commit_absent_from_the_clone_cannot_be_read() {
    let repository = workspace_holding_a_binary_and_a_library();
    let absent = Revision::from("0123456789012345678901234567890123456789");

    let error = workspace::read_at(repository.path(), &absent).unwrap_err();

    assert!(
        format!("{error:#}").contains("0123456789012345678901234567890123456789"),
        "expected the message to name the commit, got: {error:#}"
    );
}

#[test]
fn a_member_is_read_with_every_binary_it_declares() {
    let repository = TemporaryRepository::create();
    repository.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"tools/mining\"]\n",
    );
    repository.write("Cargo.lock", &lock_of(&[member_entry("mining", &[])]));
    repository.write(
        "tools/mining/Cargo.toml",
        "[package]\nname = \"mining\"\nversion = \"0.1.0\"\n\n\
         [[bin]]\nname = \"sweep\"\npath = \"src/bin/sweep/main.rs\"\n\n\
         [[bin]]\nname = \"tool-use-statistics\"\npath = \"src/bin/tool_use_statistics.rs\"\n",
    );
    repository.write("tools/mining/src/bin/sweep/main.rs", "fn main() {}\n");
    repository.write(
        "tools/mining/src/bin/tool_use_statistics.rs",
        "fn main() {}\n",
    );
    repository.commit("the workspace");
    repository.push();

    let reading = workspace::read(repository.path()).unwrap().unwrap();

    let binaries: Vec<String> = reading.members[&CrateName::from("mining")]
        .binaries
        .iter()
        .map(BinaryName::to_string)
        .collect();
    assert_eq!(binaries, vec!["sweep", "tool-use-statistics"]);
}

#[test]
fn a_member_holding_no_binaries_directory_is_read_rather_than_refused() {
    let repository = workspace_holding_a_binary_and_a_library();

    let reading = workspace::read(repository.path()).unwrap().unwrap();

    assert!(reading.members.contains_key(&CrateName::from("alpha")));
}

#[test]
fn a_member_whose_binaries_directory_cannot_be_read_as_one_refuses_the_workspace() {
    let repository = workspace_holding_a_binary_and_a_library();
    repository.write("tools/alpha/src/bin", "not a directory at all\n");
    repository.commit("a file where the binaries directory belongs");
    repository.push();

    let error = workspace::read(repository.path()).unwrap_err();

    assert!(
        format!("{error:#}").contains("tools/alpha/src/bin"),
        "expected the message to name the directory it could not read, got: {error:#}"
    );
}

#[test]
fn a_directory_holding_no_clone_yet_reports_no_workspace_rather_than_failing() {
    let directory = tempfile::tempdir().unwrap();

    let reading = workspace::read(directory.path()).unwrap();

    assert_eq!(reading, None);
}
