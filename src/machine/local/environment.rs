#[cfg(target_family = "windows")]
use {
    crate::machine::environment_reading::{carrying, entries_of},
    anyhow::Context,
    std::{iter::once, path::PathBuf, ptr},
    windows_registry::{CURRENT_USER, Key, LOCAL_MACHINE, Type, Value},
    windows_sys::Win32::{
        System::Environment::ExpandEnvironmentStringsW,
        UI::WindowsAndMessaging::{
            HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
        },
    },
};
use {
    crate::{
        configuration::{VariableName, VariableValue},
        machine::environment_reading::SearchPathReading,
    },
    anyhow::Result,
    std::path::Path,
};

#[cfg(target_family = "windows")]
const USER_ENVIRONMENT: &str = "Environment";

#[cfg(target_family = "windows")]
const MACHINE_ENVIRONMENT: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

#[cfg(target_family = "windows")]
const SEARCH_PATH: &str = "Path";

#[cfg(target_family = "windows")]
const VALUE_NOT_FOUND: i32 = 0x8007_0002u32 as i32; // HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)

#[cfg(target_family = "windows")]
const BROADCAST_TIMEOUT: u32 = 5_000; // 5 seconds

#[cfg(target_family = "windows")]
pub fn read_search_path() -> Result<SearchPathReading> {
    let mut entries: Vec<PathBuf> = Vec::new();
    entries.extend(hive_search_path(CURRENT_USER, USER_ENVIRONMENT)?);
    entries.extend(hive_search_path(LOCAL_MACHINE, MACHINE_ENVIRONMENT)?);

    Ok(SearchPathReading::of(entries))
}

#[cfg(target_family = "windows")]
pub fn read_variable(name: &VariableName) -> Result<Option<VariableValue>> {
    let key = CURRENT_USER
        .open(USER_ENVIRONMENT)
        .context("Could not read the user's environment key")?;

    match stored(&key, name.as_ref())? {
        None => Ok(None),
        Some(value) => String::try_from(value)
            .map(|text| Some(VariableValue::from(text)))
            .with_context(|| format!("{name} is stored as something other than text")),
    }
}

#[cfg(target_family = "windows")]
pub fn put_on_search_path(directory: &Path) -> Result<()> {
    let key = CURRENT_USER
        .create(USER_ENVIRONMENT)
        .context("Could not open the user's environment key to write")?;

    let (kind, raw) = match stored(&key, SEARCH_PATH)? {
        Some(value) => (value.ty(), String::try_from(value)?),
        None => (Type::ExpandString, String::new()),
    };

    let mut written = Value::from(carrying(&raw, directory).as_str());
    written.set_ty(kind);
    key.set_value(SEARCH_PATH, &written)
        .with_context(|| format!("Could not put {} on the search path", directory.display()))?;

    broadcast_environment_change();
    Ok(())
}

#[cfg(target_family = "windows")]
pub fn set_variable(name: &VariableName, value: &VariableValue) -> Result<()> {
    let key = CURRENT_USER
        .create(USER_ENVIRONMENT)
        .context("Could not open the user's environment key to write")?;

    match value.carries_a_reference() {
        true => key.set_expand_string(name.as_ref(), value.as_ref()),
        false => key.set_string(name.as_ref(), value.as_ref()),
    }
    .with_context(|| format!("Could not set {name}"))?;

    broadcast_environment_change();
    Ok(())
}

#[cfg(target_family = "windows")]
fn hive_search_path(hive: &Key, path: &str) -> Result<Vec<PathBuf>> {
    let Ok(key) = hive.open(path) else {
        return Ok(Vec::new());
    };
    let Some(value) = stored(&key, SEARCH_PATH)? else {
        return Ok(Vec::new());
    };
    let raw = String::try_from(value)
        .with_context(|| format!("{path} stores a search path that is not text"))?;

    Ok(entries_of(&raw)
        .into_iter()
        .flat_map(|entry| [PathBuf::from(entry), PathBuf::from(expanded(entry))])
        .collect())
}

#[cfg(target_family = "windows")]
fn stored(key: &Key, name: &str) -> Result<Option<Value>> {
    match key.get_value(name) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.code().0 == VALUE_NOT_FOUND => Ok(None),
        Err(error) => Err(error.into()),
    }
}

// ADR 0029
#[cfg(target_family = "windows")]
fn expanded(entry: &str) -> String {
    let source: Vec<u16> = entry.encode_utf16().chain(once(0)).collect();

    // SAFETY: source is NUL-terminated and outlives the call. A null destination with a zero
    // size is the documented form that asks only for the length it would need.
    let needed = unsafe { ExpandEnvironmentStringsW(source.as_ptr(), ptr::null_mut(), 0) };
    if needed == 0 {
        return entry.to_owned();
    }

    let mut destination = vec![0u16; needed as usize];
    // SAFETY: destination holds exactly the number of characters the call above asked for.
    let written =
        unsafe { ExpandEnvironmentStringsW(source.as_ptr(), destination.as_mut_ptr(), needed) };
    if written == 0 || written > needed {
        return entry.to_owned();
    }

    String::from_utf16_lossy(&destination[..written as usize - 1])
}

#[cfg(target_family = "windows")]
fn broadcast_environment_change() {
    let key: Vec<u16> = USER_ENVIRONMENT.encode_utf16().chain(once(0)).collect();

    // SAFETY: key is NUL-terminated and outlives the call, which the timeout bounds. The
    // result is not read, so a null pointer is passed for it.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            key.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            BROADCAST_TIMEOUT,
            ptr::null_mut(),
        );
    }
}

#[cfg(target_family = "unix")]
const REFUSAL: &str =
    "An environment variable is held in the Windows registry, and this machine has none";

#[cfg(target_family = "unix")]
pub fn read_search_path() -> Result<SearchPathReading> {
    anyhow::bail!("{REFUSAL}")
}

#[cfg(target_family = "unix")]
pub fn read_variable(_name: &VariableName) -> Result<Option<VariableValue>> {
    anyhow::bail!("{REFUSAL}")
}

#[cfg(target_family = "unix")]
pub fn put_on_search_path(_directory: &Path) -> Result<()> {
    anyhow::bail!("{REFUSAL}")
}

#[cfg(target_family = "unix")]
pub fn set_variable(_name: &VariableName, _value: &VariableValue) -> Result<()> {
    anyhow::bail!("{REFUSAL}")
}
