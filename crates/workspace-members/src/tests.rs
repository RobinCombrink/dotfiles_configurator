use {super::*, std::convert::Infallible};

fn tools_holding(
    children: &'static [&'static str],
) -> impl Fn(&str) -> Result<Vec<String>, Infallible> {
    move |parent| {
        Ok(match parent {
            "tools" => children.iter().map(|child| (*child).to_owned()).collect(),
            _ => Vec::new(),
        })
    }
}

fn workspace_of(members: &str) -> String {
    format!("[workspace]\nresolver = \"2\"\nmembers = [{members}]\n")
}

fn refusal_of(members: &str) -> MembersRefusal<Infallible> {
    members_of(&workspace_of(members), tools_holding(&["alpha"])).unwrap_err()
}

#[test]
fn every_directory_under_a_parent_expands_into_its_children_in_sorted_order() {
    let members = members_of(
        &workspace_of("\"tools/*\""),
        tools_holding(&["gamma", "alpha", "beta"]),
    )
    .unwrap();

    assert_eq!(members, vec!["tools/alpha", "tools/beta", "tools/gamma"]);
}

#[test]
fn a_member_named_outright_passes_through_unchanged() {
    let members = members_of(
        &workspace_of("\"tools/stop-gate\", \"crates/lock\""),
        tools_holding(&["alpha"]),
    )
    .unwrap();

    assert_eq!(members, vec!["tools/stop-gate", "crates/lock"]);
}

#[test]
fn every_directory_under_a_parent_expands_only_to_the_children_the_lister_returns() {
    let members = members_of(
        &workspace_of("\"tools/*\", \"crates/*\""),
        tools_holding(&["alpha"]),
    )
    .unwrap();

    assert_eq!(members, vec!["tools/alpha"]);
}

#[test]
fn a_glob_followed_by_a_further_path_is_refused_naming_the_member() {
    let refusal = refusal_of("\"tools/*/x\"");

    let MembersRefusal::MemberIsAGlob(member) = refusal else {
        panic!("expected a glob refusal, got {refusal:?}");
    };
    assert_eq!(member, "tools/*/x");
}

#[test]
fn a_glob_inside_a_directory_name_is_refused_naming_the_member() {
    let refusal = refusal_of("\"tools/a*\"");

    let MembersRefusal::MemberIsAGlob(member) = refusal else {
        panic!("expected a glob refusal, got {refusal:?}");
    };
    assert_eq!(member, "tools/a*");
}

#[test]
fn a_glob_in_the_parent_of_every_directory_is_refused_naming_the_member() {
    let refusal = refusal_of("\"t?ols/*\"");

    let MembersRefusal::MemberIsAGlob(member) = refusal else {
        panic!("expected a glob refusal, got {refusal:?}");
    };
    assert_eq!(member, "t?ols/*");
}

#[test]
fn a_character_class_in_the_parent_is_refused_naming_the_member() {
    let refusal = refusal_of("\"[t]ools/*\"");

    let MembersRefusal::MemberIsAGlob(member) = refusal else {
        panic!("expected a glob refusal, got {refusal:?}");
    };
    assert_eq!(member, "[t]ools/*");
}

#[test]
fn a_manifest_declaring_no_workspace_is_refused() {
    let refusal = members_of(
        "[package]\nname = \"stop-gate\"\n",
        tools_holding(&["alpha"]),
    )
    .unwrap_err();

    assert!(
        matches!(refusal, MembersRefusal::NoWorkspace),
        "expected no workspace, got {refusal:?}"
    );
}

#[test]
fn a_manifest_that_is_not_toml_is_refused() {
    let refusal = members_of("[workspace", tools_holding(&["alpha"])).unwrap_err();

    assert!(
        matches!(refusal, MembersRefusal::ManifestUnreadable(_)),
        "expected an unreadable manifest, got {refusal:?}"
    );
}

#[test]
fn a_parent_whose_children_cannot_be_listed_is_refused_rather_than_read_as_empty() {
    let refusal = members_of(&workspace_of("\"tools/*\""), |_: &str| {
        Err::<Vec<String>, _>("unreadable")
    })
    .unwrap_err();

    let MembersRefusal::ChildrenUnlisted { parent, cause } = refusal else {
        panic!("expected a listing refusal, got {refusal:?}");
    };
    assert_eq!((parent.as_str(), cause), ("tools", "unreadable"));
}
