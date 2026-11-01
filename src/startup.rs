//! "Abrir junto com o Windows": uma tarefa agendada que roda o Seek como administrador ao entrar no Windows,
//! sem a janela de confirmação a cada vez (a confirmação aparece uma vez só, ao ligar a opção).
//! Como administrador a leitura é a turbo e o diário do NTFS fica acessível. O Seek sobe na bandeja (`--tray`).
//! Versões antigas usavam uma entrada em HKCU\...\Run; ela é tirada ao ligar ou desligar a opção.
use std::os::windows::process::CommandExt;
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_SUCCESS},
    System::{
        Registry::{HKEY, HKEY_CURRENT_USER, RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW},
        Threading::{GetExitCodeProcess, WaitForSingleObject},
    },
    UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const TASK: &str = "Seek";
const NO_WINDOW: u32 = 0x0800_0000;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn run_key_set() -> bool {
    let mut key: HKEY = std::ptr::null_mut();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, 0x1, &mut key) } != ERROR_SUCCESS {
        return false;
    }
    let found = unsafe { RegQueryValueExW(key, wide(TASK).as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } == ERROR_SUCCESS;
    unsafe { RegCloseKey(key) };
    found
}
fn remove_run_key() {
    let mut key: HKEY = std::ptr::null_mut();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, 0x2, &mut key) } == ERROR_SUCCESS {
        unsafe { RegDeleteValueW(key, wide(TASK).as_ptr()) };
        unsafe { RegCloseKey(key) };
    }
}
fn task_exists() -> bool {
    std::process::Command::new("schtasks.exe")
        .args(["/Query", "/TN", TASK])
        .creation_flags(NO_WINDOW)
        .output()
        .is_ok_and(|o| o.status.success())
}
/// Roda como administrador (uma confirmação do Windows) e espera terminar. `true` se deu certo.
fn run_elevated(file: &str, params: &str) -> bool {
    let (verb, file, params) = (wide("runas"), wide(file), wide(params));
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = 0;
    if unsafe { ShellExecuteExW(&mut info) } == 0 || info.hProcess.is_null() {
        return false; // recusou a confirmação
    }
    let mut code = 1u32;
    unsafe {
        WaitForSingleObject(info.hProcess, 60_000);
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
    }
    code == 0
}
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
fn task_xml(exe: &str, user: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Abre o Seek na bandeja ao entrar no Windows.</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <StartWhenAvailable>true</StartWhenAvailable>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>"{exe}"</Command><Arguments>--tray</Arguments></Exec></Actions>
</Task>"#,
        exe = xml_escape(exe),
        user = xml_escape(user)
    )
}

pub fn is_enabled() -> bool {
    task_exists() || run_key_set()
}
pub fn set_enabled(on: bool) -> Result<(), String> {
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
        // O schtasks só aceita o XML em UTF-16 com BOM.
        let file = std::env::temp_dir().join("seek-tarefa.xml");
        let mut data = vec![0xFF, 0xFE];
        data.extend(task_xml(&exe.display().to_string(), &user).encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&file, data).map_err(|e| e.to_string())?;
        let ok = run_elevated("schtasks.exe", &format!("/Create /TN {TASK} /XML \"{}\" /F", file.display()));
        let _ = std::fs::remove_file(&file);
        if !ok {
            return Err("O Windows não criou a tarefa (a confirmação foi recusada?).".into());
        }
    } else if task_exists() && !run_elevated("schtasks.exe", &format!("/Delete /TN {TASK} /F")) {
        return Err("O Windows não apagou a tarefa (a confirmação foi recusada?).".into());
    }
    remove_run_key();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_xml_escapes_paths() {
        let x = task_xml("C:\\A&B\\Seek.exe", "PC\\Edu");
        assert!(x.contains("<Command>\"C:\\A&amp;B\\Seek.exe\"</Command>"));
        assert!(x.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(x.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
    }
}
