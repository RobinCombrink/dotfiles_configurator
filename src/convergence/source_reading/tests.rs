use super::*;

#[test]
fn a_source_nothing_asked_for_is_an_impediment_naming_the_source() {
    let reading: SourceReading<String> =
        SourceReading::NotRequested(ReadSource::Tool(Tool::Winget));

    let impediment = reading.read().expect_err("a reading that holds no value");

    assert_eq!(
        impediment,
        Impediment::ActualStateUnreadable(UnreadableReason::NotRead(ReadSource::Tool(
            Tool::Winget
        )))
    );
}

#[test]
fn a_workspace_nothing_asked_for_names_the_directory_it_would_have_been_read_from() {
    let directory = PathBuf::from("/repositories/Personal/dotfiles");
    let reading: SourceReading<String> =
        SourceReading::NotRequested(ReadSource::CargoWorkspace(directory.clone()));

    let impediment = reading.read().expect_err("a reading that holds no value");

    assert_eq!(
        impediment,
        Impediment::ActualStateUnreadable(UnreadableReason::NotRead(ReadSource::CargoWorkspace(
            directory
        )))
    );
}

#[test]
fn a_source_that_was_read_answers_the_value_it_read() {
    let reading = SourceReading::Read("a listing".to_owned());

    assert_eq!(
        reading.read().expect("a reading that holds a value"),
        "a listing"
    );
}
