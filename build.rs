use {
    common::Configuration, schemars::schema_for, std::{
        env,
        fs::{self, OpenOptions},
        io::{self, Write},
        path::PathBuf,
    }, winresource::WindowsResource
};

#[path = "src/common.rs"]
mod common;

#[cfg(target_os = "windows")]
fn main() -> io::Result<()> {
    create_schema()?;
    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        WindowsResource::new()
            .set_icon("assets/dotfiles_icon.ico")
            .compile()?;
    }
    Ok(())
}

fn create_schema() -> io::Result<()> {
    let configuration_schema_path: PathBuf = PathBuf::from("schema/configuration_schema.json");
    if let Some(parent) = configuration_schema_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&configuration_schema_path)?;

    let schema = schema_for!(Configuration);
    file.write_all(serde_json::to_string_pretty(&schema)?.as_bytes())?;
    println!("Schema created successfully.");
    Ok(())
}
