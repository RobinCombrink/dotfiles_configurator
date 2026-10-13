use {
    super::*,
    crate::configuration::{
        BUILD_GENERATION, CargoWorkspace, Context, GitHubRepository, RepositoryName, Resource,
    },
};

fn configuration(account: &str, estate: Option<EstateDeclaration>) -> Configuration {
    Configuration {
        version: BUILD_GENERATION,
        applies_to: Context::Everywhere,
        github_account: GitHubAccount::from(account),
        estate,
        workspaces: Vec::new(),
        resources: Vec::new(),
        notices: Vec::new(),
    }
}

fn declaring(name: &str, owners: &[&str]) -> Option<EstateDeclaration> {
    Some(EstateDeclaration {
        name: EstateName::try_from(name).unwrap(),
        owners: owners
            .iter()
            .map(|owner| EstateOwner::from(*owner))
            .collect(),
    })
}

fn repository(owner: &str, name: &str) -> GitHubRepository {
    GitHubRepository {
        owner: RepositoryOwner::from(owner),
        repository: RepositoryName::from(name),
    }
}

fn resolved(configurations: &[Configuration]) -> Result<Estates, EstateConflict> {
    let names: Vec<ConfigurationName> = (0..configurations.len())
        .map(|position| ConfigurationName::from(format!("{position}.dotconfig.json")))
        .collect();
    resolve_estates(names.iter().zip(configurations))
}

fn owners(names: &[&str]) -> BTreeSet<EstateOwner> {
    names.iter().map(|name| EstateOwner::from(*name)).collect()
}

#[test]
fn an_estate_holds_its_account_its_workspaces_owners_and_the_owners_it_names() {
    let mut work = configuration("Robin-Combrink", declaring("work", &["skynamo"]));
    work.workspaces.push(CargoWorkspace {
        repository: repository("RobinTools", "tools"),
    });

    let estates = resolved(&[work]).unwrap();

    assert_eq!(
        estates[&EstateName::try_from("work").unwrap()],
        owners(&["Robin-Combrink", "RobinTools", "skynamo"])
    );
}

#[test]
fn the_owner_of_a_repository_an_estate_only_clones_is_not_one_of_its_owners() {
    let mut personal = configuration("RobinCombrink", declaring("personal", &[]));
    personal.resources.push(Resource::Repository(
        repository("flutter", "flutter").into(),
    ));

    let estates = resolved(&[personal]).unwrap();

    assert_eq!(
        estates[&EstateName::try_from("personal").unwrap()],
        owners(&["RobinCombrink"])
    );
}

#[test]
fn a_configuration_declaring_no_estate_places_no_owner_anywhere() {
    assert!(
        resolved(&[configuration("RobinCombrink", None)])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn one_owner_named_twice_within_an_estate_is_one_owner() {
    let personal = configuration("RobinCombrink", declaring("personal", &["robincombrink"]));

    let estates = resolved(&[personal]).unwrap();

    assert_eq!(estates[&EstateName::try_from("personal").unwrap()].len(), 1);
}

#[test]
fn an_estate_declared_by_two_configurations_is_refused() {
    let refusal = resolved(&[
        configuration("RobinCombrink", declaring("personal", &[])),
        configuration("Robin-Combrink", declaring("personal", &[])),
    ])
    .unwrap_err();

    let EstateConflict::DeclaredTwice { estate, .. } = refusal else {
        panic!("expected the estate to be refused as declared twice, got: {refusal}");
    };
    assert_eq!(estate, EstateName::try_from("personal").unwrap());
}

#[test]
fn an_owner_in_two_estates_is_refused_whatever_its_capitalisation() {
    let refusal = resolved(&[
        configuration("RobinCombrink", declaring("personal", &[])),
        configuration("Robin-Combrink", declaring("work", &["robincombrink"])),
    ])
    .unwrap_err();

    let EstateConflict::OwnerInTwoEstates { owner, .. } = refusal else {
        panic!("expected the owner to be refused as in two estates, got: {refusal}");
    };
    assert_eq!(owner, EstateOwner::from("RobinCombrink"));
}

#[test]
fn an_estate_name_holding_a_separator_is_refused() {
    assert!(EstateName::try_from("work/skynamo").is_err());
    assert!(EstateName::try_from("work\\skynamo").is_err());
}

#[test]
fn an_empty_estate_name_is_refused() {
    assert!(EstateName::try_from("").is_err());
}

#[test]
fn an_estate_named_for_a_windows_device_is_refused_in_any_case() {
    assert!(EstateName::try_from("Com1").is_err());
}
