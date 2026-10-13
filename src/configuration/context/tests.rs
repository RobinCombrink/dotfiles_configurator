use {super::*, strum::IntoEnumIterator};

#[test]
fn a_configuration_for_every_machine_applies_to_a_machine_of_any_class() {
    assert!(Context::Everywhere.applies_on(MachineClass::Personal));
    assert!(Context::Everywhere.applies_on(MachineClass::Work));
}

#[test]
fn a_configuration_for_one_class_of_machine_applies_to_no_other_class() {
    assert!(!Context::Personal.applies_on(MachineClass::Work));
    assert!(!Context::Work.applies_on(MachineClass::Personal));
}

#[test]
fn a_configuration_for_one_class_of_machine_applies_to_that_class() {
    assert!(Context::Personal.applies_on(MachineClass::Personal));
    assert!(Context::Work.applies_on(MachineClass::Work));
}

#[test]
fn every_context_reaches_json_spelled_the_way_a_configuration_writes_it() {
    for context in Context::iter() {
        assert_eq!(
            serde_json::to_string(&context).unwrap(),
            format!("\"{context}\"")
        );
    }
}
