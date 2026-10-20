//! Leitura rápida de um disco NTFS inteiro: em vez de abrir pasta por pasta, lê a tabela interna do
//! volume (a MFT) em sequência, como o Everything. Precisa de administrador; sem isso devolve `Err`
//! e o chamador usa a leitura comum. Só lê nomes, tamanhos e a estrutura de pastas, nunca o conteúdo.
#![cfg(windows)]
use crate::Entry;
use rayon::prelude::*;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{CreateFileW, ReadFile, SetFilePointerEx},
    System::IO::DeviceIoControl,
};

const FSCTL_GET_NTFS_VOLUME_DATA: u32 = 0x0009_0064;
const ROOT_RECORD: u32 = 5;
const FIRST_USER_RECORD: u32 = 24; // 0..23 são arquivos internos do NTFS ($MFT, $Bitmap...)
const CHUNK: usize = 8 << 20;

struct Volume(HANDLE);
impl Drop for Volume {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}
impl Volume {
    fn open(letter: char) -> Result<Self, String> {
        let name: Vec<u16> = format!("\\\\.\\{letter}:").encode_utf16().chain(Some(0)).collect();
        let h = unsafe {
            CreateFileW(name.as_ptr(), 0x8000_0000, 1 | 2, std::ptr::null(), 3, 0, std::ptr::null_mut())
        };
        if h == INVALID_HANDLE_VALUE {
            Err("sem permissão de administrador".into())
        } else {
            Ok(Self(h))
        }
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let mut done = 0u32;
        let ok = unsafe {
            SetFilePointerEx(self.0, offset as i64, std::ptr::null_mut(), 0) != 0
                && ReadFile(self.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut done, std::ptr::null_mut()) != 0
        };
        if ok && done as usize == buf.len() {
            Ok(())
        } else {
            Err("falha ao ler o volume".into())
        }
    }
}

/// `true` se o programa pode abrir volumes (está como administrador).
pub fn can_read_volumes() -> bool {
    Volume::open('C').is_ok()
}
/// Letra do disco se `root` for a raiz de uma unidade (`D:\`).
pub fn drive_letter(root: &Path) -> Option<char> {
    let s = root.to_str()?;
    let b = s.as_bytes();
    (b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() && s[2..].chars().all(|c| c == '\\' || c == '/'))
        .then(|| (b[0] as char).to_ascii_uppercase())
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Desfaz a proteção de setores de um registro da MFT; `false` se o registro estiver corrompido.
fn fixup(rec: &mut [u8]) -> bool {
    if rec.len() < 48 || &rec[0..4] != b"FILE" {
        return false;
    }
    let (off, count) = (u16_at(rec, 4) as usize, u16_at(rec, 6) as usize);
    if count < 2 || off + count * 2 > rec.len() {
        return false;
    }
    let usn = [rec[off], rec[off + 1]];
    for i in 1..count {
        let end = i * 512;
        if end > rec.len() || rec[end - 2..end] != usn {
            return false;
        }
        rec[end - 2] = rec[off + i * 2];
        rec[end - 1] = rec[off + i * 2 + 1];
    }
    true
}

struct Rec {
    parent: u32,
    dir: bool,
    size: u64,
    modified: u64,
    created: u64,
    name: Box<str>,
}
fn parse_record(rec: &[u8]) -> Option<Rec> {
    let flags = u16_at(rec, 0x16);
    if flags & 1 == 0 || u64_at(rec, 0x20) != 0 {
        return None; // não usado, ou registro de extensão
    }
    let mut pos = u16_at(rec, 0x14) as usize;
    let (mut best, mut rank, mut size, mut modified, mut created) = (None, 0, 0u64, 0u64, 0u64);
    while pos + 16 <= rec.len() {
        let (kind, len) = (u32_at(rec, pos), u32_at(rec, pos + 4) as usize);
        if kind == 0xFFFF_FFFF || len < 16 || pos + len > rec.len() {
            break;
        }
        let non_resident = rec[pos + 8] != 0;
        if kind == 0x10 && !non_resident {
            // $STANDARD_INFORMATION: criação, modificação (+8), ... em unidades de 100 ns desde 1601.
            let c = pos + u16_at(rec, pos + 0x14) as usize;
            if c + 16 <= rec.len() {
                created = (u64_at(rec, c) / 10_000_000).saturating_sub(11_644_473_600);
                modified = (u64_at(rec, c + 8) / 10_000_000).saturating_sub(11_644_473_600);
            }
        } else if kind == 0x30 && !non_resident {
            let c = pos + u16_at(rec, pos + 0x14) as usize;
            if c + 0x42 <= rec.len() {
                let (n, space) = (rec[c + 0x40] as usize, rec[c + 0x41]);
                let r = match space {
                    1 | 3 => 3,
                    0 => 2,
                    _ => 1,
                };
                if r > rank && c + 0x42 + n * 2 <= rec.len() {
                    let units: Vec<u16> = (0..n).map(|k| u16_at(rec, c + 0x42 + k * 2)).collect();
                    best = Some(((u64_at(rec, c) & 0xFFFF_FFFF_FFFF) as u32, String::from_utf16_lossy(&units)));
                    rank = r;
                }
            }
        } else if kind == 0x80 && rec[pos + 9] == 0 {
            size = if non_resident {
                if u64_at(rec, pos + 0x10) == 0 && pos + 0x38 <= rec.len() { u64_at(rec, pos + 0x30) } else { size }
            } else {
                u32_at(rec, pos + 0x10) as u64
            };
        }
        pos += len;
    }
    let (parent, name) = best?;
    Some(Rec { parent, dir: flags & 2 != 0, size, modified, created, name: name.into() })
}

/// Trechos (início em bytes, tamanho em bytes) que formam a MFT, lidos da lista de extensões do registro 0.
fn mft_extents(rec0: &[u8], cluster: u64) -> Option<Vec<(u64, u64)>> {
    let mut pos = u16_at(rec0, 0x14) as usize;
    while pos + 16 <= rec0.len() {
        let (kind, len) = (u32_at(rec0, pos), u32_at(rec0, pos + 4) as usize);
        if kind == 0xFFFF_FFFF || len < 16 || pos + len > rec0.len() {
            break;
        }
        if kind == 0x80 && rec0[pos + 8] != 0 {
            let mut run = pos + u16_at(rec0, pos + 0x20) as usize;
            let (mut lcn, mut out) = (0i64, Vec::new());
            while run < pos + len && rec0[run] != 0 {
                let (ls, os) = ((rec0[run] & 0xF) as usize, (rec0[run] >> 4) as usize);
                run += 1;
                if ls == 0 || run + ls + os > rec0.len() {
                    return None;
                }
                let length = rec0[run..run + ls].iter().rev().fold(0u64, |a, b| a << 8 | *b as u64);
                run += ls;
                if os > 0 {
                    let mut delta = rec0[run..run + os].iter().rev().fold(0i64, |a, b| a << 8 | *b as i64);
                    if rec0[run + os - 1] & 0x80 != 0 {
                        delta -= 1i64 << (8 * os); // sinal
                    }
                    lcn += delta;
                    run += os;
                    out.push((lcn as u64 * cluster, length * cluster));
                } else {
                    return None; // trecho vazio na MFT: não esperado
                }
            }
            return Some(out);
        }
        pos += len;
    }
    None
}

fn dir_path(i: u32, recs: &[Option<Rec>], memo: &mut HashMap<u32, Option<String>>, drive: &str, depth: u32) -> Option<String> {
    if i == ROOT_RECORD {
        return Some(drive.to_owned());
    }
    if depth > 255 {
        return None;
    }
    if let Some(v) = memo.get(&i) {
        return v.clone();
    }
    let result = recs.get(i as usize).and_then(|r| r.as_ref()).filter(|r| r.dir).and_then(|r| {
        let p = dir_path(r.parent, recs, memo, drive, depth + 1)?;
        Some(format!("{p}\\{}", r.name))
    });
    memo.insert(i, result.clone());
    result
}

/// Lê todos os arquivos do disco `root` (precisa ser a raiz de uma unidade NTFS).
pub fn scan(root: &Path, cancel: &AtomicBool, progress: &AtomicUsize) -> Result<Vec<Entry>, String> {
    let letter = drive_letter(root).ok_or("não é a raiz de um disco")?;
    let vol = Volume::open(letter)?;
    let mut data = [0u8; 128];
    let mut got = 0u32;
    let ok = unsafe {
        DeviceIoControl(vol.0, FSCTL_GET_NTFS_VOLUME_DATA, std::ptr::null(), 0, data.as_mut_ptr().cast(), 128, &mut got, std::ptr::null_mut())
    };
    if ok == 0 || got < 72 {
        return Err("o disco não é NTFS".into());
    }
    let (cluster, rec_size) = (u32_at(&data, 44) as u64, u32_at(&data, 48) as usize);
    let (valid, start_lcn) = (u64_at(&data, 56), u64_at(&data, 64));
    if !(512..=4096).contains(&rec_size) || cluster == 0 || !CHUNK.is_multiple_of(rec_size) {
        return Err("formato de NTFS inesperado".into());
    }
    let mut first = vec![0u8; 4096.max(rec_size)];
    vol.read_at(start_lcn * cluster, &mut first)?;
    let mut rec0 = first[..rec_size].to_vec();
    if !fixup(&mut rec0) {
        return Err("registro da MFT inválido".into());
    }
    let extents = mft_extents(&rec0, cluster).ok_or("MFT fragmentada de um jeito não suportado")?;
    if extents.iter().map(|e| e.1).sum::<u64>() < valid {
        return Err("MFT incompleta".into());
    }
    let total = (valid as usize) / rec_size;
    let mut recs: Vec<Option<Rec>> = Vec::new();
    recs.resize_with(total, || None);
    let (mut index, mut remaining, mut seen) = (0usize, valid, 0usize);
    let mut buf = vec![0u8; CHUNK];
    'outer: for (offset, length) in extents {
        let mut done = 0u64;
        while done < length && remaining > 0 {
            if cancel.load(Ordering::Relaxed) {
                break 'outer;
            }
            let want = (length - done).min(CHUNK as u64).min(remaining.div_ceil(cluster) * cluster) as usize;
            vol.read_at(offset + done, &mut buf[..want])?;
            for r in buf[..want].chunks_exact_mut(rec_size) {
                if index >= total || remaining == 0 {
                    break;
                }
                if fixup(r) && (index as u32 == ROOT_RECORD || index as u32 >= FIRST_USER_RECORD)
                    && let Some(rec) = parse_record(r) {
                        recs[index] = Some(rec);
                        seen += 1;
                    }
                index += 1;
                remaining = remaining.saturating_sub(rec_size as u64);
            }
            progress.store(seen, Ordering::Relaxed);
            done += want as u64;
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(Vec::new());
    }
    let drive = format!("{letter}:");
    let mut memo = HashMap::new();
    let mut files: Vec<(String, u64, u64, u64)> = Vec::new();
    let skip = [format!("{drive}\\$RECYCLE.BIN"), format!("{drive}\\System Volume Information")];
    for rec in recs.iter().flatten().filter(|r| !r.dir) {
        if let Some(parent) = dir_path(rec.parent, &recs, &mut memo, &drive, 0) {
            // Lixeira e dados internos do Windows não são arquivos "perdidos" de ninguém.
            if skip.iter().any(|k| parent.as_bytes().get(..k.len()).is_some_and(|head| head.eq_ignore_ascii_case(k.as_bytes()))) {
                continue;
            }
            files.push((format!("{parent}\\{}", rec.name), rec.size, rec.modified, rec.created));
        }
    }
    Ok(files.into_par_iter().map(|(p, size, modified, created)| Entry::new(PathBuf::from(p), size).with_modified(modified).with_created(created)).collect())
}
