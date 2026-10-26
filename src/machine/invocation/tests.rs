use super::*;
use crate::{
    configuration::{McpScope, RepositoryName, RepositoryOwner},
    machine::{
        Exited,
        workspace_reading::{Fingerprint, MemberReading, ObjectHash},
    },
};
use std::collections::BTreeMap;

fn cargo_said(standard_error: &str) -> CommandOutput {
    CommandOutput {
        exited: Exited::Code(101),
        standard_output: String::new(),
        standard_error: standard_error.to_owned(),
    }
}

fn installing_a_crate() -> DisplacingInvocation {
    installing(
        "claude-session",
        ResolvedCargoSource::Registry { version: None },
    )
}

fn installing(crate_name: &str, source: ResolvedCargoSource) -> DisplacingInvocation {
    DisplacingInvocation::InstallCargoCrate {
        crate_name: CrateName::from(crate_name),
        source,
    }
}

#[test]
fn a_denied_move_is_read_as_the_destination_rather_than_the_file_it_was_built_into() {
    let output = cargo_said(
        "   Replacing C:\\Users\\Alice\\.cargo\\bin\\claude-session.exe\n\
         error: failed to move `C:\\Users\\Alice\\.cargo\\bin\\cargo-installpDjhGc\\claude-session.exe` \
         to `C:\\Users\\Alice\\.cargo\\bin\\claude-session.exe`\n\n\
         Caused by:\n  Access is denied. (os error 5)\n",
    );

    assert_eq!(
        installing_a_crate().refused_destination(&output),
        Some(PathBuf::from(
            "C:\\Users\\Alice\\.cargo\\bin\\claude-session.exe"
        ))
    );
}

#[test]
fn a_denied_move_whose_paths_wrapped_across_lines_still_names_its_destination() {
    let output = cargo_said(
        "error: failed to move `C:\\temp\\claude-session.exe`\n       \
         to `C:\\Users\\Alice\\.cargo\\bin\\claude-session.exe`\n",
    );

    assert_eq!(
        installing_a_crate().refused_destination(&output),
        Some(PathBuf::from(
            "C:\\Users\\Alice\\.cargo\\bin\\claude-session.exe"
        ))
    );
}

#[test]
fn a_failure_that_names_no_path_it_could_not_write_is_not_recovered_from_by_displacing() {
    let output = cargo_said("error: could not compile `claude-session` (bin \"claude-session\")");

    assert_eq!(installing_a_crate().refused_destination(&output), None);
}

fn build_cache() -> PathBuf {
    PathBuf::from("C:\\Users\\Alice\\.dotfiles_configurator\\build-cache")
}

fn installing_from_revision(revision: &str) -> DisplacingInvocation {
    installing(
        "stop-gate",
        ResolvedCargoSource::Repository {
            repository: GitHubRepository {
                owner: RepositoryOwner::from("Alice"),
                repository: RepositoryName::from("dotfiles"),
            },
            account: GitHubAccount::from("Alice"),
            revision: Revision::from(revision),
        },
    )
}

fn value_of<'environment>(
    environment: &'environment [(String, String)],
    name: &str,
) -> Option<&'environment str> {
    environment
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

#[test]
fn installing_a_crate_has_cargo_fetch_git_repositories_through_the_git_command_line() {
    assert_eq!(
        value_of(
            &installing_a_crate().environment(&build_cache()),
            "CARGO_NET_GIT_FETCH_WITH_CLI"
        ),
        Some("true")
    );
}

#[test]
fn every_crate_of_one_revision_builds_in_the_directory_that_revision_names() {
    let directory = installing_from_revision("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c")
        .build_directory(&build_cache());

    assert_eq!(
        directory,
        Some(build_cache().join("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c"))
    );
}

#[test]
fn two_revisions_of_one_repository_never_build_in_the_same_directory() {
    let earlier = installing_from_revision("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c")
        .build_directory(&build_cache());
    let later = installing_from_revision("f9a32f6c605fc1ed4584037a770376d084f74a8d")
        .build_directory(&build_cache());

    assert_ne!(earlier, later);
}

#[test]
fn a_crate_from_a_repository_has_cargo_build_it_in_the_directory_its_revision_names() {
    let invocation = installing_from_revision("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c");
    let environment = invocation.environment(&build_cache());

    assert_eq!(
        value_of(&environment, "CARGO_TARGET_DIR"),
        Some(
            build_cache()
                .join("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c")
                .display()
                .to_string()
                .as_str()
        )
    );
}

#[test]
fn a_crate_from_the_registry_is_left_to_build_where_cargo_would_put_it() {
    let invocation = installing("ripgrep", ResolvedCargoSource::Registry { version: None });

    assert_eq!(invocation.build_directory(&build_cache()), None);
    assert_eq!(
        value_of(&invocation.environment(&build_cache()), "CARGO_TARGET_DIR"),
        None
    );
}

#[test]
fn a_crate_from_a_path_is_left_to_build_where_cargo_would_put_it() {
    let invocation = installing(
        "stop-gate",
        ResolvedCargoSource::Path {
            path: PathBuf::from("C:\\Repositories\\dotfiles\\tools\\stop-gate"),
        },
    );

    assert_eq!(invocation.build_directory(&build_cache()), None);
}

#[test]
fn a_crate_from_the_registry_is_installed_by_name_with_the_lockfile_it_publishes() {
    assert_eq!(
        installing("ripgrep", ResolvedCargoSource::Registry { version: None }).arguments(),
        vec!["install", "--locked", "--force", "ripgrep"]
    );
}

#[test]
fn a_crate_pinned_to_a_version_is_installed_at_exactly_that_version() {
    let arguments = installing(
        "cargo-mutants",
        ResolvedCargoSource::Registry {
            version: Some(CrateVersion::try_from("27.1.0").unwrap()),
        },
    )
    .arguments();

    assert_eq!(
        arguments,
        vec![
            "install",
            "--locked",
            "--force",
            "--version",
            "27.1.0",
            "cargo-mutants"
        ]
    );
}

#[test]
fn a_crate_from_a_directory_is_installed_by_that_directory_rather_than_by_name() {
    let arguments = installing(
        "stop-gate",
        ResolvedCargoSource::Path {
            path: PathBuf::from("C:\\Repositories\\dotfiles\\tools\\stop-gate"),
        },
    )
    .arguments();

    assert_eq!(
        arguments,
        vec![
            "install",
            "--locked",
            "--force",
            "--path",
            "C:\\Repositories\\dotfiles\\tools\\stop-gate",
        ]
    );
}

#[test]
fn a_crate_from_a_repository_is_installed_at_the_revision_read_from_the_clone() {
    let arguments = installing(
        "stop-gate",
        ResolvedCargoSource::Repository {
            repository: GitHubRepository {
                owner: RepositoryOwner::from("Alice"),
                repository: RepositoryName::from("dotfiles"),
            },
            account: GitHubAccount::from("Alice"),
            revision: Revision::from("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c"),
        },
    )
    .arguments();

    assert_eq!(
        arguments,
        vec![
            "install",
            "--locked",
            "--force",
            "--git",
            "https://Alice@github.com/Alice/dotfiles",
            "--rev",
            "2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c",
            "stop-gate",
        ]
    );
}

fn workspace_at(revision: &str, members: &[&str]) -> WorkspaceReading {
    WorkspaceReading {
        revision: Revision::from(revision),
        members: members
            .iter()
            .map(|name| {
                let fingerprint = Fingerprint {
                    crate_subtree: ObjectHash::from("aaa"),
                    workspace_bindings: BTreeMap::new(),
                    lock_closure: ObjectHash::from("ccc"),
                    dependency_subtrees: BTreeMap::new(),
                };
                (
                    CrateName::from(*name),
                    MemberReading {
                        desired: fingerprint,
                        binaries: BTreeSet::new(),
                    },
                )
            })
            .collect(),
    }
}

fn changed(members: &[&str]) -> Vec<CrateName> {
    members.iter().map(|name| CrateName::from(*name)).collect()
}

fn clone_directory() -> PathBuf {
    PathBuf::from("C:\\Repositories\\Alice\\dotfiles")
}

#[test]
fn a_workspace_build_names_each_changed_member_and_keeps_going_past_one_that_fails() {
    let reading = workspace_at(
        "2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c",
        &["stop-gate", "live-set", "session-mining"],
    );

    let build = WorkspaceBuild::of(
        clone_directory(),
        &reading,
        changed(&["stop-gate", "live-set"]),
    )
    .expect("two members to build");

    assert_eq!(
        build.arguments(),
        vec![
            "build",
            "--release",
            "--locked",
            "--keep-going",
            "-p",
            "live-set",
            "-p",
            "stop-gate",
        ]
    );
}

#[test]
fn a_workspace_with_no_member_to_change_has_nothing_to_build() {
    let reading = workspace_at("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c", &["stop-gate"]);

    assert_eq!(
        WorkspaceBuild::of(clone_directory(), &reading, changed(&[])),
        None
    );
}

#[test]
fn a_workspace_build_targets_the_directory_its_revisions_installs_build_in() {
    let reading = workspace_at("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c", &["stop-gate"]);
    let build = WorkspaceBuild::of(clone_directory(), &reading, changed(&["stop-gate"]))
        .expect("one member to build");
    let install = installing_from_revision("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c");

    let built_in = build.environment(&build_cache());
    let installed_from = install.environment(&build_cache());

    assert_eq!(
        value_of(&built_in, "CARGO_TARGET_DIR"),
        value_of(&installed_from, "CARGO_TARGET_DIR")
    );
}

#[test]
fn a_workspace_build_checks_its_revision_out_beside_rather_than_inside_its_build_directory() {
    let reading = workspace_at("2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c", &["stop-gate"]);
    let build = WorkspaceBuild::of(clone_directory(), &reading, changed(&["stop-gate"]))
        .expect("one member to build");

    let source = build.source_directory(&build_cache());

    assert!(!source.starts_with(build.build_directory(&build_cache())));
}

fn registering(server: ClaudeMcpServer) -> ReplacingInvocation {
    ReplacingInvocation::ClaudeMcpServer {
        server: Box::new(server),
    }
}

fn serena() -> ClaudeMcpServer {
    let mut environment = BTreeMap::new();
    environment.insert("SERENA_HOME".to_owned(), "C:\\dotfiles\\serena".to_owned());

    ClaudeMcpServer {
        name: McpServerName::from("serena"),
        scope: McpScope::User,
        command: "serena".to_owned(),
        args: vec!["start-mcp-server".to_owned()],
        environment,
    }
}

#[test]
fn registering_a_server_passes_its_environment_before_the_command() {
    assert_eq!(
        registering(serena()).commands().claim_the_name,
        vec![
            "mcp",
            "add",
            "--scope",
            "user",
            "serena",
            "--env",
            "SERENA_HOME=C:\\dotfiles\\serena",
            "--",
            "serena",
            "start-mcp-server",
        ]
    );
}

#[test]
fn registering_a_server_frees_its_name_in_the_scope_the_new_registration_claims() {
    assert_eq!(
        registering(serena()).commands().free_the_name,
        vec!["mcp", "remove", "--scope", "user", "serena"]
    );
}

#[test]
fn a_refused_claim_on_a_name_something_was_removed_from_leaves_that_name_holding_nothing() {
    let outcome = registering(serena())
        .refused_claim(true, anyhow::anyhow!("claude refused"))
        .expect("a half-applied replacement is an outcome rather than an error");

    match outcome {
        Replacement::RemovedButCouldNotAdd { name, .. } => {
            assert_eq!(name, McpServerName::from("serena"));
        }
        Replacement::Replaced => panic!("a refused claim did not replace anything"),
    }
}

#[test]
fn a_refused_claim_on_a_name_nothing_was_removed_from_took_nothing_away_to_report() {
    let error = registering(serena())
        .refused_claim(false, anyhow::anyhow!("claude refused"))
        .expect_err("a refusal that changed nothing is an error");

    assert_eq!(error.to_string(), "claude refused");
}

#[test]
fn a_uv_tool_declaring_no_interpreter_leaves_the_choice_of_one_to_uv() {
    let installing = WriteInvocation::InstallUvTool {
        name: UvToolName::from("serena-agent"),
        python: None,
        version: None,
    };

    assert_eq!(
        installing.arguments(),
        vec!["tool", "install", "serena-agent"]
    );
}

#[test]
fn a_uv_tool_kept_at_one_version_is_installed_at_exactly_that_version() {
    let installing = WriteInvocation::InstallUvTool {
        name: UvToolName::from("serena-agent"),
        python: None,
        version: Some(UvToolVersion::from("1.5.3")),
    };

    assert_eq!(
        installing.arguments(),
        vec!["tool", "install", "serena-agent==1.5.3"]
    );
}

fn reinstalling_serena_at_latest() -> WriteInvocation {
    WriteInvocation::ReinstallUvToolAtLatest {
        name: UvToolName::from("serena-agent"),
        python: Some(PythonInterpreter::from("3.13")),
    }
}

#[test]
fn a_pinned_uv_tool_is_reinstalled_at_the_latest_with_its_declared_interpreter() {
    assert_eq!(
        reinstalling_serena_at_latest().arguments(),
        vec!["tool", "install", "--python", "3.13", "serena-agent@latest"]
    );
}

#[test]
fn a_reinstall_refused_over_a_running_launcher_is_held() {
    let output = cargo_said(
        "error: failed to remove directory `C:\\t\\uvprobe\\tools\\serena-agent\\Scripts`: \
         Access is denied. (os error 5)\n",
    );

    let Some(HeldReason::ReportedInUse(reported)) =
        reinstalling_serena_at_latest().held_by(&output)
    else {
        panic!("expected the reinstall to be held as in use");
    };
    assert!(reported.contains("Scripts"), "{reported}");
}

#[test]
fn a_reinstall_failing_for_any_other_reason_is_not_held() {
    let output = cargo_said("error: Failed to fetch the index (os error 10061)\n");

    assert_eq!(reinstalling_serena_at_latest().held_by(&output), None);
}

fn upgrading_serena() -> WriteInvocation {
    WriteInvocation::UpgradeUvTool {
        name: UvToolName::from("serena-agent"),
    }
}

// 2026-09-25: taken verbatim from `uv tool upgrade serena-agent` with the tool's launcher
// running, under uv 0.10.12 on Windows 11.
const UV_REFUSED_A_RUNNING_LAUNCHER: &str = concat!(
    "error: Failed to upgrade serena-agent\n",
    "  Caused by: Failed to install entrypoint\n",
    "  Caused by: failed to copy file from ",
    "C:\\t\\uvprobe\\tools\\serena-agent\\Scripts\\serena.exe to ",
    "C:/t/uvprobe/bin\\serena.exe: The process cannot access the file because it is being ",
    "used by another process. (os error 32)\n",
);

#[test]
fn a_copy_refused_over_a_running_launcher_names_both_the_copy_and_what_it_would_replace() {
    assert_eq!(
        upgrading_serena().refused_copy(&cargo_said(UV_REFUSED_A_RUNNING_LAUNCHER)),
        Some(RefusedCopy {
            source: PathBuf::from("C:\\t\\uvprobe\\tools\\serena-agent\\Scripts\\serena.exe"),
            destination: PathBuf::from("C:/t/uvprobe/bin\\serena.exe"),
        })
    );
}

#[test]
fn a_copy_refused_for_any_other_reason_is_not_read_as_a_running_launcher() {
    let output = cargo_said(
        "  Caused by: failed to copy file from C:\\env\\serena.exe to C:\\bin\\serena.exe: \
         Access is denied. (os error 5)\n",
    );

    assert_eq!(upgrading_serena().refused_copy(&output), None);
}

#[test]
fn the_tools_uv_reports_as_behind_are_asked_for_even_where_uv_is_set_to_work_offline() {
    assert!(
        ReadInvocation::UvOutdatedTools
            .arguments()
            .contains(&"--no-offline".to_owned())
    );
}

fn uninstalling_session_census() -> WriteInvocation {
    WriteInvocation::UninstallCargoBinary {
        specification: "git+https://Alice@github.com/Alice/dotfiles?rev=0c532c68bd883d1e6b002ad5bb1f40aa2ed70f66#session-mining@0.1.0".to_owned(),
        binary: BinaryName::from("session-census"),
    }
}

#[test]
fn a_binary_is_uninstalled_out_of_the_one_record_its_specification_names() {
    assert_eq!(
        uninstalling_session_census().arguments(),
        vec![
            "uninstall",
            "--bin",
            "session-census",
            "git+https://Alice@github.com/Alice/dotfiles?rev=0c532c68bd883d1e6b002ad5bb1f40aa2ed70f66#session-mining@0.1.0",
        ]
    );
}

#[test]
fn an_uninstall_refused_because_a_recorded_file_is_gone_is_read_as_a_corrupt_record() {
    let output = cargo_said(
        "error: corrupt metadata, `C:\\Users\\Alice\\.cargo\\bin\\session-census.exe` does \
         not exist when it should\n",
    );

    assert!(uninstalling_session_census().refused_as_corrupt(&output));
}

#[test]
fn an_uninstall_refused_for_any_other_reason_is_not_read_as_a_corrupt_record() {
    let output = cargo_said(
        "error: package ID specification `session-mining@0.1.0` did not match any packages\n",
    );

    assert!(!uninstalling_session_census().refused_as_corrupt(&output));
}

fn shfmt() -> WingetPackageId {
    WingetPackageId::from("mvdan.shfmt")
}

#[test]
fn a_winget_package_kept_at_the_latest_is_installed_at_whatever_winget_offers() {
    let arguments = WriteInvocation::InstallWingetPackage {
        id: shfmt(),
        version: None,
    }
    .arguments();

    assert!(
        !arguments.contains(&"--version".to_owned()),
        "{arguments:?}"
    );
}

#[test]
fn a_winget_package_kept_at_one_version_is_installed_over_whatever_version_it_is_at() {
    let arguments = WriteInvocation::InstallWingetPackage {
        id: shfmt(),
        version: Some(WingetVersion::from("3.13.1")),
    }
    .arguments();

    assert_eq!(
        arguments,
        vec![
            "install",
            "--exact",
            "--id",
            "mvdan.shfmt",
            "--version",
            "3.13.1",
            "--force",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ]
    );
}

fn winget_said(code: u32, standard_output: &str) -> CommandOutput {
    CommandOutput {
        exited: Exited::Code(code.cast_signed()),
        standard_output: standard_output.to_owned(),
        standard_error: String::new(),
    }
}

fn upgrading_shfmt() -> WriteInvocation {
    WriteInvocation::UpgradeWingetPackage { id: shfmt() }
}

// 2026-10-06: taken verbatim from `winget upgrade --id mvdan.shfmt --exact --silent
// --disable-interactivity --accept-package-agreements` with shfmt.exe running, under winget
// v1.29.380 on Windows 11, with the progress lines left out.
const WINGET_REFUSED_A_RUNNING_PORTABLE: &str = concat!(
    "Found shfmt [mvdan.shfmt] Version 3.14.1\n",
    "This application is licensed to you by its owner.\n",
    "Microsoft is not responsible for, nor does it grant any licenses to, third-party packages.\n",
    "Downloading https://github.com/mvdan/sh/releases/download/v3.14.1/shfmt_v3.14.1_windows_amd64.exe\n",
    "Successfully verified installer hash\n",
    "Starting package install...\n",
    "An unexpected error occurred while executing the command: \n",
    "remove: Access is denied.: \"C:\\Users\\Alice\\AppData\\Local\\Microsoft\\WinGet\\Packages\\mvdan.shfmt_Microsoft.Winget.Source_8wekyb3d8bbwe\\shfmt.exe\"\n",
    "Installer failed with exit code: 0x8a150003 : Executing command failed\n",
);

#[test]
fn an_upgrade_refused_over_a_running_portable_package_is_held_naming_the_file() {
    let held =
        upgrading_shfmt().held_by(&winget_said(0x8A15_0052, WINGET_REFUSED_A_RUNNING_PORTABLE));

    let Some(HeldReason::ReportedInUse(reported)) = held else {
        panic!("expected the upgrade to be held as in use, got {held:?}");
    };
    assert!(reported.contains("shfmt.exe"), "{reported}");
}

#[test]
fn an_upgrade_winget_reports_as_blocked_by_a_running_application_is_held() {
    let held = upgrading_shfmt().held_by(&winget_said(
        0x8A15_0101,
        "Application is currently running. Exit the application then try again.\n",
    ));

    assert_eq!(
        held,
        Some(HeldReason::ReportedInUse(
            "winget exited with 0x8A150101: Application is currently running. Exit the \
             application then try again."
                .to_owned()
        ))
    );
}

#[test]
fn an_upgrade_winget_refuses_because_the_publisher_upgrades_the_package_is_held() {
    let held = upgrading_shfmt().held_by(&winget_said(
        1,
        "The package cannot be upgraded using WinGet\n",
    ));

    assert_eq!(held, Some(HeldReason::UpgradedByItsPublisher));
}

#[test]
fn a_portable_install_failing_for_any_reason_but_a_denied_file_is_not_held() {
    let held = upgrading_shfmt().held_by(&winget_said(
        0x8A15_0052,
        "Installer hash does not match; this cannot be overridden when running as admin\n",
    ));

    assert_eq!(held, None);
}

#[test]
fn a_winget_package_behind_the_latest_is_upgraded_silently_and_without_prompting() {
    assert_eq!(
        WriteInvocation::UpgradeWingetPackage { id: shfmt() }.arguments(),
        vec![
            "upgrade",
            "--id",
            "mvdan.shfmt",
            "--exact",
            "--silent",
            "--disable-interactivity",
            "--accept-package-agreements",
        ]
    );
}

fn installing_coderabbit_findings(reference: &str) -> WriteInvocation {
    WriteInvocation::InstallDartPackage {
        package: Box::new(DartPackage {
            name: crate::configuration::DartPackageName::from("coderabbit_findings"),
            source: DartSource::Git {
                url: crate::configuration::GitRemoteUrl::from(
                    "https://github.com/Alice/dotfiles.git",
                ),
                path: crate::configuration::RepositorySubdirectory::from(
                    "tools/coderabbit-findings",
                ),
                reference: crate::configuration::DartReference::from(reference),
            },
        }),
    }
}

#[test]
fn a_dart_package_is_installed_from_its_repository_directory_and_reference_over_any_earlier() {
    assert_eq!(
        installing_coderabbit_findings("main").arguments(),
        vec![
            "install",
            "coderabbit_findings@{git: {url: https://github.com/Alice/dotfiles.git, path: \
             tools/coderabbit-findings, ref: main}}",
            "--overwrite",
        ]
    );
}

#[test]
fn a_dart_install_never_waits_on_a_credential_prompt() {
    assert_eq!(
        installing_coderabbit_findings("main").environment(),
        vec![("GIT_TERMINAL_PROMPT".to_owned(), "0".to_owned())]
    );
}

// 2026-10-07: taken verbatim from `dart install dhttpd --overwrite` with dhttpd.exe running,
// under Dart 3.11.1 on Windows 11.
const DART_REFUSED_A_RUNNING_BUNDLE: &str = concat!(
    "Generated: c:\\users\\alice\\appdata\\local\\temp\\3fc21ebb\\build\\bundle\\bin\\dhttpd.exe\n",
    "Uninstalling C:\\Users\\Alice\\AppData\\Local\\Dart\\install\\app-bundles\\dhttpd\\hosted\\4.3.0\\.\n",
    "Deleting C:\\Users\\Alice\\AppData\\Local\\Dart\\install\\bin\\dhttpd.bat\n",
    "Deleting C:\\Users\\Alice\\AppData\\Local\\Dart\\install\\app-bundles\\dhttpd\\hosted\\4.3.0\\\n",
);

#[test]
fn a_dart_install_refused_by_a_running_executable_is_held_in_dart_words() {
    let output = CommandOutput {
        exited: Exited::Code(255),
        standard_output: DART_REFUSED_A_RUNNING_BUNDLE.to_owned(),
        standard_error: "Deletion failed. The application might be in use.\n".to_owned(),
    };

    assert_eq!(
        installing_coderabbit_findings("main").held_by(&output),
        Some(HeldReason::ReportedInUse(
            "Deletion failed. The application might be in use.".to_owned()
        ))
    );
}

#[test]
fn a_dart_install_failing_for_any_other_reason_is_not_held() {
    let output = CommandOutput {
        exited: Exited::Code(65),
        standard_output: String::new(),
        standard_error: "Could not find a file named \"pubspec.yaml\"\n".to_owned(),
    };

    assert_eq!(
        installing_coderabbit_findings("main").held_by(&output),
        None
    );
}
