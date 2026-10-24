use {
    super::*,
    crate::{
        configuration::{CargoPackage, CargoSource, Package, Resource},
        convergence::Lane,
        reporting::{RunKind, Screen},
    },
};

fn a_binary_at(directory: &Path, name: &str, contents: &str) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn a_machine_naming_no_executable_extensions_looks_only_for_the_bare_program_name() {
    assert_eq!(executable_extensions(None), vec![String::new()]);
}

#[test]
fn a_machine_naming_executable_extensions_looks_for_each_of_them_as_well() {
    assert_eq!(
        executable_extensions(Some(OsStr::new(".COM;.EXE;.BAT"))),
        vec![
            String::new(),
            ".com".to_owned(),
            ".exe".to_owned(),
            ".bat".to_owned()
        ]
    );
}

#[cfg(target_family = "windows")]
#[test]
fn extensions_that_are_not_text_still_leave_the_ones_beside_them_searchable() {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};

    let unpaired_surrogate = OsString::from_wide(&[
        0x002E, 0x0045, 0x0058, 0x0045, 0x003B, 0x002E, 0xD800, 0x003B, 0x002E, 0x0042, 0x0041,
        0x0054,
    ]);

    let extensions = executable_extensions(Some(&unpaired_surrogate));

    assert!(
        extensions.contains(&".exe".to_owned()) && extensions.contains(&".bat".to_owned()),
        "expected the readable extensions to survive, got: {extensions:?}"
    );
}

#[test]
fn a_file_holding_text_is_read_as_the_text_it_holds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.json");
    fs::write(&path, "{ \"machine\": {} }").unwrap();

    assert_eq!(
        read_text_file(&path).unwrap(),
        Some("{ \"machine\": {} }".to_owned())
    );
}

#[test]
fn a_path_holding_nothing_is_read_as_nothing_being_there() {
    let directory = tempfile::tempdir().unwrap();

    let held = read_text_file(&directory.path().join("machine.json")).unwrap();

    assert_eq!(held, None);
}

#[test]
fn a_file_that_is_there_but_is_not_text_is_refused_rather_than_read_as_absent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.json");
    fs::write(&path, [0xF0, 0x28, 0x8C, 0x28]).unwrap();

    let error = read_text_file(&path).unwrap_err();

    assert!(
        format!("{error:#}").contains("machine.json"),
        "expected the message to name the file it could not read, got: {error:#}"
    );
}

#[test]
fn what_is_left_behind_keeps_its_extension_so_the_search_path_cannot_invoke_it() {
    let superseded = superseded_name(Path::new("C:\\cargo\\bin\\claude-session.exe"));

    assert_eq!(
        superseded,
        PathBuf::from("C:\\cargo\\bin\\claude-session.exe.superseded")
    );
}

#[test]
fn an_interrupted_download_leaves_nothing_at_the_destination_a_later_run_could_read_as_finished() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("rg.exe");
    let partial_path = partial_download_path(&destination);
    fs::write(&partial_path, "an interrupted body").unwrap();

    assert!(!destination.exists());
    assert_ne!(partial_path, destination);
}

#[test]
fn a_partial_download_path_cannot_collide_with_a_destination_that_shares_a_stem() {
    let exe = partial_download_path(Path::new("C:\\tools\\rg.exe"));
    let zip = partial_download_path(Path::new("C:\\tools\\rg.zip"));

    assert_ne!(exe, zip);
}

#[test]
fn a_running_launcher_holding_the_bytes_of_its_replacement_already_holds_the_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let environment_copy = a_binary_at(directory.path(), "environment.exe", "a launcher");
    let running = a_binary_at(directory.path(), "serena.exe", "a launcher");

    assert!(holds_the_same_bytes(&environment_copy, &running).unwrap());
}

#[test]
fn a_running_launcher_differing_from_its_replacement_does_not_hold_it() {
    let directory = tempfile::tempdir().unwrap();
    let environment_copy = a_binary_at(directory.path(), "environment.exe", "launcher 1.7.0");
    let running = a_binary_at(directory.path(), "serena.exe", "launcher 1.5.3");

    assert!(!holds_the_same_bytes(&environment_copy, &running).unwrap());
}

#[test]
fn displacing_a_binary_frees_its_name_and_keeps_the_image_beside_it() {
    let directory = tempfile::tempdir().unwrap();
    let destination = a_binary_at(directory.path(), "claude-session.exe", "the old image");

    let superseded = displace(&destination).unwrap();

    assert!(!destination.exists(), "the name should be free to write");
    assert_eq!(fs::read_to_string(&superseded).unwrap(), "the old image");
}

#[test]
fn displacing_a_binary_reaps_the_image_an_earlier_run_left_under_that_exact_name() {
    let directory = tempfile::tempdir().unwrap();
    let destination = a_binary_at(directory.path(), "claude-session.exe", "the current image");
    a_binary_at(
        directory.path(),
        "claude-session.exe.superseded",
        "an image from an earlier run",
    );

    let superseded = displace(&destination).unwrap();

    assert_eq!(
        fs::read_to_string(&superseded).unwrap(),
        "the current image"
    );
}

#[test]
fn superseded_images_are_found_in_every_directory_the_program_can_displace_into() {
    let home_directory = tempfile::tempdir().unwrap();
    let cargo_directory = tempfile::tempdir().unwrap();
    let tool_directory = crate::tool_directory_within(home_directory.path());
    let tool_binaries_directory = tool_directory.join("bin");
    fs::create_dir_all(&tool_binaries_directory).unwrap();
    a_binary_at(
        &tool_binaries_directory,
        "dotfiles_configurator.exe.superseded",
        "an old tool image",
    );
    a_binary_at(
        cargo_directory.path(),
        "claude-session.exe.superseded",
        "an old cargo image",
    );

    let report = RunReport::open_in(home_directory.path(), RunKind::Apply).unwrap();
    let github = GitHubAccess::new();
    let machine = LocalMachine {
        home_directory: home_directory.path().to_path_buf(),
        tool_directory,
        download_directory: home_directory.path().to_path_buf(),
        cargo_binaries_directory: cargo_directory.path().to_path_buf(),
        github: &github,
        http_client: Client::default(),
        report: &report,
        elevated: false,
    };

    let images = machine.superseded_images();

    assert!(
        images.contains(&cargo_directory.path().join("claude-session.exe.superseded")),
        "{images:?}"
    );
    assert!(
        images.contains(&tool_binaries_directory.join("dotfiles_configurator.exe.superseded")),
        "{images:?}"
    );
    assert_eq!(images.len(), 2, "{images:?}");
}

#[test]
fn restoring_puts_the_moved_image_back_under_the_name_it_had() {
    let directory = tempfile::tempdir().unwrap();
    let destination = a_binary_at(directory.path(), "claude-session.exe", "the old image");
    let superseded = displace(&destination).unwrap();

    restore(&superseded, &destination).unwrap();

    assert_eq!(fs::read_to_string(&destination).unwrap(), "the old image");
    assert!(!superseded.exists());
}

#[cfg(target_family = "windows")]
const A_SHELL_EVERY_MACHINE_HAS: Shell = Shell::CommandPrompt;
#[cfg(target_family = "unix")]
const A_SHELL_EVERY_MACHINE_HAS: Shell = Shell::Bash;

fn echoing_two_lines() -> (String, Vec<String>) {
    shell_invocation(
        A_SHELL_EVERY_MACHINE_HAS,
        &[
            "echo".to_owned(),
            "first".to_owned(),
            "&&".to_owned(),
            "echo".to_owned(),
            "second".to_owned(),
        ],
    )
}

#[test]
fn a_machine_naming_executable_extensions_launches_only_a_file_carrying_one() {
    assert_eq!(
        launchable_extensions(Some(OsStr::new(".EXE;.BAT"))),
        vec![".exe".to_owned(), ".bat".to_owned()]
    );
}

#[cfg(target_family = "windows")]
#[test]
fn a_batch_file_on_the_search_path_is_launched_by_its_bare_name_with_its_argument_whole() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();
    a_binary_at(directory.path(), "echo-argument.bat", "@echo %~1\r\n");
    let argument = "coderabbit_findings@{git: {url: https://example.invalid/dotfiles.git, \
                    path: tools/coderabbit-findings, ref: main}}";

    let output = capture_with(
        Path::new("echo-argument"),
        &[argument.to_owned()],
        &[("PATH".to_owned(), directory.path().display().to_string())],
        &report,
    )
    .unwrap();

    assert_eq!(output.standard_output.trim(), argument);
}

#[tokio::test]
async fn every_line_a_changing_child_writes_reaches_the_log() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();
    let (program, arguments) = echoing_two_lines();

    let output = stream(Path::new(&program), &arguments, &[], None, &report)
        .await
        .unwrap();

    let written = fs::read_to_string(report.log_path()).unwrap();
    assert!(output.exited.succeeded(), "{output:?}");
    assert!(
        written.contains("first") && written.contains("second"),
        "{written}"
    );
}

fn exiting_with(code: &str) -> (String, Vec<String>) {
    shell_invocation(
        A_SHELL_EVERY_MACHINE_HAS,
        &["exit".to_owned(), code.to_owned()],
    )
}

#[tokio::test]
async fn a_streamed_child_that_fails_has_its_exit_code_written_to_the_log() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();
    let (program, arguments) = exiting_with("3");

    stream(Path::new(&program), &arguments, &[], None, &report)
        .await
        .unwrap();

    let written = fs::read_to_string(report.log_path()).unwrap();
    assert!(written.contains("exited with 3"), "{written}");
}

#[tokio::test]
async fn a_streamed_child_that_succeeds_has_its_exit_code_written_to_the_log() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();
    let (program, arguments) = exiting_with("0");

    stream(Path::new(&program), &arguments, &[], None, &report)
        .await
        .unwrap();

    let written = fs::read_to_string(report.log_path()).unwrap();
    assert!(written.contains("exited with 0"), "{written}");
}

#[test]
fn a_captured_child_that_fails_has_its_exit_code_written_to_the_log() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Plan).unwrap();
    let (program, arguments) = exiting_with("3");

    capture(Path::new(&program), &arguments, &report).unwrap();

    let written = fs::read_to_string(report.log_path()).unwrap();
    assert!(written.contains("exited with 3"), "{written}");
}

#[test]
fn a_captured_child_that_succeeds_has_its_exit_code_written_to_the_log() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Plan).unwrap();
    let (program, arguments) = exiting_with("0");

    capture(Path::new(&program), &arguments, &report).unwrap();

    let written = fs::read_to_string(report.log_path()).unwrap();
    assert!(written.contains("exited with 0"), "{written}");
}

#[tokio::test]
async fn the_refusal_of_a_streamed_child_names_the_code_it_exited_with() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();
    let (program, arguments) = exiting_with("3");

    let output = stream(Path::new(&program), &arguments, &[], None, &report)
        .await
        .unwrap();

    let refusal = format!("{:#}", refused(Tool::Cargo, &arguments, &output));
    assert!(refusal.contains("exited with 3"), "{refusal}");
}

#[test]
fn the_refusal_of_a_captured_child_names_the_code_it_exited_with() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Plan).unwrap();
    let (program, arguments) = exiting_with("3");

    let output = capture(Path::new(&program), &arguments, &report).unwrap();

    let refusal = format!("{:#}", refused(Tool::Git, &arguments, &output));
    assert!(refusal.contains("exited with 3"), "{refusal}");
}

#[tokio::test]
async fn every_line_a_child_writes_while_converging_an_entry_reaches_the_log_prefixed_with_it() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_showing(
        directory.path(),
        RunKind::Apply,
        Screen::Lines(Box::new(std::io::sink())),
    )
    .unwrap();
    let (program, arguments) = echoing_two_lines();
    let stop_gate = Entry::new(
        Lane::Cargo,
        &Resource::Package(Package::Cargo(CargoPackage {
            crate_name: CrateName::from("stop-gate"),
            source: CargoSource::Registry {
                version: crate::configuration::PackageCurrency::Latest,
            },
        })),
    );

    report
        .converging(&stop_gate, async {
            stream(Path::new(&program), &arguments, &[], None, &report)
                .await
                .unwrap()
        })
        .await;

    let written = fs::read_to_string(report.log_path()).unwrap();
    assert!(
        written.contains("[cargo] stop-gate: first")
            && written.contains("[cargo] stop-gate: second"),
        "{written}"
    );
}

#[tokio::test]
async fn a_streamed_child_still_hands_back_its_output_for_a_failure_message_to_quote() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();
    let (program, arguments) = echoing_two_lines();

    let output = stream(Path::new(&program), &arguments, &[], None, &report)
        .await
        .unwrap();

    assert!(output.standard_output.contains("first"), "{output:?}");
    assert!(output.standard_output.contains("second"), "{output:?}");
}

#[test]
fn the_log_names_the_invocation_a_captured_read_ran() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Plan).unwrap();
    let (program, arguments) = echoing_two_lines();

    capture(Path::new(&program), &arguments, &report).unwrap();

    let written = fs::read_to_string(report.log_path()).unwrap();
    assert!(written.contains(&program), "{written}");
}

#[test]
fn output_a_program_redirected_as_utf16_is_matchable_once_its_nuls_are_stripped() {
    let redirected: Vec<u8> = "Ubuntu (Default)"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();

    assert!(decode_output(&redirected).contains("Ubuntu (Default)"));
}

#[test]
fn output_from_a_program_that_does_not_redirect_as_utf16_is_left_alone() {
    assert_eq!(decode_output(b"committed v1.1.11:"), "committed v1.1.11:");
}

#[test]
fn a_declared_command_reaches_its_shell_as_one_string_so_the_shell_does_the_quoting() {
    let (program, arguments) = shell_invocation(
        Shell::CommandPrompt,
        &["echo".to_owned(), "hello world".to_owned()],
    );

    assert_eq!(program, "cmd");
    assert_eq!(arguments, vec!["/C", "echo hello world"]);
}

fn archive_holding(directory: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
    let archive_path = directory.join("release.zip");
    let mut writer = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
    for (name, contents) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, contents).unwrap();
    }
    writer.finish().unwrap();
    archive_path
}

fn entry(path: &str) -> ArchiveEntry {
    ArchiveEntry::try_from(path.to_owned()).unwrap()
}

#[test]
fn the_entry_read_out_of_an_archive_is_the_one_the_configuration_named() {
    let directory = tempfile::tempdir().unwrap();
    let archive_path = archive_holding(
        directory.path(),
        &[("notes.txt", b"read me"), ("bin/rg.exe", b"the binary")],
    );

    let contents = read_archive_entry(&archive_path, &entry("bin/rg.exe")).unwrap();

    assert_eq!(contents, b"the binary");
}

#[test]
fn an_archive_holding_no_such_entry_is_refused_by_naming_what_it_does_hold() {
    let directory = tempfile::tempdir().unwrap();
    let archive_path = archive_holding(directory.path(), &[("rg.exe", b"the binary")]);

    let error = read_archive_entry(&archive_path, &entry("bin/rg.exe"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("rg.exe"), "{error}");
}

#[test]
fn a_binary_written_where_none_is_installed_needs_no_displacement() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("rg.exe");

    place_executable(&destination, b"the new copy").unwrap();

    assert_eq!(fs::read(&destination).unwrap(), b"the new copy");
    assert!(!directory.path().join("rg.exe.superseded").exists());
}

#[cfg(target_family = "windows")]
fn refused_with(os_error: i32) -> anyhow::Error {
    anyhow::Error::from(std::io::Error::from_raw_os_error(os_error))
        .context("Could not link C:\\Users\\Alice\\.gitconfig to C:\\dotfiles\\gitconfig")
}

#[cfg(target_family = "windows")]
#[test]
fn a_link_refused_with_os_error_1314_wants_a_privilege() {
    assert!(refused_for_want_of(&refused_with(1314), Want::Privilege));
}

#[cfg(target_family = "windows")]
#[test]
fn a_link_refused_for_any_other_reason_wants_no_privilege() {
    assert!(!refused_for_want_of(&refused_with(5), Want::Privilege));
}

#[cfg(target_family = "windows")]
#[test]
fn an_installer_refused_with_os_error_740_wants_elevation() {
    assert!(refused_for_want_of(&refused_with(740), Want::Elevation));
}

#[cfg(target_family = "windows")]
#[test]
fn an_installer_refused_for_want_of_a_privilege_is_not_read_as_wanting_elevation() {
    assert!(!refused_for_want_of(&refused_with(1314), Want::Elevation));
}

#[test]
fn a_wsl_command_is_run_through_wsl_rather_than_through_the_command_prompt() {
    let (program, arguments) = shell_invocation(Shell::Wsl, &["ls".to_owned()]);

    assert_eq!(program, "wsl");
    assert_eq!(arguments, vec!["--", "bash", "-c", "ls"]);
}

#[cfg(target_family = "windows")]
const AN_ECHO_OF_CARGO_TARGET_DIR: &str = "echo %CARGO_TARGET_DIR%";
#[cfg(target_family = "unix")]
const AN_ECHO_OF_CARGO_TARGET_DIR: &str = "echo $CARGO_TARGET_DIR";

#[tokio::test]
async fn the_environment_a_changing_invocation_asks_for_reaches_the_child_that_runs_it() {
    let directory = tempfile::tempdir().unwrap();
    let report = RunReport::open_in(directory.path(), RunKind::Apply).unwrap();
    let (program, arguments) = shell_invocation(
        A_SHELL_EVERY_MACHINE_HAS,
        &[AN_ECHO_OF_CARGO_TARGET_DIR.to_owned()],
    );
    let asked = "the-build-cache-of-a-revision";

    let output = stream(
        Path::new(&program),
        &arguments,
        &[("CARGO_TARGET_DIR".to_owned(), asked.to_owned())],
        None,
        &report,
    )
    .await
    .unwrap();

    assert!(output.exited.succeeded(), "{output:?}");
    assert!(output.standard_output.contains(asked), "{output:?}");
}

fn build_cache_holding(revisions: &[&str]) -> tempfile::TempDir {
    let build_cache = tempfile::tempdir().unwrap();
    for revision in revisions {
        fs::create_dir_all(build_cache.path().join(revision).join("release")).unwrap();
    }

    build_cache
}

#[test]
fn building_a_revision_reaps_the_builds_of_every_other_revision() {
    let build_cache = build_cache_holding(&["older", "oldest", "current"]);
    let keeping = build_cache.path().join("current");

    let reaped =
        builds_of_other_revisions_reaped(build_cache.path(), &BTreeSet::from([keeping.clone()]));

    assert_eq!(reaped.len(), 2);
    assert!(keeping.exists());
    assert!(!build_cache.path().join("older").exists());
    assert!(!build_cache.path().join("oldest").exists());
}

#[test]
fn building_two_revisions_at_once_keeps_the_builds_of_both() {
    let build_cache = build_cache_holding(&["older", "dotfiles", "configurator"]);
    let keeping = BTreeSet::from([
        build_cache.path().join("dotfiles"),
        build_cache.path().join("configurator"),
    ]);

    let reaped = builds_of_other_revisions_reaped(build_cache.path(), &keeping);

    assert_eq!(reaped, vec![build_cache.path().join("older")]);
    assert!(keeping.iter().all(|kept| kept.exists()));
}

#[test]
fn building_the_only_revision_a_build_cache_holds_reaps_nothing() {
    let build_cache = build_cache_holding(&["current"]);
    let keeping = build_cache.path().join("current");

    assert!(
        builds_of_other_revisions_reaped(build_cache.path(), &BTreeSet::from([keeping.clone()]))
            .is_empty()
    );
    assert!(keeping.exists());
}

#[test]
fn building_against_a_build_cache_that_does_not_exist_yet_reaps_nothing() {
    let absent = tempfile::tempdir().unwrap().path().join("build-cache");

    assert!(
        builds_of_other_revisions_reaped(&absent, &BTreeSet::from([absent.join("current")]))
            .is_empty()
    );
}
