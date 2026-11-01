//! Diário de mudanças do NTFS (USN): o Windows anota cada arquivo criado, alterado ou apagado, mesmo com o
//! Seek fechado. Aqui só se usa o horário: a releitura ao abrir diz o que mudou, e o diário diz quando.
//! Precisa de administrador; sem isso (ou em disco que não é NTFS) devolve vazio.
#![cfg(windows)]
use crate::mft::Volume;
use std::{collections::HashMap, path::Path};
use windows_sys::Win32::System::IO::DeviceIoControl;

const FSCTL_QUERY_USN_JOURNAL: u32 = 0x0009_00F4;
const FSCTL_READ_USN_JOURNAL: u32 = 0x0009_00BB;

/// (id do diário, próximo número) de um disco.
pub fn position(letter: char) -> Option<(u64, i64)> {
    let vol = Volume::open(letter).ok()?;
    let mut out = [0u8; 64];
    let mut got = 0u32;
    let ok = unsafe {
        DeviceIoControl(vol.0, FSCTL_QUERY_USN_JOURNAL, std::ptr::null(), 0, out.as_mut_ptr().cast(), out.len() as u32, &mut got, std::ptr::null_mut())
    } != 0;
    (ok && got >= 24).then(|| (u64::from_le_bytes(out[0..8].try_into().unwrap()), i64::from_le_bytes(out[16..24].try_into().unwrap())))
}
fn filetime_secs(ft: i64) -> u64 {
    (ft / 10_000_000 - 11_644_473_600).max(0) as u64
}
/// Nome do arquivo (minúsculo) -> último horário em que ele mudou, desde `from` no diário `id`.
/// Vazio se o diário foi recriado ou já descartou esse trecho.
pub fn times_since(letter: char, id: u64, from: i64) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    let Ok(vol) = Volume::open(letter) else { return map };
    if position(letter).is_none_or(|(now_id, _)| now_id != id) {
        return map;
    }
    let mut start = from;
    let mut buf = vec![0u8; 1 << 20];
    let mut records = 0usize;
    loop {
        // READ_USN_JOURNAL_DATA_V0: StartUsn, ReasonMask, ReturnOnlyOnClose, Timeout, BytesToWaitFor, UsnJournalID
        let mut input = [0u8; 40];
        input[0..8].copy_from_slice(&start.to_le_bytes());
        input[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        input[32..40].copy_from_slice(&id.to_le_bytes());
        let mut got = 0u32;
        let ok = unsafe {
            DeviceIoControl(vol.0, FSCTL_READ_USN_JOURNAL, input.as_ptr().cast(), 40, buf.as_mut_ptr().cast(), buf.len() as u32, &mut got, std::ptr::null_mut())
        } != 0;
        if !ok || got <= 8 {
            break; // fim, ou trecho já descartado pelo Windows
        }
        let next = i64::from_le_bytes(buf[0..8].try_into().unwrap());
        parse(&buf[8..got as usize], &mut map);
        records += 1;
        if next <= start || records > 20_000 {
            break; // não andou, ou já leu ~20 GB de diário (bem mais do que o normal)
        }
        start = next;
    }
    map
}
/// Registros USN_RECORD_V2/V3 de um bloco.
fn parse(mut b: &[u8], map: &mut HashMap<String, u64>) {
    while b.len() >= 8 {
        let len = u32::from_le_bytes(b[0..4].try_into().unwrap()) as usize;
        if len < 60 || len > b.len() {
            break;
        }
        let r = &b[..len];
        let major = u16::from_le_bytes([r[4], r[5]]);
        let (ts, nlen, noff) = match major {
            2 => (32, 56, 58),
            3 => (48, 72, 74),
            _ => {
                b = &b[len..];
                continue;
            }
        };
        if r.len() >= noff + 2 {
            let when = filetime_secs(i64::from_le_bytes(r[ts..ts + 8].try_into().unwrap()));
            let (n, o) = (u16::from_le_bytes([r[nlen], r[nlen + 1]]) as usize, u16::from_le_bytes([r[noff], r[noff + 1]]) as usize);
            if let Some(name) = r.get(o..o + n) {
                let units: Vec<u16> = name.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                let key = crate::normalize(&String::from_utf16_lossy(&units));
                let e = map.entry(key).or_insert(0);
                *e = (*e).max(when);
            }
        }
        b = &b[len..];
    }
}

/// Posição do diário de cada disco, guardada entre uma execução e outra (`historico\usn.txt`).
pub fn load(file: &Path) -> HashMap<char, (u64, i64)> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.split(';').collect();
            let [d, id, usn] = p.as_slice() else { return None };
            Some((d.chars().next()?, (id.parse().ok()?, usn.parse().ok()?)))
        })
        .collect()
}
pub fn save(file: &Path, drives: &[char]) {
    let text: String = drives.iter().filter_map(|&d| position(d).map(|(id, usn)| format!("{d};{id};{usn}\n"))).collect();
    if !text.is_empty() {
        let _ = std::fs::write(file, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Precisa de administrador: `SEEK_TEST_USN=D cargo test -- --ignored usn_real --nocapture`
    #[test]
    #[ignore]
    fn usn_real_journal_gives_times() {
        let Some(letter) = std::env::var("SEEK_TEST_USN").ok().and_then(|s| s.chars().next()) else { return };
        let (id, next) = position(letter).expect("sem diário (precisa de administrador e NTFS)");
        let probe = std::env::current_dir().unwrap().join("seek-usn-teste.tmp");
        std::fs::write(&probe, b"x").unwrap();
        std::fs::remove_file(&probe).unwrap();
        let map = times_since(letter, id, next);
        println!("{} nomes desde a posição salva", map.len());
        let t = map.get("seek-usn-teste.tmp").copied().expect("o arquivo criado agora tem que aparecer");
        assert!(t.abs_diff(crate::clock::now_secs()) < 120);
    }
    #[test]
    fn v2_records_are_parsed() {
        let name: Vec<u8> = "Foto.JPG".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let mut r = vec![0u8; 60];
        r[4] = 2;
        let ft: i64 = (1_700_000_000 + 11_644_473_600) * 10_000_000;
        r[32..40].copy_from_slice(&ft.to_le_bytes());
        r[56..58].copy_from_slice(&(name.len() as u16).to_le_bytes());
        r[58..60].copy_from_slice(&60u16.to_le_bytes());
        r.extend(&name);
        while r.len() % 8 != 0 {
            r.push(0);
        }
        let len = r.len() as u32;
        r[0..4].copy_from_slice(&len.to_le_bytes());
        let mut two = r.clone();
        two.extend(&r);
        let mut map = HashMap::new();
        parse(&two, &mut map);
        assert_eq!(map.get("foto.jpg"), Some(&1_700_000_000));
    }
}
