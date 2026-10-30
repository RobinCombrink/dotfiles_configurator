use {
    super::*,
    crate::configuration::{
        BUILD_GENERATION, CargoWorkspace, Context, GitHubRepository, RepositoryName,
    },
};

fn configuration(account: &str) -> Configuration {
    Configuration {
        version: BUILD_GENERATION,
        applies_to: Context::Everywhere,
        github_account: GitHubAccount::from(account),
        estate: None,
        slots_per_bank: None,
        workspaces: Vec::new(),
        resources: Vec::new(),
        notices: Vec::new(),
    }
}

fn building(account: &str, workspace_owner: &str) -> Configuration {
    let mut configuration = configuration(account);
    configuration.workspaces.push(CargoWorkspace {
        repository: repository(workspace_owner, "tools"),
    });
    configuration
}

fn owner(spelled: &str) -> Owner {
    Owner::try_from(spelled).expect("an owner")
}

fn estate(named: &str) -> EstateName {
    EstateName::try_from(named).expect("an estate name")
}

fn estate_file(named: &str) -> EstateFile {
    EstateFile::from(format!("estates/{named}/estate.json"))
}

fn keying(named: &str, keys: &[&str]) -> Option<ReadEstate> {
    Some(ReadEstate {
        name: estate(named),
        file: estate_file(named),
        owners: EstateOwners(owners(keys)),
    })
}

fn repository(owner: &str, name: &str) -> GitHubRepository {
    GitHubRepository {
        owner: RepositoryOwner::from(owner),
        repository: RepositoryName::from(name),
    }
}

fn configuration_name(position: usize) -> ConfigurationName {
    ConfigurationName::from(format!("{position}.dotconfig.json"))
}

fn resolved(declared: &[(Configuration, Option<ReadEstate>)]) -> Result<Estates, EstateConflict> {
    let names: Vec<ConfigurationName> = (0..declared.len()).map(configuration_name).collect();
    resolve_estates(
        names
            .iter()
            .zip(declared)
            .map(|(name, (configuration, estate))| (name, configuration, estate.as_ref())),
    )
}

fn owners(names: &[&str]) -> BTreeSet<Owner> {
    names.iter().map(|name| owner(name)).collect()
}

#[test]
fn an_estates_owners_are_the_keys_its_estate_file_holds() {
    let estates = resolved(&[(
        building("Alice", "AliceTools"),
        keying("personal", &["Alice", "AliceTools", "Bob"]),
    )])
    .unwrap();

    assert_eq!(
        estates.owners_of(&estate("personal")),
        Some(&owners(&["Alice", "AliceTools", "Bob"]))
    );
}

#[test]
fn an_estate_acts_as_the_account_of_the_configuration_declaring_it() {
    let estates = resolved(&[(
        configuration("Alice"),
        keying("personal", &["Alice", "Bob"]),
    )])
    .unwrap();

    assert_eq!(
        estates.account_of(&owner("Bob")).map(Account::as_declared),
        Some("Alice")
    );
}

#[test]
fn an_account_keyed_in_another_capitalisation_is_one_of_its_estates_owners() {
    let estates = resolved(&[(configuration("alice"), keying("personal", &["Alice"]))]).unwrap();

    assert_eq!(
        estates.owners_of(&estate("personal")),
        Some(&owners(&["Alice"]))
    );
}

#[test]
fn a_configuration_declaring_no_estate_places_no_owner_anywhere() {
    assert!(
        resolved(&[(configuration("RobinCombrink"), None)])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_configuration_declaring_no_estate_is_not_checked_for_its_owners() {
    assert!(
        resolved(&[(building("Robin Combrink", "Robin Tools"), None)])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_estate_declared_by_two_configurations_is_refused() {
    let refusal = resolved(&[
        (configuration("Alice"), keying("personal", &["Alice"])),
        (configuration("Alice"), keying("personal", &["Alice"])),
    ])
    .unwrap_err();

    assert_eq!(
        refusal,
        EstateConflict::DeclaredTwice {
            estate: estate("personal"),
            first: configuration_name(0),
            second: configuration_name(1),
        }
    );
}

#[test]
fn an_owner_in_two_estates_is_refused_whatever_its_capitalisation() {
    let refusal = resolved(&[
        (
            configuration("RobinCombrink"),
            keying("personal", &["RobinCombrink"]),
        ),
        (
            configuration("Robin-Combrink"),
            keying("work", &["Robin-Combrink", "robincombrink"]),
        ),
    ])
    .unwrap_err();

    let EstateConflict::Refused(EstatesRefused::OwnerInTwoEstates { owner: twice, .. }) = refusal
    else {
        panic!("expected the owner to be refused as in two estates, got: {refusal}");
    };
    assert_eq!(twice, owner("RobinCombrink"));
}

#[test]
fn an_estate_acting_as_an_account_github_would_not_name_is_refused() {
    let refusal = resolved(&[(
        configuration("Robin Combrink"),
        keying("personal", &["RobinCombrink"]),
    )])
    .unwrap_err();

    let EstateConflict::NotAnOwner { cause, .. } = refusal else {
        panic!("expected the account to be refused as no owner, got: {refusal}");
    };
    assert_eq!(cause, NotAnOwner("Robin Combrink".to_owned()));
}

#[test]
fn an_estate_links_its_folder_at_its_estate_directory() {
    let declaration = EstateDeclaration {
        name: estate("personal"),
        folder: within("estates/personal"),
    };

    assert_eq!(
        declaration.link(),
        Symlink {
            source_path: PathBuf::from("estates/personal"),
            link_path: Path::new(".dotconfig").join("estates").join("personal"),
        }
    );
}

fn within(folder: &str) -> WithinTheRepository {
    WithinTheRepository::try_from(PathBuf::from(folder)).expect("a folder within the repository")
}

fn declared(folder: &str) -> Result<EstateDeclaration, serde_json::Error> {
    serde_json::from_value(serde_json::json!({ "name": "personal", "folder": folder }))
}

#[test]
fn an_estate_folder_within_the_repository_is_read_as_declared() {
    assert_eq!(
        declared("estates/personal").unwrap().folder,
        within("estates/personal")
    );
}

#[test]
fn an_estate_folder_climbing_out_of_the_repository_is_refused_when_the_configuration_is_read() {
    assert_eq!(
        declared("estates/../../personal").unwrap_err().to_string(),
        OutsideTheRepository::Climbing(PathBuf::from("estates/../../personal")).to_string()
    );
}

#[test]
fn an_estate_folder_rooted_outside_the_repository_is_refused_when_the_configuration_is_read() {
    assert_eq!(
        declared("/estates/personal").unwrap_err().to_string(),
        OutsideTheRepository::Rooted(PathBuf::from("/estates/personal")).to_string()
    );
}

#[test]
fn an_estate_file_within_the_repository_is_named_with_forward_slashes_whatever_the_platform() {
    let folder = WithinTheRepository::try_from(Path::new("estates").join("personal")).unwrap();

    assert_eq!(
        folder.join(ESTATE_FILE).to_string(),
        "estates/personal/estate.json"
    );
}

#[test]
fn an_estate_files_owners_are_the_keys_of_its_owners_object_whatever_their_values_hold() {
    let decoded = EstateOwners::decoded(
        r#"{
            "$schema": "./estate.schema.json",
            "owners": {
                "Alice": { "tracker": { "github": {} }, "labels": { "ready": "kind/apply" } },
                "AliceTools": 7
            },
            "anything": ["else"]
        }"#,
    )
    .unwrap();

    assert_eq!(decoded.owners(), &owners(&["Alice", "AliceTools"]));
}

#[test]
fn an_estate_file_with_an_empty_owners_object_names_no_owner() {
    assert!(
        EstateOwners::decoded(r#"{ "owners": {} }"#)
            .unwrap()
            .owners()
            .is_empty()
    );
}

#[test]
fn an_estate_file_without_an_owners_object_is_refused() {
    let fault = EstateOwners::decoded(r#"{ "labels": {} }"#).unwrap_err();

    let EstateFileFault::NoOwnersObject = fault else {
        panic!("expected the file to be refused as holding no owners object, got: {fault:?}");
    };
}

#[test]
fn an_estate_file_whose_owners_are_a_list_rather_than_an_object_is_refused() {
    let fault = EstateOwners::decoded(r#"{ "owners": ["Alice"] }"#).unwrap_err();

    let EstateFileFault::NoOwnersObject = fault else {
        panic!("expected the file to be refused as holding no owners object, got: {fault:?}");
    };
}

#[test]
fn an_estate_file_that_is_not_json_is_refused() {
    let fault = EstateOwners::decoded("owners: Alice").unwrap_err();

    let EstateFileFault::NotJson(_) = fault else {
        panic!("expected the file to be refused as not JSON, got: {fault:?}");
    };
}
