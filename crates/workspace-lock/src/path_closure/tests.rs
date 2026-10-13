use {super::*, std::collections::BTreeMap};

const ROOT: &str = "[workspace]\nmembers = [\"tools/alpha\", \"tools/beta\"]\n";

fn package(name: &str, sections: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n{sections}")
}

fn repository(manifests: &[(&str, String)]) -> BTreeMap<String, String> {
    manifests
        .iter()
        .map(|(directory, text)| ((*directory).to_owned(), text.clone()))
        .collect()
}

fn lookup_in(
    manifests: &BTreeMap<String, String>,
) -> impl Fn(&RepositoryDirectory) -> Option<String> {
    |directory| manifests.get(directory.as_str()).cloned()
}

fn spelled(closure: &BTreeSet<RepositoryDirectory>) -> Vec<&str> {
    closure.iter().map(RepositoryDirectory::as_str).collect()
}

fn build_closure_of_alpha(root: &str, manifests: &[(&str, String)]) -> Vec<String> {
    let manifests = repository(manifests);
    match BuildClosure::of(root, "tools/alpha", lookup_in(&manifests)) {
        Ok(closure) => spelled(closure.directories())
            .into_iter()
            .map(str::to_owned)
            .collect(),
        Err(refusal) => panic!("expected a build closure, got: {refusal}"),
    }
}

fn test_closure_of_alpha(root: &str, manifests: &[(&str, String)]) -> Vec<String> {
    let manifests = repository(manifests);
    match TestClosure::of(root, "tools/alpha", lookup_in(&manifests)) {
        Ok(closure) => spelled(closure.directories())
            .into_iter()
            .map(str::to_owned)
            .collect(),
        Err(refusal) => panic!("expected a test closure, got: {refusal}"),
    }
}

fn alpha_on_beta_through(sections: &str) -> Vec<(&'static str, String)> {
    vec![
        ("tools/alpha", package("alpha", sections)),
        ("tools/beta", package("beta", "")),
    ]
}

#[test]
fn the_build_closure_reaches_a_crate_depended_on_to_run() {
    assert_eq!(
        build_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through("[dependencies]\nbeta = { path = \"../beta\" }\n")
        ),
        vec!["tools/beta"]
    );
}

#[test]
fn the_build_closure_reaches_a_crate_depended_on_to_build() {
    assert_eq!(
        build_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through("[build-dependencies]\nbeta = { path = \"../beta\" }\n")
        ),
        vec!["tools/beta"]
    );
}

#[test]
fn the_build_closure_reaches_a_crate_depended_on_for_one_platform() {
    assert_eq!(
        build_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through(
                "[target.'cfg(windows)'.dependencies]\nbeta = { path = \"../beta\" }\n"
            )
        ),
        vec!["tools/beta"]
    );
}

#[test]
fn the_build_closure_reaches_a_crate_depended_on_to_build_for_one_platform() {
    assert_eq!(
        build_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through(
                "[target.'cfg(unix)'.build-dependencies]\nbeta = { path = \"../beta\" }\n"
            )
        ),
        vec!["tools/beta"]
    );
}

#[test]
fn the_build_closure_leaves_out_a_crate_only_the_tests_depend_on() {
    assert!(
        build_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through("[dev-dependencies]\nbeta = { path = \"../beta\" }\n")
        )
        .is_empty()
    );
}

#[test]
fn the_build_closure_leaves_out_a_crate_only_the_tests_depend_on_for_one_platform() {
    assert!(
        build_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through(
                "[target.'cfg(windows)'.dev-dependencies]\nbeta = { path = \"../beta\" }\n"
            )
        )
        .is_empty()
    );
}

#[test]
fn the_test_closure_reaches_a_crate_only_the_tests_depend_on() {
    assert_eq!(
        test_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through("[dev-dependencies]\nbeta = { path = \"../beta\" }\n")
        ),
        vec!["tools/beta"]
    );
}

#[test]
fn the_test_closure_reaches_a_crate_only_the_tests_depend_on_for_one_platform() {
    assert_eq!(
        test_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through(
                "[target.'cfg(windows)'.dev-dependencies]\nbeta = { path = \"../beta\" }\n"
            )
        ),
        vec!["tools/beta"]
    );
}

#[test]
fn the_test_closure_reaches_every_crate_the_build_closure_reaches() {
    let alpha = package(
        "alpha",
        "[dependencies]\nbeta = { path = \"../beta\" }\n\n\
         [build-dependencies]\ngamma = { path = \"../gamma\" }\n\n\
         [target.'cfg(windows)'.dependencies]\ndelta = { path = \"../delta\" }\n",
    );
    let manifests = vec![
        ("tools/alpha", alpha),
        ("tools/beta", package("beta", "")),
        ("tools/gamma", package("gamma", "")),
        ("tools/delta", package("delta", "")),
    ];

    assert_eq!(
        test_closure_of_alpha(ROOT, &manifests),
        vec!["tools/beta", "tools/delta", "tools/gamma"]
    );
}

fn root_declaring(workspace_dependencies: &str) -> String {
    format!("{ROOT}\n[workspace.dependencies]\n{workspace_dependencies}")
}

#[test]
fn an_inherited_dependency_resolves_its_path_against_the_repository_root() {
    assert_eq!(
        build_closure_of_alpha(
            &root_declaring("beta = { path = \"tools/beta\" }\n"),
            &alpha_on_beta_through("[dependencies]\nbeta = { workspace = true }\n")
        ),
        vec!["tools/beta"]
    );
}

#[test]
fn an_inherited_dependency_only_the_tests_take_joins_the_test_closure() {
    assert_eq!(
        test_closure_of_alpha(
            &root_declaring("beta = { path = \"tools/beta\" }\n"),
            &alpha_on_beta_through("[dev-dependencies]\nbeta.workspace = true\n")
        ),
        vec!["tools/beta"]
    );
}

#[test]
fn an_inherited_dependency_the_workspace_declares_without_a_path_adds_nothing() {
    assert!(
        build_closure_of_alpha(
            &root_declaring("beta = { version = \"1.0.0\" }\n"),
            &alpha_on_beta_through("[dependencies]\nbeta = { workspace = true }\n")
        )
        .is_empty()
    );
}

#[test]
fn an_inherited_dependency_the_workspace_does_not_hold_adds_nothing() {
    assert!(
        build_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through("[dependencies]\nbeta = { workspace = true }\n")
        )
        .is_empty()
    );
}

#[test]
fn a_dependency_named_by_version_alone_adds_nothing() {
    assert!(
        build_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through("[dependencies]\nbeta = \"1.0.0\"\n")
        )
        .is_empty()
    );
}

#[test]
fn a_path_dependency_outside_the_workspace_is_walked_into_its_own_path_dependencies() {
    let manifests = vec![
        (
            "tools/alpha",
            package(
                "alpha",
                "[dependencies]\npad = { path = \"../../vendor/pad\" }\n",
            ),
        ),
        (
            "vendor/pad",
            package(
                "pad",
                "[dependencies]\npad-core = { path = \"../pad-core\" }\n",
            ),
        ),
        ("vendor/pad-core", package("pad-core", "")),
    ];

    assert_eq!(
        build_closure_of_alpha(ROOT, &manifests),
        vec!["vendor/pad", "vendor/pad-core"]
    );
}

#[test]
fn a_closure_spells_every_directory_with_forward_slashes_and_no_dot_segments() {
    assert_eq!(
        build_closure_of_alpha(
            ROOT,
            &alpha_on_beta_through(
                "[dependencies]\nbeta = { path = \"..\\\\gamma\\\\..\\\\./beta/\" }\n"
            )
        ),
        vec!["tools/beta"]
    );
}

#[test]
fn a_cycle_of_path_dependencies_terminates_and_leaves_out_the_member_itself() {
    let manifests = vec![
        (
            "tools/alpha",
            package("alpha", "[dependencies]\nbeta = { path = \"../beta\" }\n"),
        ),
        (
            "tools/beta",
            package(
                "beta",
                "[dependencies]\nalpha = { path = \"../alpha\" }\ngamma = { path = \"../gamma\" }\n",
            ),
        ),
        (
            "tools/gamma",
            package("gamma", "[dependencies]\nbeta = { path = \"../beta\" }\n"),
        ),
    ];

    assert_eq!(
        build_closure_of_alpha(ROOT, &manifests),
        vec!["tools/beta", "tools/gamma"]
    );
}

fn build_refusal_of_alpha(root: &str, manifests: &[(&str, String)]) -> PathClosureRefusal {
    let manifests = repository(manifests);
    match BuildClosure::of(root, "tools/alpha", lookup_in(&manifests)) {
        Ok(closure) => panic!("expected a refusal, got: {closure:?}"),
        Err(refusal) => refusal,
    }
}

fn path_refused_as_outside(refusal: PathClosureRefusal) -> (String, String) {
    let PathClosureRefusal::OutsideTheRepository { declared_in, path } = refusal else {
        panic!("expected a refusal naming a path outside the repository, got: {refusal}");
    };
    (declared_in.as_str().to_owned(), path)
}

#[test]
fn a_path_dependency_leaving_the_repository_refuses_the_closure_naming_the_path() {
    let refusal = build_refusal_of_alpha(
        ROOT,
        &alpha_on_beta_through("[dependencies]\nelsewhere = { path = \"../../../elsewhere\" }\n"),
    );

    assert_eq!(
        path_refused_as_outside(refusal),
        ("tools/alpha".to_owned(), "../../../elsewhere".to_owned())
    );
}

#[test]
fn an_absolute_path_dependency_refuses_the_closure_naming_the_path() {
    let refusal = build_refusal_of_alpha(
        ROOT,
        &alpha_on_beta_through("[dependencies]\nelsewhere = { path = \"/opt/elsewhere\" }\n"),
    );

    assert_eq!(
        path_refused_as_outside(refusal),
        ("tools/alpha".to_owned(), "/opt/elsewhere".to_owned())
    );
}

#[test]
fn a_path_with_a_drive_refuses_the_closure_naming_the_path() {
    let refusal = build_refusal_of_alpha(
        ROOT,
        &alpha_on_beta_through("[dependencies]\nelsewhere = { path = \"D:/elsewhere\" }\n"),
    );

    assert_eq!(
        path_refused_as_outside(refusal),
        ("tools/alpha".to_owned(), "D:/elsewhere".to_owned())
    );
}

#[test]
fn an_inherited_path_leaving_the_repository_refuses_the_closure_naming_the_path() {
    let refusal = build_refusal_of_alpha(
        &root_declaring("beta = { path = \"../beta\" }\n"),
        &alpha_on_beta_through("[dependencies]\nbeta = { workspace = true }\n"),
    );

    assert_eq!(
        path_refused_as_outside(refusal),
        (String::new(), "../beta".to_owned())
    );
}

#[test]
fn a_directory_the_lookup_cannot_answer_refuses_the_closure_naming_it() {
    let refusal = build_refusal_of_alpha(
        ROOT,
        &[(
            "tools/alpha",
            package("alpha", "[dependencies]\nbeta = { path = \"../beta\" }\n"),
        )],
    );

    let PathClosureRefusal::Unanswered(directory) = refusal else {
        panic!("expected a refusal naming the unanswered directory, got: {refusal}");
    };
    assert_eq!(directory.as_str(), "tools/beta");
}

#[test]
fn a_member_the_lookup_cannot_answer_refuses_the_closure_naming_it() {
    let refusal = build_refusal_of_alpha(ROOT, &[]);

    let PathClosureRefusal::Unanswered(directory) = refusal else {
        panic!("expected a refusal naming the unanswered member, got: {refusal}");
    };
    assert_eq!(directory.as_str(), "tools/alpha");
}

fn directory_refused_as_unparsable(refusal: PathClosureRefusal) -> String {
    let PathClosureRefusal::Unparsable { directory, .. } = refusal else {
        panic!("expected a refusal naming the unparsable manifest, got: {refusal}");
    };
    directory.as_str().to_owned()
}

#[test]
fn a_reached_manifest_that_does_not_parse_refuses_the_closure_naming_it() {
    let refusal = build_refusal_of_alpha(
        ROOT,
        &[
            (
                "tools/alpha",
                package("alpha", "[dependencies]\nbeta = { path = \"../beta\" }\n"),
            ),
            ("tools/beta", "[package\nname = ".to_owned()),
        ],
    );

    assert_eq!(directory_refused_as_unparsable(refusal), "tools/beta");
}

#[test]
fn a_root_manifest_that_does_not_parse_refuses_the_closure_naming_it() {
    let refusal = build_refusal_of_alpha("[workspace", &alpha_on_beta_through(""));

    assert_eq!(directory_refused_as_unparsable(refusal), "");
}

#[test]
fn a_refusal_names_the_manifest_it_could_not_read() {
    let refusal = build_refusal_of_alpha(ROOT, &[]);

    assert_eq!(
        refusal.to_string(),
        "tools/alpha/Cargo.toml, which the closure reaches, could not be read"
    );
}
