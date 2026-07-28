use std::io;

#[cfg(target_family = "unix")]
fn main() -> io::Result<()> {
    Ok(())
}

#[cfg(target_family = "windows")]
fn main() -> io::Result<()> {
    use std::env;
    use winresource::WindowsResource;

    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        WindowsResource::new()
            .set_icon("assets/dotfiles_icon.ico")
            .compile()?;
    }
    Ok(())
}
