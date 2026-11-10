#[cfg(target_family = "windows")]
use {
    crate::machine::{
        ElevatedWork, OsError, Placement, PrivilegeRefusal,
        environment_reading::{RegistryKey, RegistryValueName, carrying, entries_of, fronting},
    },
    anyhow::Context,
    std::{iter::once, path::PathBuf, ptr, sync::Arc},
    windows_registry::{CURRENT_USER, Key, LOCAL_MACHINE, Type, Value},
    windows_sys::Win32::{
        Foundation::ERROR_ACCESS_DENIED,
        System::Environment::ExpandEnvironmentStringsW,
        UI::WindowsAndMessaging::{
            HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
        },
    },
};
use {
    crate::{
        configuration::{VariableName, VariableValue},
        machine::environment_reading::{MachineSearchPath, RegistryReadFailure, SearchPathReading},
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
pub(super) const VALUE_NOT_FOUND: i32 = 0x8007_0002u32 as i32; // HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)

#[cfg(target_family = "windows")]
const ACCESS_DENIED: i32 = 0x8007_0005u32 as i32; // HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED)

#[cfg(target_family = "windows")]
const BROADCAST_TIMEOUT: u32 = 5_000; // 5 seconds

#[cfg(target_family = "windows")]
pub fn read_machine_search_path() -> Result<MachineSearchPath, RegistryReadFailure> {
    let raw = stored_search_path(RegistryKey::MachineEnvironment)?;

    Ok(MachineSearchPath::of(
        entries_of(&raw)
            .into_iter()
            .map(|entry| PathBuf::from(expanded(entry))),
    ))
}

#[cfg(target_family = "windows")]
pub fn put_first_on_machine_search_path(directory: &Path) -> Result<Placement> {
    match fronted_machine_search_path(directory) {
        Ok(()) => Ok(Placement::Placed),
        Err(error) if error.code().0 == ACCESS_DENIED => Ok(Placement::Refused(PrivilegeRefusal {
            work: ElevatedWork::MachineSearchPathFront {
                directory: directory.to_path_buf(),
            },
            os_error: OsError(ERROR_ACCESS_DENIED.cast_signed()),
        })),
        Err(error) => Err(error).with_context(|| {
            format!(
                "Could not put {} first on the Machine PATH",
                directory.display()
            )
        }),
    }
}

#[cfg(target_family = "windows")]
fn fronted_machine_search_path(directory: &Path) -> windows_registry::Result<()> {
    let key = LOCAL_MACHINE.create(MACHINE_ENVIRONMENT)?;
    let (kind, raw) = match key.get_value(SEARCH_PATH) {
        Ok(value) => (value.ty(), String::try_from(value)?),
        Err(error) if error.code().0 == VALUE_NOT_FOUND => (Type::ExpandString, String::new()),
        Err(error) => return Err(error),
    };

    let fronted = fronting(&raw, directory, |entry| PathBuf::from(expanded(entry)));
    if fronted == raw {
        return Ok(());
    }
    let mut written = Value::from(fronted.as_str());
    written.set_ty(kind);
    key.set_value(SEARCH_PATH, &written)?;

    broadcast_environment_change();
    Ok(())
}

#[cfg(target_family = "windows")]
pub fn read_search_path() -> Result<SearchPathReading, RegistryReadFailure> {
    let mut entries: Vec<PathBuf> = Vec::new();
    entries.extend(hive_search_path(RegistryKey::MachineEnvironment)?);
    entries.extend(hive_search_path(RegistryKey::UserEnvironment)?);

    Ok(SearchPathReading::of(entries))
}

#[cfg(target_family = "windows")]
pub fn read_variable(name: &VariableName) -> Result<Option<VariableValue>, RegistryReadFailure> {
    let key = opened(RegistryKey::UserEnvironment)?;
    let value = RegistryValueName::Variable(name.clone());

    stored_text(&key, RegistryKey::UserEnvironment, value)
        .map(|stored| stored.map(VariableValue::from))
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
fn opened(key: RegistryKey) -> Result<Key, RegistryReadFailure> {
    let (hive, path) = match key {
        RegistryKey::UserEnvironment => (CURRENT_USER, USER_ENVIRONMENT),
        RegistryKey::MachineEnvironment => (LOCAL_MACHINE, MACHINE_ENVIRONMENT),
    };
    hive.open(path)
        .map_err(|error| RegistryReadFailure::KeyUnopened {
            key,
            error: Arc::new(error),
        })
}

#[cfg(target_family = "windows")]
fn stored_text(
    key: &Key,
    opened_at: RegistryKey,
    value: RegistryValueName,
) -> Result<Option<String>, RegistryReadFailure> {
    let stored = match stored(key, &value.to_string()) {
        Ok(stored) => stored,
        Err(error) => {
            return Err(RegistryReadFailure::ValueUnread {
                key: opened_at,
                value,
                error: Arc::new(error),
            });
        }
    };
    let Some(stored) = stored else {
        return Ok(None);
    };

    String::try_from(stored)
        .map(Some)
        .map_err(|error| RegistryReadFailure::ValueNotText {
            key: opened_at,
            value,
            error: Arc::new(error),
        })
}

#[cfg(target_family = "windows")]
fn stored_search_path(key: RegistryKey) -> Result<String, RegistryReadFailure> {
    let opened_key = opened(key)?;

    stored_text(&opened_key, key, RegistryValueName::SearchPath).map(Option::unwrap_or_default)
}

#[cfg(target_family = "windows")]
fn hive_search_path(key: RegistryKey) -> Result<Vec<PathBuf>, RegistryReadFailure> {
    let raw = stored_search_path(key)?;

    Ok(entries_of(&raw)
        .into_iter()
        .flat_map(|entry| [PathBuf::from(entry), PathBuf::from(expanded(entry))])
        .collect())
}

#[cfg(target_family = "windows")]
fn stored(key: &Key, name: &str) -> windows_registry::Result<Option<Value>> {
    match key.get_value(name) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.code().0 == VALUE_NOT_FOUND => Ok(None),
        Err(error) => Err(error),
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
pub fn read_search_path() -> Result<SearchPathReading, RegistryReadFailure> {
    Err(RegistryReadFailure::NoRegistry)
}

#[cfg(target_family = "unix")]
pub fn read_machine_search_path() -> Result<MachineSearchPath, RegistryReadFailure> {
    Err(RegistryReadFailure::NoRegistry)
}

#[cfg(target_family = "unix")]
pub fn read_variable(_name: &VariableName) -> Result<Option<VariableValue>, RegistryReadFailure> {
    Err(RegistryReadFailure::NoRegistry)
}

#[cfg(target_family = "unix")]
pub fn put_on_search_path(_directory: &Path) -> Result<()> {
    anyhow::bail!("{REFUSAL}")
}

#[cfg(target_family = "unix")]
pub fn set_variable(_name: &VariableName, _value: &VariableValue) -> Result<()> {
    anyhow::bail!("{REFUSAL}")
}
