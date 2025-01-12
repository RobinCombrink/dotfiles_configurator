use {
    std::{
        env,
        io::{self},
    },
    winresource::WindowsResource,
};

#[cfg(target_os = "windows")]
fn main() -> io::Result<()> {
    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        WindowsResource::new()
            .set_icon("assets/dotfiles_icon.ico")
            .compile()?;
    }
    Ok(())
}
