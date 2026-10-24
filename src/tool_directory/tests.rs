use super::*;

#[test]
fn the_tool_directory_sits_directly_under_the_home_directory() {
    assert_eq!(
        tool_directory_within(Path::new("home")),
        Path::new("home").join(".dotfiles_configurator")
    );
}
