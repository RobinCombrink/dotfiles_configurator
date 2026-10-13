use std::collections::HashSet;

use super::*;

fn owner(spelled: &str) -> Owner {
    Owner::try_from(spelled).expect("an owner")
}

#[test]
fn the_owner_pattern_admits_exactly_the_characters_an_owner_admits() {
    let pattern = regex::Regex::new(&format!("^{OWNER_PATTERN}$")).expect("a pattern");
    for character in (0u8..=127).map(char::from).chain(['ö', 'é']) {
        let spelled = character.to_string();
        assert_eq!(
            pattern.is_match(&spelled),
            Owner::try_from(spelled.as_str()).is_ok(),
            "the owner pattern and the owner disagree on {character:?}"
        );
    }
}

#[test]
fn owners_differing_only_in_case_are_the_same_owner() {
    assert_eq!(owner("Skynamo"), owner("skynamo"));
}

#[test]
fn owners_spelled_differently_are_different_owners() {
    assert_ne!(owner("skynamo"), owner("skynamo-labs"));
}

#[test]
fn owners_differing_only_in_case_order_as_one_owner() {
    assert_eq!(owner("Skynamo").cmp(&owner("skynamo")), Ordering::Equal);
}

#[test]
fn owners_differing_only_in_case_hash_as_one_owner() {
    let owners: HashSet<Owner> = [owner("Skynamo"), owner("skynamo")].into();

    assert_eq!(owners.len(), 1);
}

#[test]
fn owners_order_alphabetically_whatever_their_case() {
    assert!(owner("alpha") < owner("Beta"));
}

#[test]
fn an_owner_keeps_the_spelling_it_was_declared_in() {
    assert_eq!(owner("Skynamo").to_string(), "Skynamo");
}

#[test]
fn an_owner_spelled_with_letters_digits_and_hyphens_is_accepted() {
    assert_eq!(owner("Robin-Combrink2").as_declared(), "Robin-Combrink2");
}

#[test]
fn an_owner_spelled_with_anything_but_letters_digits_and_hyphens_is_refused() {
    for spelled in [
        "",
        "robin combrink",
        "RobinCombrink/dotfiles",
        "rob_in",
        "robin.combrink",
    ] {
        assert_eq!(
            Owner::try_from(spelled).err(),
            Some(NotAnOwner(spelled.to_owned())),
            "{spelled:?} was accepted as an owner"
        );
    }
}

#[test]
fn an_owner_is_written_as_it_was_declared() {
    assert_eq!(
        serde_json::to_string(&owner("Skynamo")).expect("an owner serialises"),
        r#""Skynamo""#
    );
}

#[test]
fn a_decoded_owner_is_refused_when_github_would_not_admit_it() {
    let refusal = serde_json::from_str::<Owner>(r#""rob_in""#).expect_err("not an owner");

    assert!(refusal.to_string().contains("rob_in"), "{refusal}");
}
