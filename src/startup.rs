//! "Abrir junto com o Windows": uma entrada em HKCU\...\Run (não precisa de administrador).
//! O Seek sobe direto na bandeja (`--tray`), sem abrir a janela.
use windows_sys::Win32::{
    Foundation::ERROR_SUCCESS,
    System::Registry::{HKEY, HKEY_CURRENT_USER, RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW},
};

const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "Seek";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn open(access: u32) -> Option<HKEY> {
    let mut key: HKEY = std::ptr::null_mut();
    (unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(KEY).as_ptr(), 0, access, &mut key) } == ERROR_SUCCESS).then_some(key)
}
fn is_set(name: &str) -> bool {
    let Some(key) = open(0x1) else { return false }; // KEY_QUERY_VALUE
    let found = unsafe {
        RegQueryValueExW(key, wide(name).as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
    } == ERROR_SUCCESS;
    unsafe { RegCloseKey(key) };
    found
}
fn set(name: &str, command: Option<&str>) -> Result<(), String> {
    let key = open(0x2).ok_or("Não foi possível abrir o registro do Windows.")?; // KEY_SET_VALUE
    let result = unsafe {
        match command {
            Some(c) => {
                let data = wide(c);
                RegSetValueExW(key, wide(name).as_ptr(), 0, 1, data.as_ptr().cast(), (data.len() * 2) as u32) // REG_SZ
            }
            None => RegDeleteValueW(key, wide(name).as_ptr()),
        }
    };
    unsafe { RegCloseKey(key) };
    // Apagar o que já não existe não é erro.
    if result == ERROR_SUCCESS || (command.is_none() && result == 2) {
        Ok(())
    } else {
        Err(format!("Não foi possível gravar a configuração (erro {result})."))
    }
}

pub fn is_enabled() -> bool {
    is_set(NAME)
}
pub fn set_enabled(on: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    set(NAME, on.then(|| format!("\"{}\" --tray", exe.display())).as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_entry_can_be_added_and_removed() {
        let name = "SeekTestEntry";
        set(name, Some("\"C:\\x\\Seek.exe\" --tray")).unwrap();
        assert!(is_set(name));
        set(name, None).unwrap();
        assert!(!is_set(name));
        set(name, None).unwrap(); // apagar de novo não dá erro
    }
}
