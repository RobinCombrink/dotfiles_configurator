use serde_json::json;

use super::*;

fn decoded(value: serde_json::Value) -> Result<SlotsPerBank, serde_json::Error> {
    serde_json::from_value(value)
}

#[test]
fn two_and_three_slots_decode_as_the_count_they_name() {
    assert_eq!(decoded(json!(2)).expect("two slots"), SlotsPerBank::Two);
    assert_eq!(decoded(json!(3)).expect("three slots"), SlotsPerBank::Three);
}

#[test]
fn a_count_other_than_two_or_three_is_refused_rather_than_defaulted() {
    for unsupported in [
        json!(0),
        json!(1),
        json!(4),
        json!(6),
        json!(-2),
        json!(2.5),
    ] {
        let refusal = decoded(unsupported.clone()).expect_err("an unsupported count");

        assert!(refusal.is_data(), "{unsupported}: {refusal}");
    }
}

#[test]
fn a_count_written_as_text_is_refused() {
    let refusal = decoded(json!("2")).expect_err("a count written as a string");

    assert!(refusal.is_data(), "{refusal}");
}

#[test]
fn every_slots_per_bank_is_written_as_the_count_it_reads_back_as() {
    for slots in SlotsPerBank::iter() {
        let written = serde_json::to_value(slots).expect("a count serialises");

        assert_eq!(written, json!(slots.count()));
        assert_eq!(decoded(written).expect("a written count"), slots);
    }
}

#[test]
fn an_unsupported_count_names_itself_in_its_refusal() {
    assert_eq!(SlotsPerBank::try_from(5), Err(UnsupportedSlotsPerBank(5)));
}
