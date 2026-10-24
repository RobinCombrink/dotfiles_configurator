use super::*;

#[test]
fn a_configuration_the_newest_build_cannot_read_either_names_the_build_it_replaced() {
    let message = still_unreadable_by_the_newest_build(
        &"personal.dotconfig.json needs generation 99",
        &Version::try_from("3.18.0").unwrap(),
    );

    assert!(message.contains("replaced 3.18.0"), "{message}");
}
