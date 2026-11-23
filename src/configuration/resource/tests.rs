use {
    super::*,
    crate::configuration::estate::OutsideTheRepository,
    github_owner::Owner,
    machine_manifest_document::{
        Account, EstateName, Estates, MachineClass, RepositoriesDirectory,
    },
    std::path::Path,
};

fn rooted(directory: &str) -> RepositoriesDirectory {
    RepositoriesDirectory::try_from(PathBuf::from(directory)).expect("a rooted directory")
}

fn repository_resource(fields: &str) -> Result<Resource, serde_json::Error> {
    serde_json::from_str(&format!(
        r#"{{ "kind": "repository", "owner": "flutter", "repository": "flutter"{fields} }}"#
    ))
}

#[test]
fn a_repository_declaring_a_depth_of_nothing_is_not_read() {
    assert!(repository_resource(r#", "depth": 0"#).is_err());
}

#[test]
fn a_repository_declaring_no_depth_is_written_back_without_one() {
    let written = serde_json::to_string(&repository_resource("").unwrap()).unwrap();

    assert!(!written.contains("depth"), "{written}");
}

fn registry_crate(fields: &str) -> Result<Resource, serde_json::Error> {
    serde_json::from_str(&format!(
        r#"{{ "kind": "package", "manager": "cargo", "crate_name": "cargo-mutants",
                  "source": {{ "source": "registry"{fields} }} }}"#
    ))
}

fn pinned_version(resource: Resource) -> PackageCurrency<CrateVersion> {
    let Resource::Package(Package::Cargo(CargoPackage {
        source: CargoSource::Registry { version },
        ..
    })) = resource
    else {
        panic!("expected a registry crate");
    };
    version
}

#[test]
fn a_registry_crate_declaring_no_version_is_kept_at_the_latest() {
    assert_eq!(
        pinned_version(registry_crate("").unwrap()),
        PackageCurrency::Latest
    );
}

#[test]
fn a_registry_crate_declaring_an_exact_version_is_pinned_to_it() {
    assert_eq!(
        pinned_version(registry_crate(r#", "version": "27.1.0""#).unwrap()),
        PackageCurrency::Exactly(CrateVersion::try_from("27.1.0").unwrap())
    );
}

#[test]
fn a_registry_crate_pinned_to_a_caret_requirement_is_not_read() {
    let refusal = registry_crate(r#", "version": "^27""#).unwrap_err();

    assert!(refusal.to_string().contains("^27"), "{refusal}");
}

#[test]
fn a_registry_crate_pinned_to_a_tilde_requirement_is_not_read() {
    assert!(registry_crate(r#", "version": "~1.2""#).is_err());
}

#[test]
fn a_registry_crate_pinned_to_a_lower_bound_is_not_read() {
    assert!(registry_crate(r#", "version": ">=1""#).is_err());
}

#[test]
fn a_registry_crate_declaring_no_version_is_written_back_without_one() {
    let written = serde_json::to_string(&registry_crate("").unwrap()).unwrap();

    assert!(!written.contains("version"), "{written}");
}

fn dotnet_tool(fields: &str) -> Result<Resource, serde_json::Error> {
    serde_json::from_str(&format!(
        r#"{{ "kind": "package", "manager": "dotnet_tool", "id": "dotnet-stryker"{fields} }}"#
    ))
}

#[test]
fn a_dotnet_tool_declaring_no_version_is_kept_at_the_latest() {
    assert_eq!(
        dotnet_tool("").unwrap(),
        Resource::Package(Package::DotnetTool(DotnetToolPackage {
            id: DotnetToolId::from("dotnet-stryker"),
            version: PackageCurrency::Latest,
        }))
    );
}

#[test]
fn a_dotnet_tool_declaring_an_exact_version_is_pinned_to_it() {
    assert_eq!(
        dotnet_tool(r#", "version": "5.0.0""#).unwrap(),
        Resource::Package(Package::DotnetTool(DotnetToolPackage {
            id: DotnetToolId::from("dotnet-stryker"),
            version: PackageCurrency::Exactly(DotnetToolVersion::try_from("5.0.0").unwrap()),
        }))
    );
}

#[test]
fn a_dotnet_tool_pinned_to_a_version_that_does_not_parse_is_not_read() {
    let refusal = dotnet_tool(r#", "version": "5.0""#).unwrap_err();

    assert!(refusal.to_string().contains("\"5.0\""), "{refusal}");
}

#[test]
fn an_archive_entry_is_known_by_a_name_carrying_no_platform_executable_suffix() {
    let entry = ArchiveEntry::try_from(format!("bin/rg{}", std::env::consts::EXE_SUFFIX)).unwrap();

    assert_eq!(entry.installed_name(), BinaryName::from("rg"));
}

#[test]
fn a_released_binary_installs_at_its_name_within_the_binaries_directory() {
    let installed = crate::currency::own_currency().path_within(Path::new("binaries"));

    assert_eq!(installed.parent(), Some(Path::new("binaries")));
    assert_eq!(
        installed.file_stem(),
        Some(std::ffi::OsStr::new("dotfiles_configurator"))
    );
}

fn rendered(manifest: MachineManifest) -> String {
    RenderedManifest::try_from(manifest)
        .expect("a manifest that serialises")
        .document()
        .to_owned()
}

#[test]
fn a_registered_manifest_is_written_out_as_the_document_it_renders() {
    let manifest = RenderedManifest::try_from(MachineManifest {
        repositories_directory_path: rooted("/repositories/Personal"),
        estates: Estates::default(),
        slots_per_bank: None,
        recorded_run: RecordedRun {
            class: MachineClass::Personal,
            configuration_sources: Vec::new(),
        },
    })
    .expect("a manifest that serialises");

    let written = serde_json::to_value(Registration::MachineManifest(manifest)).unwrap();

    assert_eq!(
        written,
        serde_json::json!({
            "registry": "machine_manifest",
            "machine": {
                "repositories_directory_path": "/repositories/Personal",
                "estates": {},
                "class": "personal",
                "configuration_sources": []
            }
        })
    );
}

#[test]
fn the_manifest_declares_the_repositories_directory_under_a_machine_of_its_own() {
    let document = rendered(MachineManifest {
        repositories_directory_path: rooted("/repositories/Personal"),
        estates: Estates::default(),
        slots_per_bank: None,
        recorded_run: RecordedRun {
            class: MachineClass::Personal,
            configuration_sources: Vec::new(),
        },
    });

    assert_eq!(
        document,
        "{\n  \"machine\": {\n    \"repositories_directory_path\": \"/repositories/Personal\",\n    \
             \"estates\": {},\n    \"class\": \"personal\",\n    \"configuration_sources\": []\n  }\n}"
    );
}

#[test]
fn the_manifest_names_each_estate_with_its_owners_and_account_as_they_were_spelled() {
    let owner = |spelled: &str| Owner::try_from(spelled).expect("an owner");
    let account = |spelled: &str| Account::try_from(spelled).expect("an account");
    let estates = Estates::default()
        .with(
            EstateName::try_from("personal").unwrap(),
            [owner("RobinCombrink")],
            account("RobinCombrink"),
        )
        .and_then(|estates| {
            estates.with(
                EstateName::try_from("work").unwrap(),
                [owner("skynamo"), owner("Robin-Combrink")],
                account("Robin-Combrink"),
            )
        })
        .expect("estates no owner is in twice");

    let document = rendered(MachineManifest {
        repositories_directory_path: rooted("/repositories/Work"),
        estates,
        slots_per_bank: None,
        recorded_run: RecordedRun {
            class: MachineClass::Work,
            configuration_sources: Vec::new(),
        },
    });

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&document).unwrap()["machine"]["estates"],
        serde_json::json!({
            "personal": { "owners": ["RobinCombrink"], "account": "RobinCombrink" },
            "work": { "owners": ["Robin-Combrink", "skynamo"], "account": "Robin-Combrink" }
        })
    );
}

#[test]
fn the_manifest_is_written_into_the_configuration_directory_under_the_home_directory() {
    assert_eq!(
        MachineManifest::path_within(Path::new("/home/alice")),
        Path::new("/home/alice/.dotconfig/machine.json")
    );
}

fn symlink_resource(source_fields: &str) -> Result<Resource, serde_json::Error> {
    let written: serde_json::Value = serde_json::from_str(&format!(
        r#"{{ "kind": "symlink", {source_fields}, "link_path": ".claude/skills/common" }}"#
    ))
    .expect("a test writes valid JSON");
    serde_json::from_value(written)
}

fn sourced_in_notes(path: &str) -> String {
    format!(
        r#""source": {{ "repository": {{ "owner": "Alice", "repository": "notes" }},
                        "path": "{path}" }}"#
    )
}

fn notes() -> GitHubRepository {
    GitHubRepository {
        owner: RepositoryOwner::from("Alice"),
        repository: RepositoryName::from("notes"),
    }
}

#[test]
fn a_symlink_sourced_in_a_repository_is_read_as_a_path_within_that_repository() {
    let resource = symlink_resource(&sourced_in_notes("plugins/common")).unwrap();

    assert_eq!(
        resource,
        Resource::Symlink(Symlink {
            source: LinkSource::DeclaredRepository(PathInRepository {
                repository: notes(),
                path: WithinTheRepository::try_from(PathBuf::from("plugins/common")).unwrap(),
            }),
            link_path: PathBuf::from(".claude/skills/common"),
        })
    );
}

#[test]
fn a_symlink_declaring_a_source_path_is_read_as_a_path_in_the_dotfiles_repository() {
    let resource = symlink_resource(r#""source_path": "claude/skills""#).unwrap();

    assert_eq!(
        resource,
        Resource::Symlink(Symlink {
            source: LinkSource::DotfilesRepository(PathBuf::from("claude/skills")),
            link_path: PathBuf::from(".claude/skills/common"),
        })
    );
}

#[test]
fn a_symlink_sourced_in_a_repository_at_a_rooted_path_is_refused() {
    let refusal = symlink_resource(&sourced_in_notes("/plugins/common")).unwrap_err();

    assert_eq!(
        refusal.to_string(),
        OutsideTheRepository::Rooted(PathBuf::from("/plugins/common")).to_string()
    );
}

#[test]
fn a_symlink_sourced_in_a_repository_at_a_path_climbing_out_of_it_is_refused() {
    let refusal = symlink_resource(&sourced_in_notes("plugins/../../etc")).unwrap_err();

    assert_eq!(
        refusal.to_string(),
        OutsideTheRepository::Climbing(PathBuf::from("plugins/../../etc")).to_string()
    );
}

#[test]
fn a_symlink_declaring_both_sources_is_refused() {
    let both = format!(
        r#""source_path": "claude/skills", {}"#,
        sourced_in_notes("plugins/common")
    );

    assert_eq!(
        symlink_resource(&both).unwrap_err().to_string(),
        UnsourcedLink::TwoSources(PathBuf::from(".claude/skills/common")).to_string()
    );
}

#[test]
fn a_symlink_declaring_no_source_is_refused() {
    assert_eq!(
        symlink_resource(r#""kind_of_source": "none""#)
            .unwrap_err()
            .to_string(),
        UnsourcedLink::NoSource(PathBuf::from(".claude/skills/common")).to_string()
    );
}

#[test]
fn a_symlink_is_written_back_with_the_one_source_it_declares() {
    let sourced_in_a_repository = symlink_resource(&sourced_in_notes("plugins/common")).unwrap();
    let in_the_dotfiles_repository = symlink_resource(r#""source_path": "claude/skills""#).unwrap();

    let written_in_a_repository = serde_json::to_value(&sourced_in_a_repository).unwrap();
    let written_in_the_dotfiles_repository =
        serde_json::to_value(&in_the_dotfiles_repository).unwrap();

    assert_eq!(
        written_in_a_repository,
        serde_json::json!({
            "kind": "symlink",
            "source": {
                "repository": { "owner": "Alice", "repository": "notes" },
                "path": "plugins/common"
            },
            "link_path": ".claude/skills/common"
        })
    );
    assert_eq!(
        written_in_the_dotfiles_repository,
        serde_json::json!({
            "kind": "symlink",
            "source_path": "claude/skills",
            "link_path": ".claude/skills/common"
        })
    );
}

#[test]
fn a_symlink_sourced_in_a_repository_shows_its_path_in_that_repository() {
    let resource = symlink_resource(&sourced_in_notes("plugins/common")).unwrap();

    let Resource::Symlink(symlink) = resource else {
        panic!("the declaration is a symlink");
    };
    assert_eq!(symlink.source.to_string(), "plugins/common in Alice/notes");
}

fn command_run_in(shell: Shell, presence_check: Option<PresenceCheck>) -> Resource {
    Resource::Command(Command {
        shell,
        args: vec!["refresh-completions".to_owned()],
        presence_check,
    })
}

fn output_check_in(shell: Shell) -> PresenceCheck {
    PresenceCheck::CommandOutputContains {
        shell,
        args: vec!["completions".to_owned(), "--status".to_owned()],
        contains: "ready".to_owned(),
    }
}

#[test]
fn a_command_run_in_power_shell_requires_pwsh() {
    assert_eq!(
        command_run_in(Shell::PowerShell, None).tool_requirements(),
        vec![Requirement::Tool(Tool::PowerShell)]
    );
}

#[test]
fn a_command_run_in_bash_requires_bash() {
    assert_eq!(
        command_run_in(Shell::Bash, None).tool_requirements(),
        vec![Requirement::Tool(Tool::Bash)]
    );
}

#[test]
fn a_command_run_in_wsl_requires_wsl() {
    assert_eq!(
        command_run_in(Shell::Wsl, None).tool_requirements(),
        vec![Requirement::Tool(Tool::Wsl)]
    );
}

#[test]
fn a_command_run_in_the_command_prompt_requires_no_tool() {
    assert_eq!(
        command_run_in(Shell::CommandPrompt, None).tool_requirements(),
        Vec::new()
    );
}

#[test]
fn a_command_checked_in_power_shell_requires_pwsh_for_its_check() {
    assert_eq!(
        command_run_in(
            Shell::CommandPrompt,
            Some(output_check_in(Shell::PowerShell))
        )
        .tool_requirements(),
        vec![Requirement::Tool(Tool::PowerShell)]
    );
}

#[test]
fn the_power_shell_requirement_names_pwsh_as_the_program_absent() {
    assert_eq!(
        Requirement::Tool(Tool::PowerShell).to_string(),
        "pwsh is not on the path"
    );
}
