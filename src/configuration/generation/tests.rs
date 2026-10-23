use super::*;

#[test]
fn a_generation_this_build_has_passed_is_met_by_it() {
    assert!(
        Generation::try_from("2")
            .unwrap()
            .is_met_by(BUILD_GENERATION)
    );
}

#[test]
fn the_generation_below_this_build_is_still_one_it_reads() {
    assert!(!OLDEST_READABLE_GENERATION.is_outgrown_by(OLDEST_READABLE_GENERATION));
}

#[test]
fn a_generation_further_back_than_one_is_outgrown() {
    assert!(
        OLDEST_READABLE_GENERATION
            .stepped_by(-1)
            .is_outgrown_by(OLDEST_READABLE_GENERATION)
    );
}

#[test]
fn the_oldest_readable_generation_is_the_one_below_this_build() {
    assert_eq!(
        OLDEST_READABLE_GENERATION,
        Generation(BUILD_GENERATION.0 - 1)
    );
}

#[test]
fn the_generation_this_build_is_is_met_by_it() {
    assert!(BUILD_GENERATION.is_met_by(BUILD_GENERATION));
}

#[test]
fn a_generation_beyond_this_build_is_not_met_by_it() {
    assert!(!BUILD_GENERATION.stepped_by(1).is_met_by(BUILD_GENERATION));
}

#[test]
fn a_version_that_is_not_a_whole_number_is_not_a_generation() {
    let refusal = Generation::try_from("0.1.0").unwrap_err();

    assert!(refusal.contains("0.1.0"), "{refusal}");
}

#[test]
fn a_version_carrying_a_sign_is_not_a_generation_the_published_schema_would_accept() {
    let refusal = Generation::try_from("+3").unwrap_err();

    assert!(refusal.contains("digits alone"), "{refusal}");
}

#[test]
fn a_version_above_the_largest_generation_is_refused_for_its_size_not_its_shape() {
    let refusal = Generation::try_from("4294967296").unwrap_err();

    assert!(refusal.contains("larger than"), "{refusal}");
}

#[test]
fn a_generation_is_written_the_way_it_is_read() {
    let stated = "3";

    let generation = Generation::try_from(stated).unwrap();

    assert_eq!(generation.to_string(), stated);
}

#[test]
fn a_generation_reaches_json_as_the_string_a_configuration_states() {
    let generation = Generation::try_from("3").unwrap();

    let written = serde_json::to_string(&generation).unwrap();

    assert_eq!(written, r#""3""#);
    assert_eq!(
        serde_json::from_str::<Generation>(&written).unwrap(),
        generation
    );
}
