use super::*;

#[test]
fn a_machine_this_tool_does_not_know_is_rejected_with_the_ones_it_does() {
    let error = MachineClass::from_str("laptop").unwrap_err();

    assert!(
        error.contains("personal") && error.contains("work"),
        "{error}"
    );
}

#[test]
fn no_invocation_can_name_the_context_a_configuration_writes_for_every_machine() {
    assert!(MachineClass::from_str("everywhere").is_err());
}

#[test]
fn every_machine_class_is_written_the_way_it_is_read() {
    for machine in MachineClass::iter() {
        assert_eq!(MachineClass::from_str(&machine.to_string()), Ok(machine));
    }
}

#[test]
fn every_machine_class_reaches_json_spelled_the_way_it_is_written() {
    for machine in MachineClass::iter() {
        assert_eq!(
            serde_json::to_string(&machine).unwrap(),
            format!("\"{machine}\"")
        );
    }
}
