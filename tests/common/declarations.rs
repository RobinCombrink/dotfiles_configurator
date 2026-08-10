// Each integration test file is its own crate and pulls this module in whole, so a helper only
// one of them needs reads as dead in the others.
#![allow(dead_code)]

use {
    dotfiles_configurator::{
        configuration::{
            ArchiveEntry, AssetPattern, BUILD_GENERATION, CargoWorkspace, Configuration, Context,
            GitHubAccount, GitHubRepository, Notice, ReleasedBinary, RepositoryName,
            RepositoryOwner, Resource, VersionWord,
        },
        desired_state::{DesiredState, ResolvedConfiguration, SourceLocation},
    },
    std::{
        num::NonZeroUsize,
        path::{Path, PathBuf},
    },
};

pub const REPOSITORIES_ROOT: &str = "/repositories";

pub const DOTFILES_FILES_ROOT: &str = "/repositories/Personal/dotfiles";

pub fn named_repository(owner_and_name: &str) -> GitHubRepository {
    let (owner, repository) = owner_and_name
        .split_once('/')
        .expect("a repository is written owner/name");
    GitHubRepository {
        owner: RepositoryOwner::from(owner),
        repository: RepositoryName::from(repository),
    }
}

pub fn dotfiles_repository() -> GitHubRepository {
    named_repository("Alice/dotfiles")
}

pub fn reporting_its_version_in_the_second_word(
    entry: &str,
    owner_and_name: &str,
) -> ReleasedBinary {
    ReleasedBinary {
        repository: named_repository(owner_and_name),
        asset: AssetPattern::EndsWith(".zip".to_owned()),
        entry: ArchiveEntry::try_from(entry.to_owned()).expect("an entry naming a file"),
        version_arguments: vec!["--version".to_owned()],
        version_word: VersionWord::from(NonZeroUsize::new(2).expect("a word position")),
    }
}

pub fn read_out_of_a_checkout(
    resources: Vec<Resource>,
    workspaces: Vec<CargoWorkspace>,
    notices: Vec<Notice>,
) -> DesiredState {
    read_from(
        SourceLocation::Checkout(PathBuf::from(DOTFILES_FILES_ROOT)),
        resources,
        workspaces,
        notices,
    )
}

pub fn read_out_of_the_dotfiles_repository(
    resources: Vec<Resource>,
    workspaces: Vec<CargoWorkspace>,
    notices: Vec<Notice>,
) -> DesiredState {
    read_from(
        SourceLocation::Repository(dotfiles_repository()),
        resources,
        workspaces,
        notices,
    )
}

pub fn declaring(resources: Vec<Resource>, workspaces: Vec<CargoWorkspace>) -> DesiredState {
    read_out_of_a_checkout(resources, workspaces, Vec::new())
}

fn read_from(
    location: SourceLocation,
    resources: Vec<Resource>,
    workspaces: Vec<CargoWorkspace>,
    notices: Vec<Notice>,
) -> DesiredState {
    let everywhere = Configuration {
        version: BUILD_GENERATION,
        applies_to: Context::Everywhere,
        github_account: GitHubAccount::from("Alice"),
        workspaces,
        resources,
        notices,
    };
    let personal = Configuration {
        version: BUILD_GENERATION,
        applies_to: Context::Personal,
        github_account: GitHubAccount::from("Alice"),
        workspaces: Vec::new(),
        resources: Vec::new(),
        notices: Vec::new(),
    };

    DesiredState::of(vec![
        (
            "everywhere.dotconfig.json".to_owned(),
            ResolvedConfiguration::read(everywhere, location.clone(), Path::new(REPOSITORIES_ROOT)),
        ),
        (
            "personal.dotconfig.json".to_owned(),
            ResolvedConfiguration::read(personal, location, Path::new(REPOSITORIES_ROOT)),
        ),
    ])
    .expect("a set holding one configuration for every machine and one for this class")
}
