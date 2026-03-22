//! Raw registry values are retained byte-for-byte, including REG_EXPAND_SZ.
use anyhow::{Result, bail, ensure};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
    System::Registry::*,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
pub fn decode(bytes: &[u8]) -> String {
    let value: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|c| *c != 0)
        .collect();
    String::from_utf16_lossy(&value)
}
pub fn encode(text: &str) -> Vec<u8> {
    wide(text).into_iter().flat_map(u16::to_le_bytes).collect()
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn open(machine: bool, path: &str, view: u32, write: bool) -> Result<Option<Key>> {
    ensure!(!path.contains('\0'), "Invalid registry path");
    let hive = if machine {
        HKEY_LOCAL_MACHINE
    } else {
        HKEY_CURRENT_USER
    };
    let flags = if view == 32 {
        KEY_WOW64_32KEY
    } else {
        KEY_WOW64_64KEY
    } | if write {
        KEY_READ | KEY_SET_VALUE
    } else {
        KEY_READ
    };
    let mut handle = std::ptr::null_mut();
    let code = unsafe { RegOpenKeyExW(hive, wide(path).as_ptr(), 0, flags, &mut handle) };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if code != ERROR_SUCCESS {
        bail!(
            "Cannot open registry key ({code}). All-user entries may require administrator rights."
        );
    }
    Ok(Some(Key(handle)))
}
fn query(key: &Key, name: &str) -> Result<Option<(u32, Vec<u8>)>> {
    ensure!(!name.contains('\0'), "Invalid registry value name");
    let name = wide(name);
    let mut kind = 0;
    let mut len = 0;
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut len,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    ensure!(code == ERROR_SUCCESS, "Cannot read registry value ({code})");
    ensure!(
        len <= 1024 * 1024,
        "Registry value exceeds the safety limit"
    );
    let mut data = vec![0; len as usize];
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            data.as_mut_ptr(),
            &mut len,
        )
    };
    ensure!(
        code == ERROR_SUCCESS,
        "Registry value changed during the read ({code}); refresh and retry"
    );
    data.truncate(len as usize);
    Ok(Some((kind, data)))
}
pub fn read(machine: bool, path: &str, view: u32, name: &str) -> Result<Option<(u32, Vec<u8>)>> {
    open(machine, path, view, false)?.map_or(Ok(None), |key| query(&key, name))
}
pub fn values(machine: bool, path: &str, view: u32) -> Result<Vec<(String, u32, Vec<u8>)>> {
    let Some(key) = open(machine, path, view, false)? else {
        return Ok(vec![]);
    };
    let mut result = vec![];
    for i in 0..10000 {
        let mut name = vec![0u16; 16384];
        let mut len = name.len() as u32;
        let code = unsafe {
            RegEnumValueW(
                key.0,
                i,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if code == ERROR_NO_MORE_ITEMS {
            break;
        }
        ensure!(
            code == ERROR_SUCCESS,
            "Cannot enumerate registry values ({code})"
        );
        let name = String::from_utf16_lossy(&name[..len as usize]);
        if let Some((kind, data)) = query(&key, &name)? {
            result.push((name, kind, data));
        }
    }
    Ok(result)
}
pub fn write(
    machine: bool,
    path: &str,
    view: u32,
    name: &str,
    kind: u32,
    data: &[u8],
) -> Result<()> {
    ensure!(!name.contains('\0'), "Invalid registry name");
    let key = open(machine, path, view, true)?.ok_or_else(|| {
        anyhow::anyhow!(
            "The original registry key no longer exists; it will not be recreated automatically"
        )
    })?;
    let code = unsafe {
        RegSetValueExW(
            key.0,
            wide(name).as_ptr(),
            0,
            kind,
            data.as_ptr(),
            data.len() as u32,
        )
    };
    ensure!(
        code == ERROR_SUCCESS,
        "Cannot write registry value ({code}). Administrator rights may be required."
    );
    Ok(())
}
pub fn remove(machine: bool, path: &str, view: u32, name: &str) -> Result<()> {
    let key = open(machine, path, view, true)?
        .ok_or_else(|| anyhow::anyhow!("Registry key no longer exists"))?;
    let code = unsafe { RegDeleteValueW(key.0, wide(name).as_ptr()) };
    ensure!(
        code == ERROR_SUCCESS,
        "Cannot remove registry value ({code}). Administrator rights may be required."
    );
    Ok(())
}

#[cfg(test)]
pub struct TestKey(pub String);
#[cfg(test)]
impl TestKey {
    pub fn create() -> Self {
        let path = format!("Software\\StartupManagerTests\\{}", uuid::Uuid::new_v4());
        let mut key = std::ptr::null_mut();
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(&path).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS | KEY_WOW64_64KEY,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(
            result, ERROR_SUCCESS,
            "Cannot create the isolated HKCU test key"
        );
        drop(Key(key));
        Self(path)
    }
}
#[cfg(test)]
impl Drop for TestKey {
    fn drop(&mut self) {
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.0).as_ptr());
        }
    }
}
