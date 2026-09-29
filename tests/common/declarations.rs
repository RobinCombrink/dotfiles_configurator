// Each integration test file is its own crate and pulls this module in whole, so a helper only
// one of them needs reads as dead in the others.
#![allow(dead_code)]

use {
    crate::fake_machine::{dotfiles_repository_path, home_directory_path, repositories_root_path},
    dotfiles_configurator::{
        configuration::{
            ArchiveEntry, AssetPattern, BUILD_GENERATION, CargoWorkspace, Configuration,
            ConfigurationName, Context, DeclaredNotice, Estates, GitHubAccount, GitHubRepository,
            MachineClass, MachineManifest, RecordedRun, RecordedSource, ReleasedBinary,
            RepositoryName, RepositoryOwner, Resource, VersionWord,
        },
        configuration_source::AbsoluteDirectory,
        desired_state::{DesiredState, ResolvedConfiguration, SourceLocation},
        reporting::{RunKind, RunReport},
    },
    std::num::NonZeroUsize,
    tempfile::TempDir,
};

pub struct Reporting {
    _logs: TempDir,
    report: RunReport,
}

impl Reporting {
    pub fn opening(kind: RunKind) -> Self {
        let logs = tempfile::tempdir().expect("a directory to write run logs into");
        let report = RunReport::open_in(logs.path(), kind).expect("a run log");
        Self {
            _logs: logs,
            report,
        }
    }

    pub fn report(&self) -> &RunReport {
        &self.report
    }
}

pub const RECORDED_SOURCE: &str = "github:Alice/dotfiles/config";

pub fn manifest_for(machine: MachineClass) -> MachineManifest {
    MachineManifest {
        repositories_directory_path: repositories_root_path().join(machine.repositories_leaf()),
        estates: Estates::new(),
        recorded_run: RecordedRun {
            class: machine,
            configuration_sources: vec![RecordedSource::from(RECORDED_SOURCE.to_owned())],
        },
    }
}

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

fn dotfiles_checkout() -> AbsoluteDirectory {
    AbsoluteDirectory::of(dotfiles_repository_path())
        .expect("the fake machine roots its repositories")
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
    notices: Vec<DeclaredNotice>,
) -> DesiredState {
    read_from(
        SourceLocation::Checkout(dotfiles_checkout()),
        resources,
        workspaces,
        notices,
    )
}

pub fn read_out_of_the_dotfiles_repository(
    resources: Vec<Resource>,
    workspaces: Vec<CargoWorkspace>,
    notices: Vec<DeclaredNotice>,
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

fn employers_repository() -> GitHubRepository {
    named_repository("Employer/dotfiles")
}

pub fn read_as_two_accounts(
    alices_resources: Vec<Resource>,
    employers_resources: Vec<Resource>,
) -> DesiredState {
    let everywhere = Configuration {
        version: BUILD_GENERATION,
        applies_to: Context::Everywhere,
        github_account: GitHubAccount::from("Alice"),
        estate: None,
        workspaces: Vec::new(),
        resources: alices_resources,
        notices: Vec::new(),
    };
    let work = Configuration {
        version: BUILD_GENERATION,
        applies_to: Context::Work,
        github_account: GitHubAccount::from(employers_repository().owner.as_ref()),
        estate: None,
        workspaces: Vec::new(),
        resources: employers_resources,
        notices: Vec::new(),
    };

    DesiredState::of(
        vec![
            ResolvedConfiguration::read(
                ConfigurationName::from("everywhere.dotconfig.json"),
                everywhere,
                SourceLocation::Checkout(dotfiles_checkout()),
                &repositories_root_path(),
            ),
            ResolvedConfiguration::read(
                ConfigurationName::from("work.dotconfig.json"),
                work,
                SourceLocation::Repository(employers_repository()),
                &repositories_root_path(),
            ),
        ],
        manifest_for(MachineClass::Work),
        &home_directory_path(),
    )
    .expect("a set holding one configuration for every machine and one for this class")
}

fn read_from(
    location: SourceLocation,
    resources: Vec<Resource>,
    workspaces: Vec<CargoWorkspace>,
    notices: Vec<DeclaredNotice>,
) -> DesiredState {
    let everywhere = Configuration {
        version: BUILD_GENERATION,
        applies_to: Context::Everywhere,
        github_account: GitHubAccount::from("Alice"),
        estate: None,
        workspaces,
        resources,
        notices,
    };
    let personal = Configuration {
        version: BUILD_GENERATION,
        applies_to: Context::Personal,
        github_account: GitHubAccount::from("Alice"),
        estate: None,
        workspaces: Vec::new(),
        resources: Vec::new(),
        notices: Vec::new(),
    };

    DesiredState::of(
        vec![
            ResolvedConfiguration::read(
                ConfigurationName::from("everywhere.dotconfig.json"),
                everywhere,
                location.clone(),
                &repositories_root_path(),
            ),
            ResolvedConfiguration::read(
                ConfigurationName::from("personal.dotconfig.json"),
                personal,
                location,
                &repositories_root_path(),
            ),
        ],
        manifest_for(MachineClass::Personal),
        &home_directory_path(),
    )
    .expect("a set holding one configuration for every machine and one for this class")
}
