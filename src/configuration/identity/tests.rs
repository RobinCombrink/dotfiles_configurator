use super::*;

#[test]
fn two_link_paths_spelled_in_different_case_resolve_to_the_same_identity() {
    let home_directory = Path::new("C:\\Users\\Alice");

    assert_eq!(
        LinkPath::under_home(home_directory, Path::new(".gitconfig")),
        LinkPath::under_home(home_directory, Path::new(".GITCONFIG"))
    );
}

#[test]
fn two_link_paths_spelled_with_different_separators_resolve_to_the_same_identity() {
    let home_directory = Path::new("C:\\Users\\Alice");

    assert_eq!(
        LinkPath::under_home(home_directory, Path::new("config/git/config")),
        LinkPath::under_home(home_directory, Path::new("config\\git\\config"))
    );
}

#[test]
fn two_link_paths_naming_different_files_resolve_to_different_identities() {
    let home_directory = Path::new("C:\\Users\\Alice");

    assert_ne!(
        LinkPath::under_home(home_directory, Path::new(".gitconfig")),
        LinkPath::under_home(home_directory, Path::new(".npmrc"))
    );
}

#[test]
fn an_absolute_link_path_is_resolved_without_the_home_directory() {
    let absolute = if cfg!(windows) {
        "C:\\elsewhere\\.gitconfig"
    } else {
        "/elsewhere/.gitconfig"
    };
    let declared = Path::new(absolute);

    assert_eq!(
        LinkPath::under_home(Path::new("C:\\Users\\Alice"), declared),
        LinkPath::under_home(Path::new("C:\\Users\\Bob"), declared)
    );
}
