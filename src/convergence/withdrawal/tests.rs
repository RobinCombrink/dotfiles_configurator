use super::*;

const EARLIER: &str = "0c532c68bd883d1e6b002ad5bb1f40aa2ed70f66";
const LATER: &str = "e54928664eb4222b2ea79c7ef56126c5c58c7e01";

fn file(binary: &str) -> String {
    BinaryName::from(binary).file_name()
}

fn listed(header: &str, binaries: &[&str]) -> String {
    let mut lines = format!("{header}\n");
    for binary in binaries {
        lines.push_str(&format!("    {}\n", file(binary)));
    }
    lines
}

fn listing_of_two_disjoint_records_of_one_member() -> String {
    [
        listed("committed v1.1.11:", &["committed"]),
        listed(
            &format!(
                "session-mining v0.1.0 \
                 (https://Alice@github.com/Alice/dotfiles?rev={EARLIER}#0c532c68):"
            ),
            &["session-census", "reach"],
        ),
        listed(
            &format!(
                "session-mining v0.1.0 \
                 (https://Alice@github.com/Alice/dotfiles?rev={LATER}#e5492866):"
            ),
            &["sweep", "tool-use-statistics"],
        ),
        listed(
            "stop-gate v0.1.0 (C:\\Repositories\\Alice\\dotfiles\\tools\\stop-gate):",
            &["stop-gate"],
        ),
    ]
    .concat()
}

#[test]
fn only_git_records_are_read_each_with_the_binaries_listed_beneath_it() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());

    let read: Vec<(String, Vec<String>)> = records
        .iter()
        .map(|record| (record.crate_name.to_string(), record.binary_files.clone()))
        .collect();
    assert_eq!(
        read,
        vec![
            (
                "session-mining".to_owned(),
                vec![file("session-census"), file("reach")]
            ),
            (
                "session-mining".to_owned(),
                vec![file("sweep"), file("tool-use-statistics")]
            ),
        ]
    );
}

#[test]
fn a_record_is_specified_by_its_whole_source_so_that_two_of_one_package_are_told_apart() {
    let records = install_records(&listing_of_two_disjoint_records_of_one_member());

    assert_eq!(
        records[0].specification(),
        format!("git+https://Alice@github.com/Alice/dotfiles?rev={EARLIER}#session-mining@0.1.0")
    );
}

#[test]
fn every_file_any_record_lists_is_claimed_whatever_the_record_was_installed_from() {
    let claimed = claimed_binary_files(&listing_of_two_disjoint_records_of_one_member());

    assert_eq!(
        claimed,
        BTreeSet::from([
            file("committed"),
            file("session-census"),
            file("reach"),
            file("sweep"),
            file("tool-use-statistics"),
            file("stop-gate"),
        ])
    );
}
