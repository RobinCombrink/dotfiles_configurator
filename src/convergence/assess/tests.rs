use super::*;
use crate::{
    configuration::{McpScope, McpServerName},
    machine::Exited,
};

// 2026-07-31: taken verbatim from `cargo install --list`.
const LISTING: &str = concat!(
    "ci-checks v0.1.0 (C:\\Repositories\\Personal\\dotfiles\\tools\\ci-checks):\n",
    "    ci-checks.exe\n",
    "committed v1.1.11:\n",
    "    committed.exe\n",
    "stop-gate v0.1.0 (https://github.com/RobinCombrink/dotfiles",
    "?rev=2ae2ffffb580fd56b040fe7df2f2e6ad1e44c41c#2ae2ffff):\n",
    "    stop-gate.exe\n",
);

#[test]
fn a_crate_installed_from_the_registry_is_reported_as_coming_from_it_at_its_version() {
    assert_eq!(
        installed_crate_source(LISTING, "committed"),
        Some(InstalledFrom::Registry {
            version: "v1.1.11".to_owned()
        })
    );
}

fn pinned(version: &str) -> WantedVersion {
    WantedVersion::Declared(CrateVersion::try_from(version).unwrap())
}

#[test]
fn a_crate_installed_at_the_version_declared_is_converged() {
    assert_eq!(
        assess_installed_version(&pinned("27.1.0"), "v27.1.0"),
        Assessment::Converged
    );
}

#[test]
fn a_crate_installed_at_another_version_than_declared_is_drifted() {
    assert_eq!(
        assess_installed_version(&pinned("27.1.0"), "v27.0.0"),
        Assessment::Drifted(DriftReason::CargoAtAnotherVersion {
            installed: CrateVersion::try_from("27.0.0").unwrap(),
            wanted: pinned("27.1.0"),
        })
    );
}

#[test]
fn a_crate_behind_the_newest_published_is_drifted_naming_both_versions() {
    let newest = WantedVersion::NewestPublished(CrateVersion::try_from("15.1.0").unwrap());

    assert_eq!(
        assess_installed_version(&newest, "v14.1.1"),
        Assessment::Drifted(DriftReason::CargoAtAnotherVersion {
            installed: CrateVersion::try_from("14.1.1").unwrap(),
            wanted: newest.clone(),
        })
    );
}

#[test]
fn a_crate_listed_at_a_version_that_cannot_be_read_is_unassessable_rather_than_drifted() {
    let assessment = assess_installed_version(&pinned("27.1.0"), "vtwenty-seven");

    let Assessment::Unassessable(_) = assessment else {
        panic!("expected an unassessable crate, got {assessment:?}");
    };
}

#[test]
fn a_crate_installed_from_a_path_is_reported_with_that_path() {
    assert_eq!(
        installed_crate_source(LISTING, "ci-checks"),
        Some(InstalledFrom::Path(
            "C:\\Repositories\\Personal\\dotfiles\\tools\\ci-checks".to_owned()
        ))
    );
}

#[test]
fn a_crate_installed_from_git_is_reported_with_the_commit_it_resolved_to() {
    assert_eq!(
        installed_crate_source(LISTING, "stop-gate"),
        Some(InstalledFrom::Git {
            url: "https://github.com/RobinCombrink/dotfiles".to_owned(),
            commit: "2ae2ffff".to_owned(),
        })
    );
}

#[test]
fn a_crate_cargo_has_not_installed_is_absent_from_the_listing() {
    assert_eq!(installed_crate_source(LISTING, "ripgrep"), None);
}

#[test]
fn the_binaries_a_crate_installed_are_not_mistaken_for_crates_of_their_own() {
    assert_eq!(installed_crate_source(LISTING, "committed.exe"), None);
}

// 2026-08-02: taken verbatim from `winget list`, narrowed to three packages.
const PACKAGES: &str = concat!(
    "Name                 Id                        Version    Available  Source\n",
    "-----------------------------------------------------------------------------\n",
    "Battle for Wesnoth   Wesnoth.BattleForWesnoth  Unknown    1.18.4     winget\n",
    "Bitwarden            Bitwarden.Bitwarden       2026.3.1   2026.7.0   winget\n",
    "AMD Software         ARP\\Machine\\X64\\AMD Cat   26.7.1                     \n",
);

fn winget_lists_package(listing: &str, id: &str) -> Result<bool, UnreadableReason> {
    winget_row(listing, id).map(|row| row.is_some())
}

#[test]
fn a_package_winget_lists_is_found() {
    assert_eq!(
        winget_lists_package(PACKAGES, "Bitwarden.Bitwarden"),
        Ok(true)
    );
}

#[test]
fn a_package_winget_lists_is_read_at_its_version_and_the_one_winget_offers() {
    assert_eq!(
        winget_row(PACKAGES, "Bitwarden.Bitwarden"),
        Ok(Some(WingetRow {
            version: "2026.3.1".to_owned(),
            available: Some("2026.7.0".to_owned()),
        }))
    );
}

#[test]
fn a_package_winget_offers_nothing_newer_for_is_read_without_an_available_version() {
    assert_eq!(
        winget_row(PACKAGES, "ARP\\Machine\\X64\\AMD Cat"),
        Ok(Some(WingetRow {
            version: "26.7.1".to_owned(),
            available: None,
        }))
    );
}

// 2026-10-06: taken verbatim from `winget upgrade --accept-source-agreements
// --disable-interactivity`, narrowed to two packages and the lines closing the table.
const UPGRADES: &str = concat!(
    "Name                                                         Id                                     Version              Available           Source\n",
    "---------------------------------------------------------------------------------------------------------------------------------------------------\n",
    "Lefthook                                                     evilmartians.lefthook                  2.1.10               2.1.17              winget\n",
    "shfmt                                                        mvdan.shfmt                            3.13.1               3.14.1              winget\n",
    "34 upgrades available.\n",
    "2 package(s) have version numbers that cannot be determined. Use --include-unknown to see all results.\n",
);

#[test]
fn a_package_winget_would_upgrade_is_read_with_the_version_it_offers() {
    assert_eq!(
        winget_row(UPGRADES, "mvdan.shfmt"),
        Ok(Some(WingetRow {
            version: "3.13.1".to_owned(),
            available: Some("3.14.1".to_owned()),
        }))
    );
}

#[test]
fn the_lines_closing_an_upgrade_listing_are_not_read_as_packages() {
    assert_eq!(winget_row(UPGRADES, "jqlang.jq"), Ok(None));
}

fn declared(version: &str) -> WingetVersion {
    WingetVersion::from(version)
}

#[test]
fn a_package_at_the_version_declared_is_converged() {
    assert_eq!(
        assess_winget_version(&declared("2025.1.2.11"), "2025.1.2.11"),
        Assessment::Converged
    );
}

#[test]
fn a_package_newer_than_the_version_declared_is_drifted() {
    assert_eq!(
        assess_winget_version(&declared("3.13.1"), "3.14.1"),
        Assessment::Drifted(DriftReason::WingetAtAnotherVersion {
            installed: "3.14.1".to_owned(),
            declared: declared("3.13.1"),
        })
    );
}

#[test]
fn a_package_winget_knows_no_one_version_of_is_unassessable_rather_than_drifted() {
    let Assessment::Unassessable(_) = assess_winget_version(&declared("17.14.41"), "< 17.14.37")
    else {
        panic!("a version winget gives only a bound for was read as one version");
    };
}

#[test]
fn a_package_winget_does_not_list_is_not_found() {
    assert_eq!(
        winget_lists_package(PACKAGES, "Microsoft.PowerShell"),
        Ok(false)
    );
}

#[test]
fn an_identifier_holding_spaces_is_read_whole_rather_than_cut_at_the_first_one() {
    assert_eq!(
        winget_lists_package(PACKAGES, "ARP\\Machine\\X64\\AMD Cat"),
        Ok(true)
    );
}

#[test]
fn a_package_named_after_another_packages_identifier_is_not_mistaken_for_it() {
    let listing = concat!(
        "Name                  Id             Version  Available  Source\n",
        "---------------------------------------------------------------\n",
        "Bitwarden.Bitwarden   Some.Other.Id  1.0.0                winget\n",
    );

    assert_eq!(
        winget_lists_package(listing, "Bitwarden.Bitwarden"),
        Ok(false)
    );
}

#[test]
fn a_name_that_is_not_ascii_does_not_shift_the_column_the_identifier_is_read_from() {
    let listing = concat!(
        "Name          Id                   Version  Available  Source\n",
        "-------------------------------------------------------------\n",
        "Café Münchén  Bitwarden.Bitwarden  1.0.0               winget\n",
    );

    assert_eq!(
        winget_lists_package(listing, "Bitwarden.Bitwarden"),
        Ok(true)
    );
}

#[test]
fn a_listing_that_cut_an_identifier_short_is_refused_rather_than_read_as_an_absent_package() {
    let listing = concat!(
        "Name        Id             Version  Available  Source\n",
        "-----------------------------------------------------\n",
        "Bitwarden   Bitwarden.Bi…  2026.3.1            winget\n",
    );

    assert!(winget_lists_package(listing, "Bitwarden.Bitwarden").is_err());
}

#[test]
fn a_listing_whose_columns_cannot_be_located_is_refused() {
    assert!(winget_lists_package("no columns here\n", "Bitwarden.Bitwarden").is_err());
}

// 2026-09-13: taken verbatim from `claude mcp get probe-server` against a stdio server
// registered for the measurement, on Claude Code 2.1.270 under Windows 11.
const REGISTERED: &str = concat!(
    "probe-server:\n",
    "  Scope: Local config (private to you in this project)\n",
    "  Status: \u{2718} Failed to connect\n",
    "  Issue: CONNECTION_CLOSED: Connection closed\n",
    "  Type: stdio\n",
    "  Command: my-program\n",
    "  Args: start --port 1234\n",
    "  Environment:\n",
    "    ALPHA=one\n",
    "    BETA=two\n",
    "\n",
    "To remove this server, run: claude mcp remove probe-server -s local\n",
);

fn probe_server() -> ClaudeMcpServer {
    ClaudeMcpServer {
        name: McpServerName::from("probe-server"),
        scope: McpScope::Local,
        command: "my-program".to_owned(),
        args: vec!["start".to_owned(), "--port".to_owned(), "1234".to_owned()],
        environment: BTreeMap::from([
            ("ALPHA".to_owned(), "one".to_owned()),
            ("BETA".to_owned(), "two".to_owned()),
        ]),
    }
}

#[test]
fn a_registration_holding_everything_that_was_declared_has_not_drifted() {
    assert_eq!(first_difference(&probe_server(), REGISTERED), None);
}

#[test]
fn a_registration_running_another_program_drifts_and_names_the_one_it_runs() {
    let declared = ClaudeMcpServer {
        command: "another-program".to_owned(),
        ..probe_server()
    };

    assert_eq!(
        first_difference(&declared, REGISTERED),
        Some(ServerDifference::RunsAnotherCommand(
            "my-program".to_owned()
        ))
    );
}

#[test]
fn a_registration_started_with_other_arguments_drifts_and_names_the_ones_it_holds() {
    let declared = ClaudeMcpServer {
        args: vec!["start".to_owned(), "--port".to_owned(), "9999".to_owned()],
        ..probe_server()
    };

    assert_eq!(
        first_difference(&declared, REGISTERED),
        Some(ServerDifference::OtherArguments(
            "start --port 1234".to_owned()
        ))
    );
}

#[test]
fn a_registration_missing_a_declared_environment_entry_drifts_and_names_the_variable() {
    let mut environment = probe_server().environment;
    environment.insert("GAMMA".to_owned(), "three".to_owned());
    let declared = ClaudeMcpServer {
        environment,
        ..probe_server()
    };

    assert_eq!(
        first_difference(&declared, REGISTERED),
        Some(ServerDifference::EnvironmentUnset {
            key: "GAMMA".to_owned()
        })
    );
}

#[test]
fn a_registration_holding_a_declared_variable_at_another_value_drifts() {
    let mut environment = probe_server().environment;
    environment.insert("ALPHA".to_owned(), "something else".to_owned());
    let declared = ClaudeMcpServer {
        environment,
        ..probe_server()
    };

    assert_eq!(
        first_difference(&declared, REGISTERED),
        Some(ServerDifference::EnvironmentUnset {
            key: "ALPHA".to_owned()
        })
    );
}

#[test]
fn a_registration_reporting_no_arguments_for_a_server_declaring_some_names_that_rather_than_nothing()
 {
    let reported = "probe-server:\n  Type: stdio\n  Command: my-program\n";

    assert_eq!(
        first_difference(&probe_server(), reported),
        Some(ServerDifference::NoArguments)
    );
}

#[test]
fn a_registration_reporting_no_arguments_for_a_server_declaring_none_has_not_drifted() {
    let declared = ClaudeMcpServer {
        args: Vec::new(),
        environment: BTreeMap::new(),
        ..probe_server()
    };
    let reported = "probe-server:\n  Type: stdio\n  Command: my-program\n";

    assert_eq!(first_difference(&declared, reported), None);
}

#[test]
fn a_report_naming_no_command_at_all_drifts_rather_than_reading_as_no_command_declared() {
    let reported = "probe-server:\n  Scope: Local config\n  Type: http\n";

    assert_eq!(
        first_difference(&probe_server(), reported),
        Some(ServerDifference::ReportsNoCommand)
    );
}

fn claude_exiting_one_saying(standard_error: &str) -> CommandOutput {
    CommandOutput {
        exited: Exited::Code(1),
        standard_output: String::new(),
        standard_error: standard_error.to_owned(),
    }
}

#[test]
fn a_claude_that_reports_no_server_under_the_name_has_drifted() {
    let refusal = "No MCP server named \"probe-server\". Configured servers: serena, github";

    assert_eq!(
        claude_refusal(&claude_exiting_one_saying(refusal)),
        Assessment::Drifted(DriftReason::NoSuchServer)
    );
}

#[test]
fn a_claude_that_failed_for_any_other_reason_is_unassessable_rather_than_drifted() {
    assert_eq!(
        claude_refusal(&claude_exiting_one_saying(
            "Invalid API key · Please run /login"
        )),
        Assessment::Unassessable(Impediment::ActualStateUnreadable(
            "claude could not be read, exited with 1: Invalid API key · Please run /login".into()
        ))
    );
}

// 2026-09-25: taken verbatim from `uv tool list` and `uv tool list --outdated` under uv
// 0.10.12 on Windows 11, with a second tool added to the first.
const UV_TOOLS: &str = concat!(
    "cowsay v6.1\n",
    "- cowsay\n",
    "serena-agent v1.5.3\n",
    "- serena\n",
    "- serena-agent\n",
    "- serena-hooks\n",
);
const UV_OUTDATED_TOOLS: &str = concat!(
    "serena-agent v1.5.3 [latest: 1.7.0]\n",
    "- serena\n",
    "- serena-agent\n",
    "- serena-hooks\n",
);

fn serena() -> UvToolName {
    UvToolName::from("serena-agent")
}

#[test]
fn a_tool_uv_lists_is_read_at_the_version_it_is_installed_at() {
    assert_eq!(
        listed_uv_tool(UV_TOOLS, &serena()),
        Ok(Some((UvToolVersion::from("1.5.3"), None)))
    );
}

#[test]
fn a_tool_uv_does_not_list_is_not_installed() {
    assert_eq!(
        listed_uv_tool(UV_TOOLS, &UvToolName::from("ruff")),
        Ok(None)
    );
}

#[test]
fn an_executable_named_after_a_tool_is_not_mistaken_for_the_tool() {
    let listing = "serena-agent v1.5.3\n- serena\n";

    assert_eq!(
        listed_uv_tool(listing, &UvToolName::from("serena")),
        Ok(None)
    );
}

#[test]
fn a_tool_uv_lists_as_behind_is_read_with_the_newest_version_that_resolves() {
    assert_eq!(
        listed_uv_tool(UV_OUTDATED_TOOLS, &serena()),
        Ok(Some((
            UvToolVersion::from("1.5.3"),
            Some(UvToolVersion::from("1.7.0"))
        )))
    );
}

#[test]
fn a_tool_is_found_under_the_name_uv_normalises_it_to() {
    assert!(
        listed_uv_tool(UV_TOOLS, &UvToolName::from("Serena_Agent"))
            .expect("a readable listing")
            .is_some()
    );
}

#[test]
fn a_tool_listed_without_a_version_is_refused_rather_than_read_as_absent() {
    assert!(listed_uv_tool("serena-agent\n- serena\n", &serena()).is_err());
}

#[test]
fn a_tool_listed_as_behind_nothing_it_names_is_refused_rather_than_read_as_current() {
    assert!(listed_uv_tool("serena-agent v1.5.3 [latest: ]\n", &serena()).is_err());
}

// 2026-10-07: shaped as `uv tool list --show-version-specifiers` printed `black v24.1.0
// [required: ==24.1.0]` for a pinned tool, under uv 0.10.12 on Windows 11.
const UV_TOOLS_ONE_PINNED: &str = concat!(
    "cowsay v6.1\n",
    "- cowsay\n",
    "serena-agent v1.5.3 [required: ==1.5.3]\n",
    "- serena\n",
);

#[test]
fn a_tool_uv_lists_with_an_exact_requirement_is_pinned() {
    assert!(uv_tool_is_pinned_in(UV_TOOLS_ONE_PINNED, &serena()));
}

#[test]
fn a_tool_uv_lists_without_a_requirement_is_not_pinned() {
    assert!(!uv_tool_is_pinned_in(
        UV_TOOLS_ONE_PINNED,
        &UvToolName::from("cowsay")
    ));
}

#[test]
fn a_pinned_tool_is_still_read_at_the_version_it_is_installed_at() {
    assert_eq!(
        listed_uv_tool(UV_TOOLS_ONE_PINNED, &serena()),
        Ok(Some((UvToolVersion::from("1.5.3"), None)))
    );
}
