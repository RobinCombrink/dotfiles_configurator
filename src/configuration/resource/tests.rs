use {
    super::*,
    github_owner::Owner,
    machine_manifest_document::{
        EstateName, Estates, MachineClass, RecordedSource, RepositoriesDirectory,
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
fn a_repository_declaring_no_depth_is_cloned_with_its_whole_history() {
    let Resource::Repository(clone) = repository_resource("").unwrap() else {
        panic!("expected a repository");
    };

    assert_eq!(clone.depth, None);
}

#[test]
fn a_repository_declaring_a_depth_is_cloned_holding_only_that_much_history() {
    let Resource::Repository(clone) = repository_resource(r#", "depth": 1"#).unwrap() else {
        panic!("expected a repository");
    };

    assert_eq!(
        clone.depth,
        Some(CloneDepth::from(NonZeroU32::new(1).unwrap()))
    );
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
fn the_manifest_records_the_class_and_the_sources_of_the_run_that_wrote_it_as_written() {
    let document = rendered(MachineManifest {
        repositories_directory_path: rooted("/repositories/Work"),
        estates: Estates::default(),
        slots_per_bank: None,
        recorded_run: RecordedRun {
            class: MachineClass::Work,
            configuration_sources: vec![
                RecordedSource::from("github:Alice/dotfiles/config".to_owned()),
                RecordedSource::from("local:/repositories/Work/dotfiles/config".to_owned()),
            ],
        },
    });

    let machine = &serde_json::from_str::<serde_json::Value>(&document).unwrap()["machine"];
    assert_eq!(
        (&machine["class"], &machine["configuration_sources"]),
        (
            &serde_json::json!("work"),
            &serde_json::json!([
                "github:Alice/dotfiles/config",
                "local:/repositories/Work/dotfiles/config"
            ])
        )
    );
}

#[test]
fn the_manifest_names_each_estate_with_its_owners_as_they_were_spelled() {
    let owner = |spelled: &str| Owner::try_from(spelled).expect("an owner");
    let estates = Estates::default()
        .with(
            EstateName::try_from("personal").unwrap(),
            [owner("RobinCombrink")],
        )
        .and_then(|estates| {
            estates.with(
                EstateName::try_from("work").unwrap(),
                [owner("skynamo"), owner("Robin-Combrink")],
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
            "personal": ["RobinCombrink"],
            "work": ["Robin-Combrink", "skynamo"]
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
