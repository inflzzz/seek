//! Registro de erros e eventos importantes em `seek-log.txt`, ao lado do programa.
//! Serve para entender o que aconteceu quando alguém relata um problema; nada é enviado para a internet.
use std::{fs, io::Write, path::PathBuf};

const MAX_BYTES: u64 = 512 * 1024;

fn path() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("seek-log.txt"))
}
/// Acrescenta uma linha com data e hora. Falhas de gravação são ignoradas de propósito.
pub fn line(text: &str) {
    let Some(path) = path() else { return };
    // arquivo grande demais: recomeça (guarda só o que vem a seguir)
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = fs::remove_file(&path);
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {text}", crate::clock::format_local(crate::clock::now_secs()));
    }
}
