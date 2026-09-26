use std::io;

fn main() -> io::Result<()> {
    set_windows_icon()
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
