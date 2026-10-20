//! Atualização automática pelo GitHub Releases: confere a versão mais nova, baixa o executável,
//! confere o SHA-256 publicado pelo GitHub e troca o arquivo. A versão nova vale na próxima abertura.
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub const REPO: &str = "inflzzz/seek";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const ASSET: &str = "Seek.exe";
const MAX_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.trim().trim_start_matches('v').split('.');
    let n = |p: Option<&str>| p?.split(['-', '+']).next()?.parse().ok();
    Some((n(parts.next())?, n(parts.next())?, n(parts.next())?))
}
pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse_version(candidate), parse_version(current)), (Some(a), Some(b)) if a > b)
}
/// Lê a resposta de `releases/latest`. Sem hash publicado pelo GitHub, a versão é ignorada:
/// nunca instala um arquivo que não pôde ser conferido.
pub fn parse_release(json: &serde_json::Value) -> Option<Release> {
    let version = json.get("tag_name")?.as_str()?.trim_start_matches('v').to_owned();
    let asset = json
        .get("assets")?
        .as_array()?
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(ASSET))?;
    let url = asset.get("browser_download_url")?.as_str()?.to_owned();
    let sha256 = asset.get("digest")?.as_str()?.strip_prefix("sha256:")?.to_lowercase();
    // Só baixa do próprio repositório, por HTTPS.
    url.starts_with(&format!("https://github.com/{REPO}/releases/download/"))
        .then_some(Release { version, url, sha256 })
}

fn sibling(exe: &Path, ext: &str) -> PathBuf {
    exe.with_extension(ext)
}
/// Apaga restos de uma atualização anterior (o .exe antigo só pode sair depois que o programa fecha).
pub fn cleanup(exe: &Path) {
    let _ = fs::remove_file(sibling(exe, "old"));
    let _ = fs::remove_file(sibling(exe, "update"));
}
/// Troca o executável em uso: o Windows permite renomear um .exe aberto, mas não sobrescrevê-lo.
pub fn swap(exe: &Path, new: &Path) -> std::io::Result<()> {
    let old = sibling(exe, "old");
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old)?;
    if let Err(e) = fs::rename(new, exe) {
        let _ = fs::rename(&old, exe);
        return Err(e);
    }
    Ok(())
}

fn sha256_hex(data: &[u8; 32]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}
fn download(url: &str, to: &Path, expected: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let response = agent().get(url).call().map_err(|e| e.to_string())?;
    let mut reader = response.into_reader().take(MAX_BYTES + 1);
    let mut file = fs::File::create(to).map_err(|e| e.to_string())?;
    let (mut hasher, mut buf, mut total) = (Sha256::new(), [0u8; 64 * 1024], 0u64);
    let mut first = [0u8; 2];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if total == 0 && n >= 2 {
            first.copy_from_slice(&buf[..2]);
        }
        total += n as u64;
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    let digest: [u8; 32] = hasher.finalize().into();
    if total > MAX_BYTES || &first != b"MZ" || sha256_hex(&digest) != expected {
        let _ = fs::remove_file(to);
        return Err("O arquivo baixado não passou na verificação.".into());
    }
    Ok(())
}
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .user_agent("Seek-updater")
        .build()
}

/// Confere o GitHub e, havendo versão mais nova, baixa e instala. `Ok(Some(versão))` se instalou.
pub fn check_and_install(exe: &Path) -> Result<Option<String>, String> {
    let json: serde_json::Value = agent()
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let Some(release) = parse_release(&json) else {
        return Ok(None);
    };
    if !is_newer(&release.version, VERSION) {
        return Ok(None);
    }
    let staged = sibling(exe, "update");
    download(&release.url, &staged, &release.sha256)?;
    swap(exe, &staged).map_err(|e| format!("Não foi possível trocar o arquivo: {e}"))?;
    Ok(Some(release.version))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v0.1.10", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("lixo", "0.1.0"));
    }
    #[test]
    fn release_needs_hash_and_own_repo_url() {
        let ok = |url: &str, digest: Option<&str>| {
            let mut asset = serde_json::json!({"name": ASSET, "browser_download_url": url});
            if let Some(d) = digest {
                asset["digest"] = d.into();
            }
            serde_json::json!({"tag_name": "v0.2.0", "assets": [asset]})
        };
        let good = format!("https://github.com/{REPO}/releases/download/v0.2.0/Seek.exe");
        let r = parse_release(&ok(&good, Some("sha256:ABCDEF"))).unwrap();
        assert_eq!((r.version.as_str(), r.sha256.as_str()), ("0.2.0", "abcdef"));
        assert!(parse_release(&ok(&good, None)).is_none());
        assert!(parse_release(&ok("https://evil.example/Seek.exe", Some("sha256:ab"))).is_none());
    }
    #[test]
    fn swap_replaces_and_cleanup_removes_old() {
        let dir = std::env::temp_dir().join(format!("seek-upd-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (exe, new) = (dir.join("Seek.exe"), dir.join("Seek.update"));
        fs::write(&exe, b"antigo").unwrap();
        fs::write(&new, b"novo").unwrap();
        swap(&exe, &new).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"novo");
        assert_eq!(fs::read(dir.join("Seek.old")).unwrap(), b"antigo");
        cleanup(&exe);
        assert!(!dir.join("Seek.old").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
