use serde_json::json;

use super::*;

fn written() -> serde_json::Value {
    json!({
        "machine": {
            "repositories_directory_path": "/repositories/Personal",
            "estates": { "personal": ["RobinCombrink"] },
            "class": "personal",
            "configuration_sources": ["github:RobinCombrink/dotfiles/config"]
        }
    })
}

fn decoded(document: serde_json::Value) -> Result<Document, serde_json::Error> {
    serde_json::from_value(document)
}

#[test]
fn a_manifest_reads_back_as_the_document_it_was_written_from() {
    let document = decoded(written()).expect("a written manifest");

    assert_eq!(
        serde_json::to_value(&document).expect("a manifest serialises"),
        written()
    );
}

#[test]
fn a_manifest_names_the_class_and_sources_of_the_run_beside_the_directory() {
    let document = decoded(written()).expect("a written manifest");

    assert_eq!(
        document.machine.recorded_run,
        RecordedRun {
            class: MachineClass::Personal,
            configuration_sources: vec![RecordedSource::from(
                "github:RobinCombrink/dotfiles/config".to_owned()
            )],
        }
    );
}

#[test]
fn a_manifest_carrying_keys_no_reader_reads_still_decodes() {
    let mut carrying = written();
    carrying["version"] = json!(3);
    carrying["machine"]["github_username"] = json!("RobinCombrink");

    let document = decoded(carrying).expect("a manifest with keys nobody reads");

    assert_eq!(
        document.machine.repositories_directory_path.as_path(),
        Path::new("/repositories/Personal")
    );
}

#[test]
fn a_relative_repositories_directory_is_refused_when_the_manifest_decodes() {
    let mut relative = written();
    relative["machine"]["repositories_directory_path"] = json!("Repositories");

    let refusal = decoded(relative).expect_err("a manifest naming a relative directory");

    assert!(refusal.is_data(), "{refusal}");
}

#[test]
fn a_directory_is_rooted_or_it_names_itself_in_its_refusal() {
    let rooted = RepositoriesDirectory::try_from(PathBuf::from("/repositories/Personal"))
        .expect("a rooted directory");
    assert_eq!(rooted.as_path(), Path::new("/repositories/Personal"));
    assert_eq!(
        RepositoriesDirectory::try_from(PathBuf::from("Repositories")),
        Err(NotRooted(PathBuf::from("Repositories")))
    );
}

#[test]
fn a_manifest_naming_no_slots_per_bank_decodes_as_naming_none() {
    let document = decoded(written()).expect("a manifest without the setting");

    assert_eq!(document.machine.slots_per_bank, None);
}

#[test]
fn a_manifest_naming_its_slots_per_bank_reads_back_as_it_was_written() {
    let mut naming = written();
    naming["machine"]["slots_per_bank"] = json!(2);

    let document = decoded(naming.clone()).expect("a manifest naming two slots per bank");

    assert_eq!(document.machine.slots_per_bank, Some(SlotsPerBank::Two));
    assert_eq!(
        serde_json::to_value(&document).expect("a manifest serialises"),
        naming
    );
}

#[test]
fn a_manifest_naming_an_unsupported_slots_per_bank_is_refused() {
    let mut unsupported = written();
    unsupported["machine"]["slots_per_bank"] = json!(4);

    let refusal = decoded(unsupported).expect_err("a manifest naming four slots per bank");

    assert!(refusal.is_data(), "{refusal}");
}

#[test]
fn a_manifest_missing_a_field_the_configurator_writes_is_refused_naming_it() {
    for field in [
        "repositories_directory_path",
        "estates",
        "class",
        "configuration_sources",
    ] {
        let mut missing = written();
        missing["machine"]
            .as_object_mut()
            .expect("a machine object")
            .remove(field);

        let refusal = decoded(missing).expect_err("a manifest missing a field");

        assert!(refusal.to_string().contains(field), "{field}: {refusal}");
    }
}
