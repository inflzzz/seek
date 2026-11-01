//! Jogos instalados pela Steam e pela Epic: onde estão e quando foram jogados pela última vez.
//! Os formatos dos launchers não têm documentação oficial: tudo aqui só lê, e o que falhar é ignorado.
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub name: String,
    pub launcher: &'static str,
    pub dir: PathBuf,
    pub last_played: u64, // segundos desde 1970; 0 = não se sabe
    pub exe: String,      // executável principal (Epic), para achar no Prefetch
}

/// Valores `"chave" "valor"` de um arquivo VDF/ACF da Steam, na ordem em que aparecem.
pub fn vdf_pairs(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut quoted = Vec::new();
        let mut cur: Option<String> = None;
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match (c, cur.as_mut()) {
                ('"', None) => cur = Some(String::new()),
                ('"', Some(_)) => quoted.push(cur.take().unwrap()),
                ('\\', Some(s)) => {
                    if let Some(n) = chars.next() {
                        s.push(n);
                    }
                }
                (c, Some(s)) => s.push(c),
                _ => {}
            }
        }
        if let [k, v] = quoted.as_slice() {
            out.push((k.to_lowercase(), v.clone()));
        }
    }
    out
}
fn steam_root() -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, w("Software\\Valve\\Steam").as_ptr(), w("SteamPath").as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len)
    } == 0;
    let found = ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]).replace('/', "\\")));
    found.or_else(|| Some(PathBuf::from("C:\\Program Files (x86)\\Steam"))).filter(|p| p.is_dir())
}
/// Jogos da Steam em todas as bibliotecas.
pub fn steam() -> Vec<Game> {
    let Some(root) = steam_root() else { return Vec::new() };
    let mut libs = vec![root.clone()];
    if let Ok(text) = fs::read_to_string(root.join("steamapps\\libraryfolders.vdf")) {
        for (k, v) in vdf_pairs(&text) {
            let p = PathBuf::from(v);
            let same = |a: &PathBuf| a.to_string_lossy().eq_ignore_ascii_case(&p.to_string_lossy());
            if k == "path" && !libs.iter().any(same) {
                libs.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for lib in libs {
        let apps = lib.join("steamapps");
        let Ok(read) = fs::read_dir(&apps) else { continue };
        for f in read.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = fs::read_to_string(f.path()) else { continue };
            let pairs = vdf_pairs(&text);
            let get = |k: &str| pairs.iter().find(|p| p.0 == k).map(|p| p.1.clone());
            let (Some(title), Some(dir)) = (get("name"), get("installdir")) else { continue };
            if get("appid").as_deref() == Some("228980") {
                continue; // Steamworks Common Redistributables: peças da Steam, não um jogo
            }
            out.push(Game {
                name: title,
                launcher: "Steam",
                dir: apps.join("common").join(dir),
                last_played: get("lastplayed").and_then(|v| v.parse().ok()).unwrap_or(0),
                exe: String::new(),
            });
        }
    }
    out
}
/// Jogos da Epic Games.
pub fn epic() -> Vec<Game> {
    let dir = Path::new("C:\\ProgramData\\Epic\\EpicGamesLauncher\\Data\\Manifests");
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for f in read.flatten() {
        if f.path().extension().is_none_or(|e| e != "item") {
            continue;
        }
        let Ok(v) = fs::read_to_string(f.path()).map_err(|_| ()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|_| ())) else { continue };
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
        let (name, loc) = (s("DisplayName"), s("InstallLocation"));
        if name.is_empty() || loc.is_empty() {
            continue;
        }
        let exe = Path::new(&s("LaunchExecutable")).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push(Game { name, launcher: "Epic", dir: PathBuf::from(loc.replace('/', "\\")), last_played: 0, exe });
    }
    out
}
/// Última vez que um executável rodou, pelo Prefetch do Windows (precisa de administrador para ler).
fn prefetch_time(exe: &str) -> u64 {
    if exe.is_empty() {
        return 0;
    }
    let want = format!("{}-", exe.to_uppercase());
    let Ok(read) = fs::read_dir("C:\\Windows\\Prefetch") else { return 0 };
    read.flatten()
        .filter(|f| f.file_name().to_string_lossy().to_uppercase().starts_with(&want))
        .filter_map(|f| f.metadata().ok()?.modified().ok())
        .map(crate::unix_secs)
        .max()
        .unwrap_or(0)
}
/// Todos os jogos, com a última vez jogada preenchida pelo Prefetch quando o launcher não diz.
pub fn all() -> Vec<Game> {
    let mut list = steam();
    list.extend(epic());
    for g in &mut list {
        if g.last_played == 0 {
            if g.exe.is_empty() {
                // Steam sem data: o maior .exe da pasta costuma ser o jogo
                g.exe = fs::read_dir(&g.dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|f| f.path().extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
                    .max_by_key(|f| f.metadata().map_or(0, |m| m.len()))
                    .map(|f| f.file_name().to_string_lossy().into_owned())
                    .unwrap_or_default();
            }
            g.last_played = prefetch_time(&g.exe);
        }
    }
    list.retain(|g| g.dir.is_dir());
    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    list
}
/// Algum destes programas está aberto? (nomes de executável, sem diferenciar maiúsculas)
pub fn running(names: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("tasklist").args(["/fo", "csv", "/nh"]).creation_flags(0x0800_0000).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    names.iter().find(|n| !n.is_empty() && text.contains(&format!("\"{}\"", n.to_lowercase()))).map(|n| n.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn steam_files_are_read() {
        let lib = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t\t\"label\"\t\t\"\"\n\t}\n}";
        let p = vdf_pairs(lib);
        assert!(p.contains(&("path".into(), "D:\\SteamLibrary".into())));
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\t\"installdir\"\t\t\"Counter-Strike Global Offensive\"\n\t\"LastPlayed\"\t\t\"1761000000\"\n}";
        let p = vdf_pairs(acf);
        assert!(p.contains(&("lastplayed".into(), "1761000000".into())));
        assert!(p.contains(&("name".into(), "Counter-Strike 2".into())));
    }
}
