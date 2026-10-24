use super::*;

#[test]
fn a_child_exiting_zero_succeeded() {
    assert!(Exited::Code(0).succeeded());
}

#[test]
fn a_child_exiting_with_any_other_code_did_not_succeed() {
    assert!(!Exited::Code(1).succeeded());
}

#[test]
fn a_child_ending_without_a_code_did_not_succeed() {
    assert!(!Exited::WithoutCode.succeeded());
}

#[test]
fn a_code_without_its_high_bit_set_is_written_in_decimal() {
    assert_eq!(Exited::Code(1603).to_string(), "exited with 1603");
}

#[test]
fn a_code_with_its_high_bit_set_is_written_in_hex_as_windows_documents_it() {
    assert_eq!(
        Exited::Code(0x8A15_0014_u32.cast_signed()).to_string(),
        "exited with 0x8A150014"
    );
}

#[test]
fn a_child_ending_without_a_code_is_written_without_one() {
    assert_eq!(Exited::WithoutCode.to_string(), "exited without a code");
}
