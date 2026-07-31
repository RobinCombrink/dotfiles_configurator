use std::{
    fs,
    io::{self},
    path::PathBuf,
};

// The schema is derived from the same types the tool reads configurations with, so it cannot
// describe a shape the tool would refuse. Only `Configuration` is named here; everything the
// crate itself uses is unreachable from a build script and would otherwise be reported as dead.
#[allow(dead_code, unused_imports)]
#[path = "src/configuration.rs"]
mod configuration;

fn main() -> io::Result<()> {
    write_configuration_schema()?;
    set_windows_icon()
}

fn write_configuration_schema() -> io::Result<()> {
    let path = PathBuf::from("schema/configuration_schema.json");
    if let Some(parent_directory) = path.parent() {
        fs::create_dir_all(parent_directory)?;
    }

    let schema = schemars::schema_for!(configuration::Configuration);
    let mut rendered = serde_json::to_string_pretty(&schema)?;
    rendered.push('\n');

    // Only written when it would change, so a build does not dirty the working tree for nothing.
    if fs::read_to_string(&path).is_ok_and(|existing| existing == rendered) {
        return Ok(());
    }
    fs::write(&path, rendered)
}

#[cfg(target_family = "unix")]
fn set_windows_icon() -> io::Result<()> {
    Ok(())
}

#[cfg(target_family = "windows")]
fn set_windows_icon() -> io::Result<()> {
    use {std::env, winresource::WindowsResource};

    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        WindowsResource::new()
            .set_icon("assets/dotfiles_icon.ico")
            .compile()?;
    }
    Ok(())
}
