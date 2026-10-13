use super::*;

fn owner(spelled: &str) -> Owner {
    Owner::try_from(spelled).expect("an owner")
}

fn estate(named: &str) -> EstateName {
    EstateName::try_from(named).expect("an estate name")
}

fn decoded(estates: &str) -> Result<Estates, serde_json::Error> {
    serde_json::from_str(estates)
}

const TWO_ESTATES: &str =
    r#"{ "personal": ["RobinCombrink"], "work": ["Robin-Combrink", "skynamo"] }"#;

#[test]
fn an_owner_belongs_to_the_estate_that_lists_it() {
    let estates = decoded(TWO_ESTATES).expect("declared estates");

    assert_eq!(estates.estate_of(&owner("skynamo")), Some(&estate("work")));
}

#[test]
fn an_owner_is_found_whatever_case_it_is_asked_about_in() {
    let estates = decoded(TWO_ESTATES).expect("declared estates");

    assert_eq!(
        estates.estate_of(&owner("robincombrink")),
        Some(&estate("personal"))
    );
}

#[test]
fn an_owner_no_estate_lists_belongs_to_none() {
    let estates = decoded(TWO_ESTATES).expect("declared estates");

    assert_eq!(estates.estate_of(&owner("flutter")), None);
}

#[test]
fn the_machines_owners_are_every_owner_of_every_estate_as_declared() {
    let estates = decoded(TWO_ESTATES).expect("declared estates");
    let spelled: Vec<&str> = estates.machines_owners().map(Owner::as_declared).collect();

    assert_eq!(spelled, ["RobinCombrink", "Robin-Combrink", "skynamo"]);
}

#[test]
fn estates_are_written_as_the_object_they_are_read_from() {
    let estates = decoded(TWO_ESTATES).expect("declared estates");

    assert_eq!(
        serde_json::to_value(&estates).expect("estates serialise"),
        serde_json::from_str::<serde_json::Value>(TWO_ESTATES).expect("estates as JSON")
    );
}

#[test]
fn estate_names_differing_only_in_case_are_different_estates() {
    assert_ne!(estate("Work"), estate("work"));
}

#[test]
fn an_owner_in_two_estates_is_refused_rather_than_given_to_either() {
    let refusal = Estates::default()
        .with(estate("personal"), [owner("RobinCombrink")])
        .expect("one estate")
        .with(estate("work"), [owner("robincombrink")])
        .expect_err("an owner in two estates");

    let EstatesRefused::OwnerInTwoEstates { first, second, .. } = &refusal else {
        panic!("an owner in two estates was reported as {refusal}");
    };
    assert_eq!((first, second), (&estate("personal"), &estate("work")));
}

#[test]
fn an_owner_listed_twice_in_one_estate_is_refused() {
    let refusal = Estates::default()
        .with(estate("work"), [owner("skynamo"), owner("Skynamo")])
        .expect_err("an owner listed twice");

    let EstatesRefused::OwnerListedTwice { .. } = &refusal else {
        panic!("an owner listed twice was reported as {refusal}");
    };
}

#[test]
fn an_estate_listed_twice_is_refused_rather_than_the_last_one_winning() {
    let refusal = decoded(r#"{ "work": ["skynamo"], "work": ["Robin-Combrink"] }"#)
        .expect_err("an estate listed twice");

    assert!(refusal.to_string().contains("listed twice"), "{refusal}");
}

#[test]
fn a_decoded_owner_in_two_estates_is_refused_naming_the_owner() {
    let refusal = decoded(r#"{ "personal": ["RobinCombrink"], "work": ["robincombrink"] }"#)
        .expect_err("an owner in two estates");

    assert!(refusal.to_string().contains("robincombrink"), "{refusal}");
}

#[test]
fn an_estate_whose_name_cannot_be_a_directory_is_refused() {
    let refusal =
        decoded(r#"{ "../work": ["skynamo"] }"#).expect_err("an estate name with a separator");

    assert!(refusal.to_string().contains("../work"), "{refusal}");
}

#[test]
fn an_estate_listing_something_that_is_not_an_owner_is_refused() {
    let refusal = decoded(r#"{ "work": ["sky namo"] }"#).expect_err("an owner with a space");

    assert!(refusal.to_string().contains("sky namo"), "{refusal}");
}

#[test]
fn estates_written_as_a_list_are_refused() {
    assert!(decoded(r#"["personal", "work"]"#).is_err());
}

#[test]
fn an_estate_name_holding_a_separator_is_refused() {
    assert!(EstateName::try_from("work/skynamo").is_err());
    assert!(EstateName::try_from("work\\skynamo").is_err());
}

#[test]
fn an_empty_estate_name_is_refused() {
    assert_eq!(EstateName::try_from(""), Err(NotAnEstateName::Unnamed));
}

#[test]
fn an_estate_named_for_a_windows_device_is_refused_in_any_case() {
    assert_eq!(
        EstateName::try_from("Com1"),
        Err(NotAnEstateName::DeviceName("Com1".to_owned()))
    );
}
