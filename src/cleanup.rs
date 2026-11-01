//! O que muda o disco: mandar para a Lixeira (e trazer de volta), mover pasta deixando uma junction no lugar,
//! limpezas oficiais do Windows e o registro de tudo (`acoes.log`), com Desfazer.
use crate::knowledge::{self, Act};
use std::{
    fs,
    io::Write,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}
fn drive_root(path: &Path) -> Option<PathBuf> {
    let s = path.to_string_lossy();
    (s.len() >= 2 && s.as_bytes()[1] == b':').then(|| PathBuf::from(format!("{}\\", &s[..2])))
}
/// Lixeira só existe em disco fixo: em pen drive, cartão ou rede o Windows apagaria de vez.
pub fn has_recycle_bin(path: &Path) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    drive_root(path).is_some_and(|r| unsafe { GetDriveTypeW(wide(r.as_os_str()).as_ptr()) } == 3)
}

/// Manda para a Lixeira, um por um. Se o Windows fosse apagar de vez (item grande demais para a Lixeira),
/// ele mesmo pergunta antes (`FOF_WANTNUKEWARNING`).
pub fn to_recycle_bin(path: &Path) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, SHFILEOPSTRUCTW, SHFileOperationW,
    };
    if let Some(why) = knowledge::protected(path, false) {
        return Err(why.into());
    }
    if !has_recycle_bin(path) {
        return Err("Este disco não tem Lixeira (pen drive, cartão ou rede): o Windows apagaria de vez.".into());
    }
    if fs::symlink_metadata(path).is_err() {
        return Err("Não existe mais.".into());
    }
    let mut from = wide(path.as_os_str());
    from.push(0); // lista terminada em dois zeros
    let mut op: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    op.wFunc = FO_DELETE;
    op.pFrom = from.as_ptr();
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT | FOF_WANTNUKEWARNING) as _;
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 || op.fAnyOperationsAborted != 0 {
        return Err(if fs::symlink_metadata(path).is_ok() { "Em uso ou sem permissão.".into() } else { format!("O Windows avisou um erro ({code}).") });
    }
    if fs::symlink_metadata(path).is_ok() {
        return Err("Parte dos arquivos ficou (em uso ou sem permissão).".into());
    }
    Ok(())
}
/// Item da Lixeira que veio de `original`: o mais recente, se houver vários.
fn find_in_bin(original: &Path) -> Option<(PathBuf, PathBuf)> {
    let bin = drive_root(original)?.join("$Recycle.Bin");
    let want = crate::normalize(&original.to_string_lossy());
    let mut best: Option<(u64, PathBuf, PathBuf)> = None;
    for user in fs::read_dir(bin).ok()?.flatten() {
        let Ok(items) = fs::read_dir(user.path()) else { continue };
        for item in items.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix("$I") else { continue };
            let Ok(data) = fs::read(item.path()) else { continue };
            let Some((when, path)) = parse_info(&data) else { continue };
            if crate::normalize(&path) == want && best.as_ref().is_none_or(|b| when > b.0) {
                best = Some((when, item.path(), user.path().join(format!("$R{rest}"))));
            }
        }
    }
    best.map(|(_, i, r)| (i, r))
}
/// Arquivo `$I` da Lixeira: (quando foi apagado, caminho original).
fn parse_info(data: &[u8]) -> Option<(u64, String)> {
    let u64_at = |at: usize| Some(u64::from_le_bytes(data.get(at..at + 8)?.try_into().ok()?));
    let version = u64_at(0)?;
    let when = u64_at(16)?;
    let units: Vec<u16> = match version {
        1 => data.get(24..)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
        2 => {
            let n = u32::from_le_bytes(data.get(24..28)?.try_into().ok()?) as usize;
            data.get(28..28 + n * 2)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
        }
        _ => return None,
    };
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    Some((when, String::from_utf16_lossy(&units[..end])))
}
/// Traz de volta da Lixeira para o lugar de onde saiu.
pub fn restore(original: &Path) -> Result<(), String> {
    if fs::symlink_metadata(original).is_ok() {
        return Err("Já existe outro item com esse nome no lugar original.".into());
    }
    let (info, data) = find_in_bin(original).ok_or("Não está mais na Lixeira (ela foi esvaziada?).")?;
    if let Some(dir) = original.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::rename(&data, original).map_err(|e| format!("Não foi possível restaurar: {e}"))?;
    let _ = fs::remove_file(info);
    Ok(())
}
/// Tamanho e quantidade de itens na Lixeira de todos os discos.
pub fn recycle_bin_size() -> (u64, u64) {
    use windows_sys::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};
    let mut info: SHQUERYRBINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHQUERYRBINFO>() as u32;
    if unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) } == 0 {
        (info.i64Size as u64, info.i64NumItems as u64)
    } else {
        (0, 0)
    }
}

// ---- mover pasta deixando uma junction ----
fn walk(dir: &Path, out: &mut (u64, u64)) -> Result<(), String> {
    for item in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let item = item.map_err(|e| e.to_string())?;
        let meta = fs::symlink_metadata(item.path()).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            return Err(format!("{} é um atalho especial (link): o Seek não move pastas com isso dentro.", item.path().display()));
        }
        if meta.is_dir() {
            walk(&item.path(), out)?;
        } else {
            out.0 += 1;
            out.1 += meta.len();
        }
    }
    Ok(())
}
/// (arquivos, bytes) de uma pasta. Recusa pastas com links dentro.
pub fn folder_totals(dir: &Path) -> Result<(u64, u64), String> {
    let mut t = (0, 0);
    walk(dir, &mut t)?;
    Ok(t)
}
fn copy_tree(from: &Path, to: &Path, done: &AtomicU64, cancel: &AtomicBool) -> Result<(), String> {
    fs::create_dir(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for item in fs::read_dir(from).map_err(|e| e.to_string())? {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelado.".into());
        }
        let item = item.map_err(|e| e.to_string())?;
        let (src, dst) = (item.path(), to.join(item.file_name()));
        let meta = fs::symlink_metadata(&src).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            copy_tree(&src, &dst, done, cancel)?;
        } else {
            let n = fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
            done.fetch_add(n, Ordering::Relaxed);
        }
    }
    Ok(())
}
pub fn free_space(path: &Path) -> u64 {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let Some(root) = drive_root(path) else { return 0 };
    let (mut free, mut total, mut all) = (0u64, 0u64, 0u64);
    unsafe { GetDiskFreeSpaceExW(wide(root.as_os_str()).as_ptr(), &mut free, &mut total, &mut all) };
    free
}
fn make_junction(link: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("cmd.exe")
        .raw_arg(format!("/c mklink /J \"{}\" \"{}\"", link.display(), target.display()))
        .creation_flags(0x0800_0000) // sem janela
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stdout).trim().to_owned()) }
}
/// Move `src` para dentro de `dest_parent` (outro disco) e deixa no lugar uma junction apontando para lá:
/// programas e jogos continuam achando tudo no caminho antigo. Passo a passo, com volta atrás em cada erro:
/// copia, confere, renomeia a original, cria a junction, confere de novo e só então apaga a original.
pub fn move_with_junction(src: &Path, dest_parent: &Path, done: &AtomicU64, cancel: &AtomicBool) -> Result<PathBuf, String> {
    if let Some(why) = knowledge::protected(src, true) {
        return Err(why.into());
    }
    let meta = fs::symlink_metadata(src).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        return Err("Esta pasta já é um atalho (junction) para outro lugar.".into());
    }
    if !meta.is_dir() {
        return Err("Só pastas podem ser movidas assim.".into());
    }
    if drive_root(src) == drive_root(dest_parent) {
        return Err("Escolha outro disco.".into());
    }
    let name = src.file_name().ok_or("Pasta inválida.")?;
    let dest = dest_parent.join(name);
    if fs::symlink_metadata(&dest).is_ok() {
        return Err(format!("Já existe {} no destino.", dest.display()));
    }
    let (files, size) = folder_totals(src)?;
    if free_space(dest_parent) < size + (1 << 30) {
        return Err("Não cabe no disco de destino (precisa sobrar 1 GB).".into());
    }
    fs::create_dir_all(dest_parent).map_err(|e| e.to_string())?;
    if let Err(e) = copy_tree(src, &dest, done, cancel) {
        let _ = fs::remove_dir_all(&dest);
        return Err(e);
    }
    if folder_totals(&dest).ok() != Some((files, size)) {
        let _ = fs::remove_dir_all(&dest);
        return Err("A cópia não bateu com a original; nada foi mudado.".into());
    }
    let old = src.with_file_name(format!("{}.seek-antigo", name.to_string_lossy()));
    if let Err(e) = fs::rename(src, &old) {
        let _ = fs::remove_dir_all(&dest);
        return Err(format!("Algum arquivo está em uso (feche o programa e tente de novo): {e}"));
    }
    let linked = make_junction(src, &dest).and_then(|_| match folder_totals(src) {
        Ok(t) if t == (files, size) => Ok(()),
        _ => Err("A junction não abriu a pasta nova.".into()),
    });
    if let Err(e) = linked {
        let _ = fs::remove_dir(src); // tira só o link, se ele chegou a ser criado
        let _ = fs::rename(&old, src);
        let _ = fs::remove_dir_all(&dest);
        return Err(format!("Não deu para criar o atalho no lugar antigo: {e}"));
    }
    fs::remove_dir_all(&old).map_err(|e| format!("Movido, mas a cópia antiga ficou em {} ({e}). Apague depois.", old.display()))?;
    Ok(dest)
}
/// Desfaz `move_with_junction`: traz a pasta de volta e tira a junction.
pub fn undo_junction(link: &Path, dest: &Path, done: &AtomicU64, cancel: &AtomicBool) -> Result<(), String> {
    let meta = fs::symlink_metadata(link).map_err(|_| "O atalho no lugar antigo não existe mais.")?;
    if !meta.file_type().is_symlink() {
        return Err("No lugar antigo já existe uma pasta de verdade.".into());
    }
    let (files, size) = folder_totals(dest)?;
    if free_space(link) < size + (1 << 30) {
        return Err("Não cabe de volta no disco original (precisa sobrar 1 GB).".into());
    }
    let back = link.with_file_name(format!("{}.seek-volta", link.file_name().unwrap_or_default().to_string_lossy()));
    if let Err(e) = copy_tree(dest, &back, done, cancel) {
        let _ = fs::remove_dir_all(&back);
        return Err(e);
    }
    if folder_totals(&back).ok() != Some((files, size)) {
        let _ = fs::remove_dir_all(&back);
        return Err("A cópia não bateu; nada foi mudado.".into());
    }
    fs::remove_dir(link).map_err(|e| e.to_string())?;
    fs::rename(&back, link).map_err(|e| format!("A pasta voltou para {} ({e}); renomeie à mão.", back.display()))?;
    fs::remove_dir_all(dest).map_err(|e| format!("Voltou, mas a cópia em {} ficou ({e}).", dest.display()))
}

// ---- limpezas oficiais do Windows ----
fn shell(file: &str, params: &str, admin: bool) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let verb = wide(std::ffi::OsStr::new(if admin { "runas" } else { "open" }));
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            wide(std::ffi::OsStr::new(file)).as_ptr(),
            wide(std::ffi::OsStr::new(params)).as_ptr(),
            std::ptr::null(),
            1,
        )
    };
    if r as usize > 32 { Ok(()) } else { Err("O Windows não abriu (permissão negada?).".into()) }
}
/// Abre a ferramenta oficial do Windows para o item. As de linha de comando ficam numa janela visível.
pub fn run_official(act: Act, drive: &str) -> Result<(), String> {
    match act {
        Act::Cleanmgr => shell("cleanmgr.exe", &format!("/d {}", &drive[..1]), false),
        Act::Dism => shell("cmd.exe", "/k Dism.exe /Online /Cleanup-Image /StartComponentCleanup", true),
        Act::Hibernate => shell("cmd.exe", "/k powercfg /h off", true),
        Act::Restore => shell("SystemPropertiesProtection.exe", "", false),
        Act::EmptyBin => {
            // O próprio Windows pergunta antes de esvaziar.
            std::thread::spawn(|| unsafe { windows_sys::Win32::UI::Shell::SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), 0) });
            Ok(())
        }
        Act::None => Ok(()),
    }
}
pub fn official_label(act: Act) -> &'static str {
    match act {
        Act::Cleanmgr => "Abrir a Limpeza de Disco",
        Act::Dism => "Limpar componentes antigos (DISM)",
        Act::Hibernate => "Desligar a hibernação",
        Act::Restore => "Abrir os pontos de restauração",
        Act::EmptyBin => "Esvaziar a Lixeira",
        Act::None => "",
    }
}

// ---- registro de ações ----
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Recycle,  // mandado para a Lixeira
    Junction, // pasta movida, com junction no lugar
}
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub at: u64,
    pub kind: Kind,
    pub undone: bool,
    pub bytes: u64,
    pub from: PathBuf,
    pub to: PathBuf, // destino (junction); vazio na Lixeira
}
pub struct Actions {
    file: Option<PathBuf>,
    pub list: Vec<Action>,
}
impl Actions {
    pub fn open() -> Self {
        let file = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join("acoes.log")));
        Self::at(file)
    }
    pub fn at(file: Option<PathBuf>) -> Self {
        let text = file.as_ref().and_then(|f| fs::read_to_string(f).ok()).unwrap_or_default();
        let list = text
            .lines()
            .filter_map(|l| {
                let p: Vec<&str> = l.split('|').collect();
                let [at, kind, undone, bytes, from, to] = p.as_slice() else { return None };
                Some(Action {
                    at: at.parse().ok()?,
                    kind: match *kind {
                        "lixeira" => Kind::Recycle,
                        "junction" => Kind::Junction,
                        _ => return None,
                    },
                    undone: *undone == "1",
                    bytes: bytes.parse().ok()?,
                    from: PathBuf::from(from),
                    to: PathBuf::from(to),
                })
            })
            .collect();
        Self { file, list }
    }
    fn save(&self) {
        let Some(file) = &self.file else { return };
        let tmp = file.with_extension("tmp");
        let ok = (|| -> std::io::Result<()> {
            let mut w = std::io::BufWriter::new(fs::File::create(&tmp)?);
            for a in &self.list {
                let kind = match a.kind {
                    Kind::Recycle => "lixeira",
                    Kind::Junction => "junction",
                };
                writeln!(w, "{}|{kind}|{}|{}|{}|{}", a.at, u8::from(a.undone), a.bytes, a.from.display(), a.to.display())?;
            }
            w.flush()
        })();
        if ok.is_ok() {
            let _ = fs::rename(&tmp, file);
        }
    }
    pub fn push(&mut self, a: Action) {
        self.list.push(a);
        self.save();
    }
    pub fn mark_undone(&mut self, i: usize) {
        if let Some(a) = self.list.get_mut(i) {
            a.undone = true;
        }
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recycle_bin_info_files_are_parsed() {
        let mut v2 = 2u64.to_le_bytes().to_vec();
        v2.extend(5u64.to_le_bytes());
        v2.extend(77u64.to_le_bytes());
        let path: Vec<u16> = "C:\\a\\é.txt\0".encode_utf16().collect();
        v2.extend((path.len() as u32).to_le_bytes());
        v2.extend(path.iter().flat_map(|u| u.to_le_bytes()));
        assert_eq!(parse_info(&v2), Some((77, "C:\\a\\é.txt".to_string())));
        let mut v1 = 1u64.to_le_bytes().to_vec();
        v1.extend(5u64.to_le_bytes());
        v1.extend(9u64.to_le_bytes());
        let mut fixed = path.clone();
        fixed.resize(260, 0);
        v1.extend(fixed.iter().flat_map(|u| u.to_le_bytes()));
        assert_eq!(parse_info(&v1), Some((9, "C:\\a\\é.txt".to_string())));
        assert_eq!(parse_info(b"lixo"), None);
    }
    #[test]
    fn recycle_and_restore_a_temp_file() {
        // TEMP costuma vir no formato curto (ADMINI~1); a Lixeira guarda o caminho longo, como o índice.
        let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
        let temp = PathBuf::from(temp.to_string_lossy().trim_start_matches("\\\\?\\"));
        let dir = temp.join(format!("seek-lixeira-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("apagar-teste.txt");
        fs::write(&f, b"oi").unwrap();
        if !has_recycle_bin(&f) {
            return; // TEMP num disco sem Lixeira
        }
        to_recycle_bin(&f).unwrap();
        assert!(!f.exists());
        restore(&f).unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"oi");
        let _ = fs::remove_dir_all(dir);
    }
    /// Mover de verdade entre dois discos: `SEEK_TEST_DEST=D:\pasta-de-teste cargo test -- --ignored across_disks`
    #[test]
    #[ignore]
    fn junction_move_across_disks_and_undo() {
        let Ok(dest_root) = std::env::var("SEEK_TEST_DEST") else { return };
        let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
        let src_root = PathBuf::from(temp.to_string_lossy().trim_start_matches("\\\\?\\")).join(format!("seek-junc-{}", std::process::id()));
        let src = src_root.join("Minha Pasta é");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/a.bin"), vec![7u8; 300_000]).unwrap();
        fs::write(src.join("b.txt"), b"oi").unwrap();
        let dest_parent = PathBuf::from(&dest_root);
        let (done, cancel) = (AtomicU64::new(0), AtomicBool::new(false));
        let dest = move_with_junction(&src, &dest_parent, &done, &cancel).unwrap();
        assert!(fs::symlink_metadata(&src).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(src.join("b.txt")).unwrap(), b"oi"); // abre pelo caminho antigo
        assert_eq!(folder_totals(&dest).unwrap(), (2, 300_002));
        assert!(!src_root.join("Minha Pasta é.seek-antigo").exists());
        undo_junction(&src, &dest, &done, &cancel).unwrap();
        assert!(!fs::symlink_metadata(&src).unwrap().file_type().is_symlink());
        assert_eq!(folder_totals(&src).unwrap(), (2, 300_002));
        assert!(!dest.exists());
        fs::remove_dir_all(src_root).unwrap();
        let _ = fs::remove_dir(&dest_parent);
    }
    #[test]
    fn junction_move_and_undo_on_same_disk_is_refused_but_logic_runs_across_dirs() {
        // Mover exige outro disco; aqui só confere a recusa e o registro.
        let dir = std::env::temp_dir().join(format!("seek-junc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/sub")).unwrap();
        fs::write(dir.join("src/sub/a.bin"), vec![1u8; 1000]).unwrap();
        let (done, cancel) = (AtomicU64::new(0), AtomicBool::new(false));
        assert!(move_with_junction(&dir.join("src"), &dir.join("dest"), &done, &cancel).is_err());
        assert_eq!(folder_totals(&dir.join("src")).unwrap(), (1, 1000));
        let log_file = dir.join("acoes.log");
        let mut log = Actions::at(Some(log_file.clone()));
        log.push(Action { at: 5, kind: Kind::Junction, undone: false, bytes: 1000, from: "C:\\a b".into(), to: "D:\\a b".into() });
        log.mark_undone(0);
        assert_eq!(Actions::at(Some(log_file)).list, log.list);
        let _ = fs::remove_dir_all(dir);
    }
}
