//! Mudanças ao vivo: o Windows avisa quando algo é criado, apagado, renomeado ou alterado dentro da
//! pasta indexada (`ReadDirectoryChangesW`). Não precisa de administrador e funciona em qualquer disco.
#![cfg(windows)]
use crate::{Change, Pending, resolve};
use std::{
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{CreateFileW, ReadDirectoryChangesW},
    System::IO::CancelIoEx,
};

pub struct Watcher {
    handle: Arc<AtomicUsize>,
}
impl Drop for Watcher {
    fn drop(&mut self) {
        let h = self.handle.load(Ordering::Relaxed);
        if h != 0 {
            unsafe { CancelIoEx(h as HANDLE, std::ptr::null()) };
        }
    }
}

// Ruído que ninguém quer no índice: lixeira, dados do sistema e pastas temporárias.
fn noisy(relative: &std::ffi::OsStr) -> bool {
    // Só vale o trecho dentro da pasta vigiada: quem indexa uma pasta dentro de Temp continua recebendo tudo.
    let p = format!("\\{}", relative.to_string_lossy().to_lowercase());
    ["\\$recycle.bin", "\\system volume information", "\\appdata\\local\\temp\\", "\\windows\\temp\\"]
        .iter()
        .any(|n| p.contains(n))
}

/// Passa a vigiar `root`; tudo que muda é resolvido aqui e juntado em `pending`.
pub fn start(root: &Path, pending: Arc<Mutex<Pending>>) -> Option<Watcher> {
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    // FILE_LIST_DIRECTORY, compartilhamento total, FILE_FLAG_BACKUP_SEMANTICS (necessário para abrir pastas).
    let h = unsafe { CreateFileW(wide.as_ptr(), 1, 7, std::ptr::null(), 3, 0x0200_0000, std::ptr::null_mut()) };
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    let handle = Arc::new(AtomicUsize::new(h as usize));
    let shared = handle.clone();
    let root = root.to_path_buf();
    let raw = h as usize;
    thread::spawn(move || {
        let h = raw as HANDLE;
        let mut buf = vec![0u32; 64 * 1024]; // 256 KB, alinhado em 4 bytes como o Windows exige
        loop {
            let mut got = 0u32;
            // 0x5B = nome de arquivo, nome de pasta, tamanho, última gravação, criação.
            let ok = unsafe {
                ReadDirectoryChangesW(h, buf.as_mut_ptr().cast(), (buf.len() * 4) as u32, 1, 0x5B, &mut got, std::ptr::null_mut(), None)
            };
            if ok == 0 {
                break;
            }
            if got == 0 {
                // Muitas mudanças de uma vez: o Windows descartou eventos.
                pending.lock().unwrap().merge(crate::Resolved::Overflow);
                continue;
            }
            let bytes: &[u8] = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast(), got as usize) };
            let mut batch: Vec<Change> = Vec::new();
            let mut off = 0usize;
            loop {
                if off + 12 > bytes.len() {
                    break;
                }
                let next = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
                let action = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap());
                let len = u32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap()) as usize;
                if off + 12 + len <= bytes.len() {
                    let name: Vec<u16> = bytes[off + 12..off + 12 + len]
                        .as_chunks::<2>().0.iter()
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    let relative = std::ffi::OsString::from_wide(&name);
                    let path: PathBuf = root.join(&relative);
                    if !noisy(&relative) {
                        let change = match action {
                            2 | 4 => Change::Remove(path),
                            3 => Change::Modified(path),
                            _ => Change::Upsert(path),
                        };
                        batch.push(change);
                    }
                }
                if next == 0 {
                    break;
                }
                off += next;
            }
            // ler o disco fora da trava (uma pasta nova pode ser grande), depois juntar de uma vez
            let resolved: Vec<_> = batch.into_iter().map(resolve).collect();
            let mut guard = pending.lock().unwrap();
            for r in resolved {
                guard.merge(r);
            }
        }
        shared.store(0, Ordering::Relaxed);
        unsafe { CloseHandle(h) };
    });
    Some(Watcher { handle })
}
