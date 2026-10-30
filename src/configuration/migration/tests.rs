use {
    super::*,
    crate::configuration::{OLDEST_READABLE_GENERATION, estate::EstateOwners},
};

fn outgoing(estate: &str, resources: &str) -> OutgoingConfiguration {
    serde_json::from_str(&format!(
        r#"{{
            "version": "{OLDEST_READABLE_GENERATION}",
            "applies_to": "everywhere",
            "github_account": "Alice",
            "estate": {estate},
            "resources": {resources}
        }}"#
    ))
    .expect("a generation 11 document")
}

const ESTATE_LINK: &str = r#"{
    "kind": "symlink",
    "source_path": "estates/personal",
    "link_path": ".dotconfig/estates/personal"
}"#;

const GITCONFIG_LINK: &str = r#"{
    "kind": "symlink",
    "source_path": "gitconfig/.gitconfig",
    "link_path": ".gitconfig"
}"#;

fn estate(named: &str) -> EstateName {
    EstateName::try_from(named).expect("an estate name")
}

fn owner(spelled: &str) -> Owner {
    Owner::try_from(spelled).expect("an owner")
}

fn read_estate(keys: &str) -> ReadEstate {
    ReadEstate {
        name: estate("personal"),
        file: EstateFile::from("estates/personal/estate.json".to_owned()),
        owners: EstateOwners::decoded(&format!(r#"{{ "owners": {keys} }}"#)).expect("owners"),
    }
}

#[test]
fn an_estate_takes_its_folder_from_the_link_at_its_estate_directory() {
    let (configuration, _) = outgoing(
        r#"{ "name": "personal" }"#,
        &format!("[{GITCONFIG_LINK}, {ESTATE_LINK}]"),
    )
    .migrated()
    .unwrap();

    assert_eq!(
        configuration.estate,
        Some(EstateDeclaration {
            name: estate("personal"),
            folder: WithinTheRepository::try_from(PathBuf::from("estates/personal"))
                .expect("a folder within the repository"),
        })
    );
}

#[test]
fn the_link_an_estate_takes_its_folder_from_is_no_longer_declared_as_a_resource() {
    let (configuration, _) = outgoing(
        r#"{ "name": "personal" }"#,
        &format!("[{GITCONFIG_LINK}, {ESTATE_LINK}]"),
    )
    .migrated()
    .unwrap();

    assert_eq!(
        configuration.resources,
        vec![Resource::Symlink(Symlink {
            source_path: PathBuf::from("gitconfig/.gitconfig"),
            link_path: PathBuf::from(".gitconfig"),
        })]
    );
}

#[test]
fn an_estate_with_no_link_at_its_estate_directory_is_refused() {
    let refused = outgoing(r#"{ "name": "personal" }"#, &format!("[{GITCONFIG_LINK}]"))
        .migrated()
        .unwrap_err();

    assert_eq!(
        refused,
        MigrationRefused::NoEstateLink {
            estate: estate("personal"),
            link_path: PathBuf::from(".dotconfig/estates/personal"),
        }
    );
}

#[test]
fn an_estate_linked_from_a_folder_outside_the_repository_is_refused() {
    let refused = outgoing(
        r#"{ "name": "personal" }"#,
        r#"[{
            "kind": "symlink",
            "source_path": "../estates/personal",
            "link_path": ".dotconfig/estates/personal"
        }]"#,
    )
    .migrated()
    .unwrap_err();

    assert_eq!(
        refused,
        MigrationRefused::FolderOutsideTheRepository {
            estate: estate("personal"),
            cause: OutsideTheRepository::Climbing(PathBuf::from("../estates/personal")),
        }
    );
}

#[test]
fn a_configuration_declaring_no_estate_migrates_with_its_resources_whole() {
    let (configuration, _) = outgoing("null", &format!("[{ESTATE_LINK}]"))
        .migrated()
        .unwrap();

    assert_eq!(
        configuration.resources,
        vec![Resource::Symlink(Symlink {
            source_path: PathBuf::from("estates/personal"),
            link_path: PathBuf::from(".dotconfig/estates/personal"),
        })]
    );
}

#[test]
fn a_configuration_declaring_no_estate_completes_its_migration_from_the_generation_it_was_read_at()
{
    let (_, pending) = outgoing("null", &format!("[{ESTATE_LINK}]"))
        .migrated()
        .unwrap();

    assert_eq!(pending.completed(None), Ok(OLDEST_READABLE_GENERATION));
}

#[test]
fn every_owner_an_estate_named_being_a_key_of_its_estate_file_completes_the_migration() {
    let (_, pending) = outgoing(
        r#"{ "name": "personal", "owners": ["alicetools"] }"#,
        &format!("[{ESTATE_LINK}]"),
    )
    .migrated()
    .unwrap();

    assert_eq!(
        pending.completed(Some(&read_estate(r#"{ "Alice": {}, "AliceTools": {} }"#))),
        Ok(OLDEST_READABLE_GENERATION)
    );
}

#[test]
fn an_owner_an_estate_named_that_its_estate_file_does_not_key_is_refused_by_name() {
    let (_, pending) = outgoing(
        r#"{ "name": "personal", "owners": ["Alice", "Bob"] }"#,
        &format!("[{ESTATE_LINK}]"),
    )
    .migrated()
    .unwrap();

    assert_eq!(
        pending.completed(Some(&read_estate(r#"{ "Alice": {} }"#))),
        Err(MigrationRefused::OwnerMissingFromTheEstateFile {
            estate: estate("personal"),
            owner: owner("Bob"),
            file: EstateFile::from("estates/personal/estate.json".to_owned()),
        })
    );
}

#[test]
fn the_migrated_estate_names_no_owners_of_its_own() {
    let (configuration, _) = outgoing(
        r#"{ "name": "personal", "owners": ["Alice"] }"#,
        &format!("[{ESTATE_LINK}]"),
    )
    .migrated()
    .unwrap();

    let written = serde_json::to_value(&configuration).unwrap();

    assert_eq!(
        written["estate"],
        serde_json::json!({ "name": "personal", "folder": "estates/personal" })
    );
}
