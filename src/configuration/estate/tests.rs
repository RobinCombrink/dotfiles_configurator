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

fn owner(spelled: &str) -> Owner {
    Owner::try_from(spelled).expect("an owner")
}

fn estate(named: &str) -> EstateName {
    EstateName::try_from(named).expect("an estate name")
}

fn declaring(name: &str, owners: &[&str]) -> Option<EstateDeclaration> {
    Some(EstateDeclaration {
        name: estate(name),
        owners: owners.iter().map(|spelled| owner(spelled)).collect(),
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

fn owners(names: &[&str]) -> BTreeSet<Owner> {
    names.iter().map(|name| owner(name)).collect()
}

#[test]
fn an_estate_holds_its_account_its_workspaces_owners_and_the_owners_it_names() {
    let mut work = configuration("Robin-Combrink", declaring("work", &["skynamo"]));
    work.workspaces.push(CargoWorkspace {
        repository: repository("RobinTools", "tools"),
    });

    let estates = resolved(&[work]).unwrap();

    assert_eq!(
        estates.owners_of(&estate("work")),
        Some(&owners(&["Robin-Combrink", "RobinTools", "skynamo"]))
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
        estates.owners_of(&estate("personal")),
        Some(&owners(&["RobinCombrink"]))
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

    assert_eq!(
        estates.owners_of(&estate("personal")).map(BTreeSet::len),
        Some(1)
    );
}

#[test]
fn an_estate_declared_by_two_configurations_is_refused() {
    let refusal = resolved(&[
        configuration("RobinCombrink", declaring("personal", &[])),
        configuration("Robin-Combrink", declaring("personal", &[])),
    ])
    .unwrap_err();

    let EstateConflict::DeclaredTwice { estate: twice, .. } = refusal else {
        panic!("expected the estate to be refused as declared twice, got: {refusal}");
    };
    assert_eq!(twice, estate("personal"));
}

#[test]
fn an_owner_in_two_estates_is_refused_whatever_its_capitalisation() {
    let refusal = resolved(&[
        configuration("RobinCombrink", declaring("personal", &[])),
        configuration("Robin-Combrink", declaring("work", &["robincombrink"])),
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
    let refusal =
        resolved(&[configuration("Robin Combrink", declaring("personal", &[]))]).unwrap_err();

    let EstateConflict::NotAnOwner { cause, .. } = refusal else {
        panic!("expected the account to be refused as no owner, got: {refusal}");
    };
    assert_eq!(cause, NotAnOwner("Robin Combrink".to_owned()));
}

#[test]
fn a_configuration_declaring_no_estate_may_act_as_any_account() {
    assert!(resolved(&[configuration("Robin Combrink", None)]).is_ok());
}
