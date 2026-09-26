use {
    dotfiles_configurator::configuration::Configuration,
    std::{env, fs, path::PathBuf},
};

const UPDATE_SCHEMA: &str = "UPDATE_SCHEMA";

fn committed_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join("configuration_schema.json")
}

fn rendered_schema() -> String {
    let schema = schemars::schema_for!(Configuration);
    let mut rendered =
        serde_json::to_string_pretty(&schema).expect("a schema always renders as JSON");
    rendered.push('\n');
    rendered
}

#[test]
fn the_committed_schema_is_the_one_the_configuration_types_render() {
    let path = committed_schema_path();
    let rendered = rendered_schema();

    if env::var_os(UPDATE_SCHEMA).is_some_and(|value| value == "1") {
        if let Some(schema_directory) = path.parent() {
            fs::create_dir_all(schema_directory).expect("the schema directory is creatable");
        }
        fs::write(&path, rendered).expect("the committed schema is writable");
        return;
    }

    let committed = fs::read_to_string(&path).expect("the committed schema is readable");
    assert!(
        committed == rendered,
        "{} differs from what the configuration types render; regenerate it by setting the \
         environment variable {UPDATE_SCHEMA} to 1 and running `cargo test --test schema`, then \
         commit the result",
        path.display()
    );
}
