#![allow(clippy::disallowed_macros)]

use {
    dotfiles_configurator::{
        configuration::CrateName,
        machine::{
            local::workspace,
            workspace_reading::{BinaryName, Revision},
        },
    },
    git2::{IndexAddOption, Repository, Signature},
    std::{collections::BTreeMap, fs, path::Path},
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

fn workspace_holding_a_binary_and_a_library() -> TemporaryRepository {
    let repository = TemporaryRepository::create();
    repository.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"tools/alpha\", \"tools/beta\"]\n",
    );
    repository.write("Cargo.lock", "version = 4\n");
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

fn installed(crate_name: &str, revision: &Revision) -> BTreeMap<CrateName, Revision> {
    BTreeMap::from([(CrateName::from(crate_name), revision.clone())])
}

fn installed_binaries(names: &[&str]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    for name in names {
        let file_name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        fs::write(directory.path().join(file_name), []).unwrap();
    }
    directory
}

fn alpha(reading: &dotfiles_configurator::machine::workspace_reading::WorkspaceReading) -> bool {
    let member = &reading.members[&CrateName::from("alpha")];
    member.installed.as_ref() == Some(&member.desired)
}

#[test]
fn the_desired_revision_is_the_commit_the_tracked_remote_branch_names() {
    let repository = workspace_holding_a_binary_and_a_library();

    let reading = workspace::read(
        repository.path(),
        &BTreeMap::new(),
        installed_binaries(&["alpha"]).path(),
    )
    .unwrap()
    .unwrap();

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

    let reading = workspace::read(
        repository.path(),
        &BTreeMap::new(),
        installed_binaries(&["alpha"]).path(),
    )
    .unwrap()
    .unwrap();

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

    let reading = workspace::read(
        repository.path(),
        &installed("alpha", &installed_from),
        installed_binaries(&["alpha"]).path(),
    )
    .unwrap()
    .unwrap();

    assert!(alpha(&reading));
}

#[test]
fn a_commit_changing_the_lockfile_drifts_a_crate_it_never_touched() {
    let repository = workspace_holding_a_binary_and_a_library();
    let installed_from = repository.head_revision();
    repository.write(
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"serde\"\n",
    );
    repository.commit("bump a dependency");
    repository.push();

    let reading = workspace::read(
        repository.path(),
        &installed("alpha", &installed_from),
        installed_binaries(&["alpha"]).path(),
    )
    .unwrap()
    .unwrap();

    assert!(!alpha(&reading));
}

#[test]
fn a_member_that_builds_no_binary_is_not_reported_as_a_member() {
    let repository = workspace_holding_a_binary_and_a_library();

    let reading = workspace::read(
        repository.path(),
        &BTreeMap::new(),
        installed_binaries(&["alpha"]).path(),
    )
    .unwrap()
    .unwrap();

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

    let error = workspace::read(
        repository.path(),
        &BTreeMap::new(),
        installed_binaries(&["alpha"]).path(),
    )
    .unwrap_err();

    assert!(
        format!("{error:#}").contains("tracks no remote branch"),
        "expected the message to say the branch tracks nothing, got: {error:#}"
    );
}

#[test]
fn the_abbreviated_commit_cargo_lists_still_finds_the_content_it_names() {
    let repository = workspace_holding_a_binary_and_a_library();
    let installed_from = repository.head_revision();
    let abbreviated = Revision::from(&installed_from.as_ref()[..9]);
    repository.write("README.md", "a change to something else entirely\n");
    repository.commit("unrelated");
    repository.push();

    let reading = workspace::read(
        repository.path(),
        &installed("alpha", &abbreviated),
        installed_binaries(&["alpha"]).path(),
    )
    .unwrap()
    .unwrap();

    assert!(alpha(&reading));
}

#[test]
fn a_crate_whose_installed_commit_is_absent_from_the_clone_reads_as_not_installed() {
    let repository = workspace_holding_a_binary_and_a_library();
    let absent = Revision::from("0123456789012345678901234567890123456789");

    let reading = workspace::read(
        repository.path(),
        &installed("alpha", &absent),
        installed_binaries(&["alpha"]).path(),
    )
    .unwrap()
    .unwrap();

    assert_eq!(reading.members[&CrateName::from("alpha")].installed, None);
}

fn absent_binaries_of(
    reading: &dotfiles_configurator::machine::workspace_reading::WorkspaceReading,
    crate_name: &str,
) -> Vec<String> {
    reading.members[&CrateName::from(crate_name)]
        .absent_binaries
        .iter()
        .map(BinaryName::to_string)
        .collect()
}

#[test]
fn a_member_whose_binary_is_where_cargo_installs_it_is_missing_nothing() {
    let repository = workspace_holding_a_binary_and_a_library();

    let reading = workspace::read(
        repository.path(),
        &BTreeMap::new(),
        installed_binaries(&["alpha"]).path(),
    )
    .unwrap()
    .unwrap();

    assert!(absent_binaries_of(&reading, "alpha").is_empty());
}

#[test]
fn a_member_whose_binary_is_not_where_cargo_installs_it_is_read_as_missing_it() {
    let repository = workspace_holding_a_binary_and_a_library();

    let reading = workspace::read(
        repository.path(),
        &BTreeMap::new(),
        installed_binaries(&[]).path(),
    )
    .unwrap()
    .unwrap();

    assert_eq!(absent_binaries_of(&reading, "alpha"), vec!["alpha"]);
}

#[test]
fn a_member_declaring_several_binaries_names_only_the_ones_that_are_gone() {
    let repository = TemporaryRepository::create();
    repository.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"tools/mining\"]\n",
    );
    repository.write("Cargo.lock", "version = 4\n");
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

    let reading = workspace::read(
        repository.path(),
        &BTreeMap::new(),
        installed_binaries(&["sweep"]).path(),
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        absent_binaries_of(&reading, "mining"),
        vec!["tool-use-statistics"]
    );
}

#[test]
fn a_directory_holding_no_clone_yet_reports_no_workspace_rather_than_failing() {
    let directory = tempfile::tempdir().unwrap();

    let reading = workspace::read(
        directory.path(),
        &BTreeMap::new(),
        installed_binaries(&["alpha"]).path(),
    )
    .unwrap();

    assert_eq!(reading, None);
}
