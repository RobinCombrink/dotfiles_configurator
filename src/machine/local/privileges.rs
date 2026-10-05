use {
    super::environment::VALUE_NOT_FOUND,
    anyhow::{Context, Result},
    windows_registry::LOCAL_MACHINE,
    windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

const APP_MODEL_UNLOCK: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock";

const DEVELOPMENT_WITHOUT_A_LICENSE: &str = "AllowDevelopmentWithoutDevLicense";

pub fn this_process_is_elevated() -> Result<bool> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo-handle GetCurrentProcess returns needs no closing, and token is a valid
    // place for the handle the call opens.
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) };
    if opened == 0 {
        return Err(std::io::Error::last_os_error())
            .context("Could not open this process's token to read whether it is elevated");
    }

    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut written = 0u32;
    // SAFETY: token was opened above for querying, and elevation is a TOKEN_ELEVATION of exactly
    // the length passed.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &raw mut written,
        )
    };
    let unread = (read == 0).then(std::io::Error::last_os_error);
    // SAFETY: token is the handle opened above, closed exactly once.
    unsafe { CloseHandle(token) };

    match unread {
        Some(failure) => {
            Err(failure).context("Could not read whether this process's token is elevated")
        }
        None => Ok(elevation.TokenIsElevated != 0),
    }
}

pub fn developer_mode_is_on() -> Result<bool> {
    let key = match LOCAL_MACHINE.open(APP_MODEL_UNLOCK) {
        Ok(key) => key,
        Err(error) if error.code().0 == VALUE_NOT_FOUND => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| format!("Could not open {APP_MODEL_UNLOCK}"));
        }
    };

    match key.get_u32(DEVELOPMENT_WITHOUT_A_LICENSE) {
        Ok(allowed) => Ok(allowed != 0),
        Err(error) if error.code().0 == VALUE_NOT_FOUND => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("Could not read {DEVELOPMENT_WITHOUT_A_LICENSE}"))
        }
    }
}

pub fn opening_lines(elevated: bool, developer_mode: &Result<bool>) -> [String; 2] {
    let elevation = match elevated {
        true => "this apply is elevated".to_owned(),
        false => "this apply is not elevated".to_owned(),
    };
    let developer_mode = match developer_mode {
        Ok(true) => "Developer Mode is on".to_owned(),
        Ok(false) => "Developer Mode is off".to_owned(),
        Err(error) => format!("whether Developer Mode is on could not be read: {error:#}"),
    };

    [elevation, developer_mode]
}

#[cfg(test)]
mod tests {
    use {super::*, anyhow::anyhow};

    #[test]
    fn an_apply_that_is_not_elevated_says_so() {
        let [elevation, _] = opening_lines(false, &Ok(true));

        assert_eq!(elevation, "this apply is not elevated");
    }

    #[test]
    fn an_elevated_apply_says_so() {
        let [elevation, _] = opening_lines(true, &Ok(true));

        assert_eq!(elevation, "this apply is elevated");
    }

    #[test]
    fn developer_mode_that_is_off_is_recorded_as_off() {
        let [_, developer_mode] = opening_lines(false, &Ok(false));

        assert_eq!(developer_mode, "Developer Mode is off");
    }

    #[test]
    fn developer_mode_that_is_on_is_recorded_as_on() {
        let [_, developer_mode] = opening_lines(false, &Ok(true));

        assert_eq!(developer_mode, "Developer Mode is on");
    }

    #[test]
    fn developer_mode_that_could_not_be_read_is_recorded_with_why_rather_than_as_off() {
        let [_, developer_mode] = opening_lines(false, &Err(anyhow!("Access is denied.")));

        assert!(
            developer_mode.contains("Access is denied."),
            "{developer_mode}"
        );
    }
}
