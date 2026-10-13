use super::*;

#[test]
fn a_workspace_manifest_lists_the_paths_of_its_members() {
    let manifest = r#"
        [workspace]
        resolver = "2"
        members = ["tools/stop-gate", "tools/ci-checks"]
    "#;

    assert_eq!(
        member_paths(manifest).unwrap(),
        vec!["tools/stop-gate".to_owned(), "tools/ci-checks".to_owned()]
    );
}

#[test]
fn a_member_path_holding_a_glob_is_refused_by_name() {
    let manifest = r#"
        [workspace]
        members = ["tools/*"]
    "#;

    let error = member_paths(manifest).unwrap_err().to_string();

    assert!(
        error.contains("tools/*"),
        "expected the message to name the entry, got: {error}"
    );
}

#[test]
fn a_manifest_that_declares_no_workspace_is_refused() {
    let manifest = r#"
        [package]
        name = "stop-gate"
    "#;

    assert!(member_paths(manifest).is_err());
}

#[test]
fn a_member_is_known_by_the_name_its_own_manifest_gives_it() {
    let manifest = r#"
        [package]
        name = "session-mining"
        version = "0.1.0"
    "#;

    assert_eq!(
        read_member_manifest(manifest).unwrap().name,
        CrateName::from("session-mining")
    );
}

fn binaries_of(manifest: &str, tree: &MemberTree) -> Vec<String> {
    read_member_manifest(manifest)
        .unwrap()
        .binaries(tree)
        .iter()
        .map(BinaryName::to_string)
        .collect()
}

fn under_binaries_directory(stems: &[&str]) -> MemberTree {
    MemberTree {
        holds_a_main_file: false,
        inferable_binaries: stems
            .iter()
            .map(|stem| InferableBinary {
                path: format!("src/bin/{stem}.rs"),
                name: BinaryName::from(*stem),
            })
            .collect(),
    }
}

#[test]
fn a_member_is_known_by_the_binaries_its_manifest_names() {
    let manifest = r#"
        [package]
        name = "session-mining"

        [[bin]]
        name = "session-census"
        path = "src/bin/session_census.rs"
    "#;

    assert_eq!(
        binaries_of(manifest, &MemberTree::default()),
        vec!["session-census".to_owned()]
    );
}

#[test]
fn a_member_holding_a_main_file_builds_a_binary_named_for_the_package() {
    let manifest = r#"
        [package]
        name = "stop-gate"
    "#;
    let tree = MemberTree {
        holds_a_main_file: true,
        inferable_binaries: Vec::new(),
    };

    assert_eq!(binaries_of(manifest, &tree), vec!["stop-gate".to_owned()]);
}

#[test]
fn a_member_builds_a_binary_for_every_file_under_its_binaries_directory() {
    let manifest = r#"
        [package]
        name = "session-mining"
    "#;

    assert_eq!(
        binaries_of(
            manifest,
            &under_binaries_directory(&["sweep-status", "reach"])
        ),
        vec!["reach".to_owned(), "sweep-status".to_owned()]
    );
}

#[test]
fn a_file_a_binary_section_already_claims_does_not_also_build_one_named_for_its_stem() {
    let manifest = r#"
        [package]
        name = "session-mining"

        [[bin]]
        name = "tool-use-statistics"
        path = "src/bin/tool_use_statistics.rs"
    "#;

    assert_eq!(
        binaries_of(
            manifest,
            &under_binaries_directory(&["tool_use_statistics"])
        ),
        vec!["tool-use-statistics".to_owned()]
    );
}

#[test]
fn a_main_file_a_binary_section_already_claims_does_not_also_build_one_named_for_the_package() {
    let manifest = r#"
        [package]
        name = "claude-session"

        [[bin]]
        name = "claude-live-set"
        path = "src/main.rs"
    "#;
    let tree = MemberTree {
        holds_a_main_file: true,
        inferable_binaries: Vec::new(),
    };

    assert_eq!(
        binaries_of(manifest, &tree),
        vec!["claude-live-set".to_owned()]
    );
}

#[test]
fn a_binary_section_naming_no_path_does_not_double_the_file_it_infers() {
    let manifest = r#"
        [package]
        name = "session-mining"

        [[bin]]
        name = "sweep"
    "#;

    assert_eq!(
        binaries_of(manifest, &under_binaries_directory(&["sweep"])),
        vec!["sweep".to_owned()]
    );
}

#[test]
fn a_member_holding_only_a_library_builds_no_binary() {
    let manifest = r#"
        [package]
        name = "shared"

        [lib]
        path = "src/lib.rs"
    "#;

    assert!(binaries_of(manifest, &MemberTree::default()).is_empty());
}

#[test]
fn a_fingerprint_matching_another_reports_no_difference() {
    let fingerprint = Fingerprint {
        crate_subtree: ObjectHash::from("aaa"),
        workspace_bindings: BTreeMap::new(),
        lock_closure: ObjectHash::from("ccc"),
        dependency_subtrees: BTreeMap::new(),
    };

    assert_eq!(fingerprint.difference_from(&fingerprint), None);
}

#[test]
fn a_fingerprint_differing_only_in_its_lock_closure_names_the_dependencies() {
    let installed = Fingerprint {
        crate_subtree: ObjectHash::from("aaa"),
        workspace_bindings: BTreeMap::new(),
        lock_closure: ObjectHash::from("ccc"),
        dependency_subtrees: BTreeMap::new(),
    };
    let desired = Fingerprint {
        lock_closure: ObjectHash::from("ddd"),
        ..installed.clone()
    };

    assert_eq!(
        desired.difference_from(&installed),
        Some("its dependencies have changed".to_owned())
    );
}
