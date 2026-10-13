use {
    dotfiles_configurator::configuration::Configuration,
    std::{env, fs, path::PathBuf},
};

const UPDATE_SCHEMA: &str = "UPDATE_SCHEMA";

fn committed_schema_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join(file)
}

fn rendered<Schema: schemars::JsonSchema>() -> String {
    let schema = schemars::schema_for!(Schema);
    let mut rendered =
        serde_json::to_string_pretty(&schema).expect("a schema always renders as JSON");
    rendered.push('\n');
    rendered
}

#[test]
fn the_committed_schema_is_the_one_the_configuration_types_render() {
    let path = committed_schema_path("configuration_schema.json");
    let rendered = rendered::<Configuration>();

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

#[test]
fn the_committed_machine_schema_is_the_one_the_manifest_it_writes_renders() {
    let path = committed_schema_path("machine.schema.json");

    let committed = fs::read_to_string(&path).expect("the committed machine schema is readable");

    assert!(
        committed == rendered::<machine_manifest_document::Document>(),
        "{} differs from what the machine manifest renders",
        path.display()
    );
}
