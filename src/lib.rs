use rayon::prelude::*;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
pub mod clock;
pub mod icon_search;
pub mod log;
#[cfg(windows)]
pub mod shellthumb;
pub mod update;
#[cfg(windows)]
pub mod watch;
#[cfg(windows)]
pub mod mft;

/// Minúsculas, sem acentos e com `\` no lugar de `/`: é a forma usada para buscar e comparar caminhos.
pub fn normalize(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c == '/' { '\\' } else { c })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Photo,
    Video,
    Audio,
    Document,
    App,
    Code,
    Archive,
    Font,
    Design,
    Disk,
    Data,
    System,
    Other,
}
// Extensões conhecidas por categoria (minúsculas, sem ponto). Se uma extensão aparece em duas
// listas, vale a primeira. Qualquer extensão que não esteja aqui continua pesquisável por `.ext`.
const KNOWN: [(Kind, &str); 12] = [
    (Kind::App, "exe msi msix msixbundle appx appxbundle bat cmd com scr lnk url apk aab xapk ipa app deb rpm pkg appimage cpl msp msu gadget jar pif"),
    (Kind::Photo, "jpg jpeg jpe jfif png gif webp bmp dib tif tiff heic heif hif avif jxl svg svgz ico icns cur raw cr2 cr3 crw nef nrw arw srf sr2 dng orf raf rw2 pef srw x3f 3fr erf mef mos mrw kdc dcr tga pcx pbm pgm ppm pnm exr hdr dds pic pct jp2 j2k jpx jpf wdp jxr apng bpg flif xbm xpm wbmp"),
    (Kind::Video, "mp4 m4v mkv mov avi wmv flv f4v webm mpg mpeg mpe m2v m2ts mts vob ogv ogm 3gp 3g2 asf rm rmvb divx xvid mxf dv bik smk braw r3d srt sub ass ssa vtt"),
    (Kind::Audio, "mp3 wav flac aac ogg oga opus m4a m4b wma aiff aif aifc ape wv mka mid midi kar amr au snd ra dsf dff caf ac3 dts mp2 mpc tta 3ga weba ogx spx gsm voc xm it s3m flp als ptx cpr rpp sng"),
    (Kind::Document, "pdf doc docx docm dot dotx dotm rtf odt ott txt text md markdown rst tex latex epub mobi azw azw3 fb2 djvu xps oxps pages wps wpd abw lit chm hlp xls xlsx xlsm xlsb xlt xltx xltm ods ots csv tsv numbers ppt pptx pptm pps ppsx pot potx odp otp key msg eml emlx pst ost vcf ics one onetoc2 pub vsd vsdx mpp nfo ps rtfd gdoc gsheet gslides"),
    (Kind::Code, "c h cpp cc cxx hpp hh hxx cs csx vb vbs fs fsx java kt kts scala groovy gradle py pyw pyc pyd ipynb rb erb rs go swift m mm js mjs cjs jsx ts tsx vue svelte astro html htm xhtml css scss sass less php phtml pl pm t lua r rmd jl dart sh bash zsh fish ps1 psm1 psd1 sql asm s nasm inc cmake mk proto thrift graphql gql sol hs lhs ml mli elm clj cljs cljc edn erl hrl ex exs nim zig sv vhd vhdl tcl awk pas pp dpr lisp lsp el scm rkt cob cbl f90 f95 ada adb ads bas vbproj csproj sln vcxproj pro pri qml gd tscn wsf"),
    (Kind::Archive, "zip zipx rar r00 7z tar gz tgz bz2 tbz2 xz txz zst lz lz4 lzma lzh lha arj cab z cpio war ear sit sitx ace alz egg wim swm esd zpaq br tlz taz tbz"),
    (Kind::Font, "ttf otf ttc woff woff2 eot fon fnt pfb pfm pfa afm bdf pcf dfont otb"),
    (Kind::Design, "psd psb xcf kra ai eps indd indt idml cdr sketch fig xd afdesign afphoto afpub aseprite pdn blend blend1 fbx obj stl 3ds max ma mb c4d dae gltf glb usd usda usdc usdz ply 3mf step stp iges igs dwg dxf dwf rvt rfa skp ifc sldprt sldasm slddrw ipt iam prt catpart catproduct f3d x_t x_b 3dm lwo lws wrl x3d vrml zpr ztl uasset umap uproject unitypackage spp sbs sbsar hip hipnc bvh gbr drl"),
    (Kind::Disk, "iso img cue nrg mds ccd dmg vhd vhdx vmdk vdi qcow qcow2 ova ovf toast gho tib"),
    (Kind::Data, "json jsonl ndjson xml xaml yaml yml toml ini cfg conf config properties env plist reg db sqlite sqlite3 db3 mdb accdb mdf ldf sdf frm myd myi ibd dbf bson parquet avro orc arrow feather h5 hdf5 npy npz pkl pickle mat sav dta rds rdata sas7bdat log dmp trace etl evtx tmp temp bak old swp cache lock idx dat data bin dump gpx kml kmz geojson shp gdb mmdb torrent crt cer pem key pfx p12 kdbx ovpn nbt"),
    (Kind::System, "dll sys drv ocx ax efi cat inf mui manifest msc nls pdb lib exp ilk o a so dylib ko class wasm node bpl tlb olb sdb evt mum hiv blf mof pnf nlp rll dic adm admx adml theme themepack msstyles scf"),
];
fn known() -> &'static std::collections::HashMap<&'static str, Kind> {
    static MAP: std::sync::OnceLock<std::collections::HashMap<&'static str, Kind>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        for (kind, list) in KNOWN {
            for ext in list.split_whitespace() {
                m.entry(ext).or_insert(kind);
            }
        }
        m
    })
}
impl Kind {
    pub const ALL: [Self; 13] = [
        Self::Photo,
        Self::Video,
        Self::Audio,
        Self::Document,
        Self::App,
        Self::Code,
        Self::Archive,
        Self::Font,
        Self::Design,
        Self::Disk,
        Self::Data,
        Self::System,
        Self::Other,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Photo => "Fotos",
            Self::Video => "Vídeos",
            Self::Audio => "Áudios",
            Self::Document => "Documentos",
            Self::App => "Aplicativos",
            Self::Code => "Código",
            Self::Archive => "Compactados",
            Self::Font => "Fontes",
            Self::Design => "Design e 3D",
            Self::Disk => "Imagens de disco",
            Self::Data => "Dados",
            Self::System => "Sistema",
            Self::Other => "Outros",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "foto" | "fotos" | "imagem" | "imagens" => Some(Self::Photo),
            "video" | "videos" | "filme" | "filmes" => Some(Self::Video),
            "audio" | "audios" | "musica" | "musicas" => Some(Self::Audio),
            "documento" | "documentos" | "texto" | "textos" | "planilha" | "planilhas"
            | "apresentacao" | "apresentacoes" => Some(Self::Document),
            "app" | "apps" | "aplicativo" | "aplicativos" | "programa" | "programas"
            | "executavel" | "executaveis" => Some(Self::App),
            "codigo" | "codigos" | "script" | "scripts" | "programacao" => Some(Self::Code),
            "compactado" | "compactados" | "arquivo-zip" => Some(Self::Archive),
            "fonte" | "fontes" | "tipografia" => Some(Self::Font),
            "design" | "3d" | "modelo3d" | "cad" => Some(Self::Design),
            "imagem-de-disco" | "imagens-de-disco" => Some(Self::Disk),
            "dados" | "banco" | "bancos" => Some(Self::Data),
            "sistema" => Some(Self::System),
            "outro" | "outros" => Some(Self::Other),
            _ => None,
        }
    }
    pub fn for_ext(ext: &str) -> Self {
        known().get(ext).copied().unwrap_or(Self::Other)
    }
}

/// Um arquivo do índice. Guarda só o caminho e a versão normalizada (minúsculas, sem acentos);
/// nome e extensão saem desses textos na hora, o que corta mais da metade da memória.
#[derive(Clone)]
pub struct Entry {
    path: Box<str>,
    pub searchable: Box<str>,
    pub bytes: u64,
    pub modified: u64, // segundos desde 1970 (0 = desconhecido)
    pub created: u64,
    pub kind: Kind,
    pub size_known: bool,
}
impl Entry {
    pub fn new(path: PathBuf, bytes: u64) -> Self {
        let path: Box<str> = path
            .into_os_string()
            .into_string()
            .unwrap_or_else(|o| o.to_string_lossy().into_owned())
            .into();
        let searchable: Box<str> = normalize(&path).into();
        let mut e = Self { path, searchable, bytes, modified: 0, created: 0, kind: Kind::Other, size_known: true };
        e.kind = Kind::for_ext(e.ext());
        e
    }
    pub fn with_modified(mut self, modified: u64) -> Self {
        self.modified = modified;
        self
    }
    pub fn with_created(mut self, created: u64) -> Self {
        self.created = created;
        self
    }
    pub fn path(&self) -> &Path {
        Path::new(&*self.path)
    }
    /// Caminho completo, com maiúsculas e acentos como no disco.
    pub fn path_str(&self) -> &str {
        &self.path
    }
    /// Nome do arquivo com a grafia original.
    pub fn name(&self) -> &str {
        let p = &*self.path;
        &p[p.rfind(['\\', '/']).map_or(0, |i| i + 1)..]
    }
    /// Extensão normalizada (minúsculas, sem ponto); vazia se não houver.
    pub fn ext(&self) -> &str {
        let s = &*self.searchable;
        let n = &s[s.rfind(['\\', '/']).map_or(0, |i| i + 1)..];
        match n.rfind('.') {
            Some(i) if i > 0 => &n[i + 1..],
            _ => "",
        }
    }
    /// Nome normalizado (minúsculas, sem acentos), bom para ordenar.
    pub fn name_key(&self) -> &str {
        let s = &*self.searchable;
        &s[s.rfind(['\\', '/']).map_or(0, |i| i + 1)..]
    }
    /// Pasta que contém o arquivo.
    pub fn folder(&self) -> &str {
        let p = &*self.path;
        p.rfind(['\\', '/']).map_or("", |i| &p[..i])
    }
}
/// Segundos desde 1970 de um horário do sistema (0 se anterior ou inválido).
pub fn unix_secs(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
pub struct Scan {
    pub entries: Arc<Vec<Entry>>,
    pub errors: usize,
    pub elapsed: std::time::Duration,
    pub cached: bool,
    pub cache_saved: bool,
    pub cancelled: bool,
    pub turbo: bool, // leitura direta da MFT (administrador)
}
/// Raiz especial: indexa todos os discos fixos do computador de uma vez.
pub const ALL_DISKS: &str = "Todos os discos";
pub fn is_all(root: &Path) -> bool {
    root == Path::new(ALL_DISKS)
}
/// Discos locais com mídia: HD, SSD, HD externo e pen drive (`C:\`, `D:\`...). Sem CD vazio nem pasta de rede.
#[cfg(windows)]
pub fn fixed_drives() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
            if kind != 2 && kind != 3 {
                return false; // 3 = fixo, 2 = removível (pen drive, cartão)
            }
            let (mut free, mut total, mut all) = (0u64, 0u64, 0u64);
            unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut all) != 0 && total > 0 }
        })
        .map(PathBuf::from)
        .collect()
}
pub fn scan(root: &Path, cancel: &AtomicBool, progress: &AtomicUsize) -> Scan {
    #[cfg(windows)]
    if is_all(root) {
        let start = Instant::now();
        let (mut all, mut errors, mut turbo) = (Vec::new(), 0, true);
        for drive in fixed_drives() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let part = scan(&drive, cancel, progress);
            errors += part.errors;
            turbo &= part.turbo;
            all.extend(Arc::try_unwrap(part.entries).unwrap_or_default());
        }
        all.par_sort_unstable_by(|a, b| a.searchable.cmp(&b.searchable));
        return Scan {
            entries: Arc::new(all),
            errors,
            elapsed: start.elapsed(),
            cached: false,
            cache_saved: false,
            cancelled: cancel.load(Ordering::Relaxed),
            turbo,
        };
    }
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;
    // Disco NTFS inteiro como administrador: lê a MFT em segundos. Senão, segue a leitura por pastas.
    #[cfg(windows)]
    if mft::drive_letter(root).is_some() {
        let start = Instant::now();
        let mft = mft::scan(root, cancel, progress);
        if let Err(reason) = &mft {
            log::line(&format!("leitura turbo de {} indisponível: {reason}", root.display()));
        }
        if let Ok(mut entries) = mft {
            entries.par_sort_unstable_by(|a, b| a.searchable.cmp(&b.searchable));
            return Scan {
                entries: Arc::new(entries),
                errors: 0,
                elapsed: start.elapsed(),
                cached: false,
                cache_saved: false,
                cancelled: cancel.load(Ordering::Relaxed),
                turbo: true,
            };
        }
    }
    struct Work {
        stack: Vec<PathBuf>,
        pending: usize, // pastas na fila + pastas em leitura
    }
    let start = Instant::now();
    let work = Mutex::new(Work {
        stack: vec![root.to_path_buf()],
        pending: 1,
    });
    let wake = Condvar::new();
    // shortcut: 8 leitores no máximo; em HD mecânico antigo menos threads podem ser mais rápidos.
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(2, 8);
    let parts: Vec<(Vec<Entry>, usize)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let (mut entries, mut errors) = (Vec::new(), 0);
                    loop {
                        let dir = {
                            let mut w = work.lock().unwrap();
                            loop {
                                if cancel.load(Ordering::Relaxed) || w.pending == 0 {
                                    break None;
                                }
                                if let Some(d) = w.stack.pop() {
                                    break Some(d);
                                }
                                w = wake.wait_timeout(w, Duration::from_millis(100)).unwrap().0;
                            }
                        };
                        let Some(dir) = dir else { break };
                        let mut subdirs = Vec::new();
                        let before = entries.len();
                        match fs::read_dir(&dir) {
                            Err(_) => errors += 1,
                            Ok(read) => {
                                for item in read {
                                    let Ok(item) = item else {
                                        errors += 1;
                                        continue;
                                    };
                                    let Ok(kind) = item.file_type() else {
                                        errors += 1;
                                        continue;
                                    };
                                    if kind.is_dir() {
                                        subdirs.push(item.path());
                                    } else if kind.is_file() {
                                        // No Windows o tamanho já vem da listagem da pasta, sem nova leitura do disco.
                                        match item.metadata() {
                                            Ok(meta) => entries.push(
                                                Entry::new(item.path(), meta.len())
                                                    .with_modified(meta.modified().map_or(0, unix_secs)).with_created(meta.created().map_or(0, unix_secs)),
                                            ),
                                            Err(_) => {
                                                errors += 1;
                                                // Mesmo sem acesso ao tamanho, o nome encontrado continua pesquisável.
                                                let mut entry = Entry::new(item.path(), 0);
                                                entry.size_known = false;
                                                entries.push(entry);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        progress.fetch_add(entries.len() - before, Ordering::Relaxed);
                        let mut w = work.lock().unwrap();
                        w.pending = w.pending + subdirs.len() - 1;
                        w.stack.extend(subdirs);
                        wake.notify_all();
                    }
                    (entries, errors)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let errors = parts.iter().map(|p| p.1).sum();
    let mut entries: Vec<Entry> = parts.into_iter().flat_map(|p| p.0).collect();
    entries.par_sort_unstable_by(|a, b| a.searchable.cmp(&b.searchable));
    Scan {
        entries: Arc::new(entries),
        errors,
        elapsed: start.elapsed(),
        cached: false,
        cache_saved: false,
        cancelled: cancel.load(Ordering::Relaxed),
        turbo: false,
    }
}

// Store only paths and sizes; rebuild search strings on load. No file contents are read.
// UTF-16 preserves Windows filenames, including names not representable as UTF-8.
pub mod cache {
    use super::*;
    use std::{
        hash::{Hash, Hasher},
        io::{self, BufReader, BufWriter, Read, Write},
    };
    #[cfg(windows)]
    fn encode(path: &Path) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().collect()
    }
    #[cfg(windows)]
    fn decode(data: &[u16]) -> PathBuf {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(data).into()
    }
    #[cfg(not(windows))]
    fn encode(path: &Path) -> Vec<u16> {
        path.to_string_lossy().encode_utf16().collect()
    }
    #[cfg(not(windows))]
    fn decode(data: &[u16]) -> PathBuf {
        String::from_utf16_lossy(data).into()
    }
    pub fn location(root: &Path) -> Option<PathBuf> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        root.hash(&mut h);
        Some(
            std::env::current_exe()
                .ok()?
                .parent()?
                .join("indices")
                .join(format!("{:016x}.velo", h.finish())),
        )
    }
    pub fn remember_root(root: &Path) {
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
                && let Ok(mut file) = fs::File::create(dir.join("ultima-pasta.velo")) {
                    let _ = path_write(&mut file, root);
                }
    }
    pub fn previous_root() -> Option<PathBuf> {
        let path = std::env::current_exe()
            .ok()?
            .parent()?
            .join("ultima-pasta.velo");
        path_read(&mut fs::File::open(path).ok()?).ok()
    }
    fn u64_read(r: &mut impl Read) -> io::Result<u64> {
        let mut b = [0; 8];
        r.read_exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn path_write(w: &mut impl Write, path: &Path) -> io::Result<()> {
        let path = encode(path);
        w.write_all(&(path.len() as u64).to_le_bytes())?;
        for unit in path {
            w.write_all(&unit.to_le_bytes())?;
        }
        Ok(())
    }
    fn path_read(r: &mut impl Read) -> io::Result<PathBuf> {
        let len = u64_read(r)?;
        if len > 32768 {
            return Err(io::Error::other("Caminho inválido no índice"));
        }
        let mut buf = vec![0; len as usize * 2];
        r.read_exact(&mut buf)?;
        Ok(decode(
            &buf.as_chunks::<2>().0.iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        ))
    }
    pub fn save(file: &Path, root: &Path, entries: &[Entry], errors: usize) -> io::Result<()> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp = file.with_extension(format!("{}-{nonce}.tmp", std::process::id()));
        let result = (|| {
            let mut w = BufWriter::new(fs::File::create(&temp)?);
            w.write_all(b"VELO0004")?;
            path_write(&mut w, root)?;
            w.write_all(&(errors as u64).to_le_bytes())?;
            w.write_all(&(entries.len() as u64).to_le_bytes())?;
            for e in entries {
                path_write(&mut w, e.path())?;
                w.write_all(&e.bytes.to_le_bytes())?;
                w.write_all(&e.modified.to_le_bytes())?;
                w.write_all(&e.created.to_le_bytes())?;
                w.write_all(&[u8::from(e.size_known)])?;
            }
            w.flush()?;
            w.get_ref().sync_all()?;
            drop(w);
            // Removing only our own old cache is safe: a failed replacement triggers a rescan.
            if file.exists() {
                fs::remove_file(file)?;
            }
            fs::rename(&temp, file)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    pub fn load(
        file: &Path,
        root: &Path,
        cancel: &AtomicBool,
        progress: &AtomicUsize,
    ) -> io::Result<Scan> {
        let start = Instant::now();
        let mut r = BufReader::new(fs::File::open(file)?);
        let mut magic = [0; 8];
        r.read_exact(&mut magic)?;
        if &magic != b"VELO0004" || path_read(&mut r)? != root {
            return Err(io::Error::other("Índice incompatível"));
        }
        let errors = u64_read(&mut r)? as usize;
        let count = u64_read(&mut r)?;
        if count > 100_000_000 {
            return Err(io::Error::other("Índice inválido"));
        }
        let mut raw = Vec::with_capacity(count as usize);
        for i in 0..count {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelado"));
            }
            let path = path_read(&mut r)?;
            let bytes = u64_read(&mut r)?;
            let modified = u64_read(&mut r)?;
            let created = u64_read(&mut r)?;
            let mut known = [0];
            r.read_exact(&mut known)?;
            if known[0] > 1 {
                return Err(io::Error::other("Tamanho inválido no índice"));
            }
            raw.push((path, bytes, modified, created, known[0] == 1));
            progress.store(i as usize + 1, Ordering::Relaxed);
        }
        // Normalizar milhões de caminhos é a parte cara: usa todos os núcleos.
        let entries: Vec<Entry> = raw
            .into_par_iter()
            .map(|(path, bytes, modified, created, known)| {
                let mut entry = Entry::new(path, bytes).with_modified(modified).with_created(created);
                entry.size_known = known;
                entry
            })
            .collect();
        Ok(Scan {
            entries: Arc::new(entries),
            errors,
            elapsed: start.elapsed(),
            cached: true,
            cache_saved: true,
            cancelled: false,
            turbo: false,
        })
    }
}

#[derive(Clone, Debug)]
enum Predicate {
    Text(String),
    Name(String),
    Kind(Kind),
    Ext(String),
    Min(u64),
    Max(u64),
    Modified(u64, u64), // [início, fim) em segundos desde 1970
    Created(u64, u64),
}
#[derive(Clone, Debug)]
pub struct Query {
    groups: Vec<Vec<(bool, Predicate)>>,
}
fn size(s: &str) -> Result<u64, String> {
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(s.len());
    let n: f64 = s[..split]
        .replace(',', ".")
        .parse()
        .map_err(|_| format!("Tamanho inválido: {s}"))?;
    let unit = match &s[split..] {
        "" | "b" => 1.,
        "kb" => 1024.,
        "mb" => 1024. * 1024.,
        "gb" => 1024. * 1024. * 1024.,
        _ => return Err(format!("Unidade inválida: {s}. Use KB, MB ou GB.")),
    };
    let value = n * unit;
    if !value.is_finite() || value < 0. || value >= u64::MAX as f64 {
        return Err("Tamanho fora do limite".into());
    }
    Ok(value as u64)
}
impl Query {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut tokens = Vec::new();
        let mut token = String::new();
        let mut quoted = false;
        for c in normalize(raw).chars() {
            match c {
                '"' => quoted = !quoted,
                '|' if !quoted => {
                    if !token.is_empty() {
                        tokens.push(std::mem::take(&mut token));
                    }
                    tokens.push("|".into());
                }
                c if c.is_whitespace() && !quoted => {
                    if !token.is_empty() {
                        tokens.push(std::mem::take(&mut token));
                    }
                }
                _ => token.push(c),
            }
        }
        if quoted {
            return Err("Feche as aspas para buscar uma frase.".into());
        }
        if !token.is_empty() {
            tokens.push(token);
        }
        let mut groups = vec![Vec::new()];
        for token in tokens {
            if token == "|" {
                if groups.last().unwrap().is_empty() {
                    return Err("Escreva uma busca antes de |.".into());
                }
                groups.push(Vec::new());
                continue;
            }
            let (neg, value) = token
                .strip_prefix('-')
                .map_or((false, token.as_str()), |t| (true, t));
            if value.is_empty() {
                return Err("Escreva o termo depois de -.".into());
            }
            let pred = if let Some(v) = value.strip_prefix("ext:") {
                if v.trim_start_matches('.').is_empty() {
                    return Err("Informe a extensão, como ext:jpg.".into());
                }
                Predicate::Ext(v.trim_start_matches('.').into())
            } else if let Some(v) = value.strip_prefix("tipo:") {
                Predicate::Kind(Kind::parse(v).ok_or_else(|| format!("Tipo desconhecido: {v}"))?)
            } else if let Some(v) = value.strip_prefix("modificado:").or_else(|| value.strip_prefix("data:")) {
                let (lo, hi) = clock::date_range(v)?;
                Predicate::Modified(lo, hi)
            } else if let Some(v) = value.strip_prefix("criado:") {
                let (lo, hi) = clock::date_range(v)?;
                Predicate::Created(lo, hi)
            } else if let Some(v) = value.strip_prefix("maior:") {
                Predicate::Min(size(v)?)
            } else if let Some(v) = value.strip_prefix("menor:") {
                Predicate::Max(size(v)?)
            } else if let Some(v) = value.strip_prefix("nome:") {
                Predicate::Name(v.into())
            } else if value.starts_with('.')
                && value.len() > 1
                && !value[1..].contains('.')
                && !value.contains(['/', '\\'])
            {
                Predicate::Ext(value[1..].into())
            } else if let Some(kind) = Kind::parse(value) {
                Predicate::Kind(kind)
            } else {
                Predicate::Text(value.into())
            };
            groups.last_mut().unwrap().push((neg, pred));
        }
        if groups.len() > 1 && groups.last().unwrap().is_empty() {
            return Err("Escreva uma busca depois de |.".into());
        }
        Ok(Self { groups })
    }
    /// Palavras que a busca procura no nome (sem as negadas), para destacar nos resultados.
    pub fn terms(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (neg, p) in self.groups.iter().flatten() {
            if let (false, Predicate::Text(t) | Predicate::Name(t)) = (neg, p)
                && !out.contains(t) {
                    out.push(t.clone());
                }
        }
        out
    }
    pub fn matches(&self, e: &Entry) -> bool {
        self.groups.iter().any(|g| {
            g.iter().all(|(neg, p)| {
                let hit = match p {
                    Predicate::Text(t) => e.searchable.contains(t),
                    Predicate::Name(t) => e
                        .searchable
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or("")
                        .contains(t),
                    Predicate::Kind(k) => e.kind == *k,
                    Predicate::Ext(x) => e.ext() == x.as_str(),
                    Predicate::Min(n) => e.size_known && e.bytes > *n,
                    Predicate::Max(n) => e.size_known && e.bytes < *n,
                    Predicate::Modified(lo, hi) => e.modified > 0 && e.modified >= *lo && e.modified < *hi,
                    Predicate::Created(lo, hi) => e.created > 0 && e.created >= *lo && e.created < *hi,
                };
                hit != *neg
            })
        })
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Path,
    Name,
    Size,
    Date,
    Kind,
}
/// Reordena os resultados (índices do índice) pela coluna escolhida.
pub fn sort_results(entries: &[Entry], idx: &mut [usize], key: SortKey, desc: bool) {
    if key == SortKey::Path && !desc {
        idx.sort_unstable(); // o índice já está em ordem de caminho
        return;
    }
    idx.par_sort_by(|&a, &b| {
        let (x, y) = (&entries[a], &entries[b]);
        let o = match key {
            SortKey::Path => x.searchable.cmp(&y.searchable),
            SortKey::Name => x.name_key().cmp(y.name_key()),
            SortKey::Size => x.bytes.cmp(&y.bytes),
            SortKey::Date => x.modified.cmp(&y.modified),
            SortKey::Kind => x.ext().cmp(y.ext()).then_with(|| x.name_key().cmp(y.name_key())),
        };
        if desc { o.reverse() } else { o }
    });
}
/// Põe primeiro os arquivos cujo NOME é o que foi digitado (nome igual, começa com, contém, tem todas as
/// palavras) e deixa o resto na ordem atual. Acima de 300 mil resultados não reordena (custaria tempo demais).
pub fn rank_relevance(entries: &[Entry], idx: &mut [usize], query: &Query) {
    let terms = query.terms();
    if terms.is_empty() || idx.len() > 300_000 {
        return;
    }
    let phrase: String = terms.concat();
    idx.par_sort_by_cached_key(|&i| {
        let name = entries[i].name_key();
        let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect();
        let stem = compact.rsplit_once('.').map_or(compact.as_str(), |(a, _)| a);
        let score = if stem == phrase {
            0
        } else if compact.starts_with(&phrase) {
            1
        } else if compact.contains(&phrase) {
            2
        } else if terms.iter().all(|t| name.contains(t.as_str())) {
            3
        } else {
            4
        };
        (score, compact.len())
    });
}
pub fn search(entries: &[Entry], query: &Query, kind: Option<Kind>) -> Vec<usize> {
    entries
        .par_iter()
        .enumerate()
        .filter_map(|(i, e)| (kind.is_none_or(|k| k == e.kind) && query.matches(e)).then_some(i))
        .collect()
}

// Nome parecido: compara trincas de letras do nome (sem extensão), ignorando acentos e símbolos.
fn name_grams(name: &str) -> Vec<u64> {
    let norm = normalize(name);
    let stem = norm.rsplit_once('.').map_or(norm.as_str(), |(a, _)| a);
    let clean: Vec<char> = std::iter::once(' ')
        .chain(stem.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }))
        .chain(std::iter::once(' '))
        .collect();
    let mut grams: Vec<u64> = clean
        .windows(3)
        .map(|w| w.iter().fold(0u64, |h, c| h * 0x1f_ffff + *c as u64))
        .collect();
    grams.sort_unstable();
    grams.dedup();
    grams
}
/// Reordena `candidates` por semelhança de nome com `reference` (maior primeiro); descarta abaixo de `min`.
pub fn rank_similar_names(entries: &[Entry], candidates: &[usize], reference: &str, min: f32) -> Vec<usize> {
    let wanted = name_grams(reference);
    let mut scored: Vec<(usize, f32)> = candidates
        .par_iter()
        .filter_map(|&i| {
            let e = &entries[i];
            let name = e.searchable.rsplit(['/', '\\']).next().unwrap_or("");
            let grams = name_grams(name);
            let (mut a, mut b, mut common) = (0, 0, 0);
            while a < wanted.len() && b < grams.len() {
                match wanted[a].cmp(&grams[b]) {
                    std::cmp::Ordering::Less => a += 1,
                    std::cmp::Ordering::Greater => b += 1,
                    std::cmp::Ordering::Equal => {
                        common += 1;
                        a += 1;
                        b += 1;
                    }
                }
            }
            let union = wanted.len() + grams.len() - common;
            let score = if union == 0 { 0. } else { common as f32 / union as f32 };
            (score >= min).then_some((i, score))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().map(|(i, _)| i).collect()
}

// ---- Mudanças ao vivo ----
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Upsert(PathBuf), // criado ou movido para dentro (arquivo ou pasta; pasta nova é varrida)
    Modified(PathBuf), // conteúdo alterado: só vale para arquivo (o horário de uma pasta muda a toda hora)
    Remove(PathBuf), // apagado ou renomeado para fora (arquivo ou pasta)
    Overflow,        // o Windows perdeu eventos: o índice pode ter ficado desatualizado
}
fn collect_files(path: &Path, out: &mut Vec<Entry>) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return; // já sumiu
    };
    if meta.is_dir() {
        if let Ok(read) = fs::read_dir(path) {
            for item in read.flatten() {
                collect_files(&item.path(), out);
            }
        }
    } else if meta.is_file() {
        out.push(Entry::new(path.to_path_buf(), meta.len()).with_modified(meta.modified().map_or(0, unix_secs)).with_created(meta.created().map_or(0, unix_secs)));
    }
}
/// Mudança já lida do disco: pronta para entrar no índice sem tocar no disco de novo.
pub enum Resolved {
    Remove(String),      // chave normalizada de um arquivo ou pasta que sumiu
    Upsert(Vec<Entry>),  // arquivos novos ou alterados (uma pasta nova traz tudo que há dentro)
    Overflow,
}
/// Descobre no disco o que uma mudança significa. Pode demorar (varrer uma pasta nova): chame fora de travas.
pub fn resolve(change: Change) -> Resolved {
    let key = |p: &Path| normalize(&p.to_string_lossy());
    match change {
        Change::Overflow => Resolved::Overflow,
        Change::Remove(p) => Resolved::Remove(key(&p)),
        Change::Upsert(p) => {
            let mut found = Vec::new();
            collect_files(&p, &mut found);
            Resolved::Upsert(found)
        }
        Change::Modified(p) => {
            let mut found = Vec::new();
            if fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) {
                collect_files(&p, &mut found);
            }
            Resolved::Upsert(found)
        }
    }
}
/// O que mudou desde a última aplicação, junto por caminho. Vive numa trava compartilhada: o vigia grava
/// aqui mesmo com a janela escondida na bandeja, e a interface aplica tudo de uma vez quando aparece.
#[derive(Default)]
pub struct Pending {
    present: std::collections::HashMap<String, Entry>,
    removed: Vec<String>,
    pub overflow: bool,
}
pub struct Update {
    pub upserts: Vec<Entry>,
    pub removed: Vec<String>, // cada chave vale para o arquivo e, se for pasta, para tudo dentro dela
}
impl Pending {
    pub fn len(&self) -> usize {
        self.present.len() + self.removed.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn merge(&mut self, change: Resolved) {
        match change {
            Resolved::Overflow => self.overflow = true,
            Resolved::Remove(key) => {
                let inside = format!("{key}\\");
                self.present.retain(|k, _| *k != key && !k.starts_with(&inside));
                self.removed.push(key);
            }
            Resolved::Upsert(found) => {
                for e in found {
                    self.present.insert(e.searchable.to_string(), e);
                }
            }
        }
        // Passou do limite (alguém ficou dias na bandeja num disco muito ativo): descarta e pede nova leitura.
        if self.len() > 300_000 {
            self.present.clear();
            self.removed.clear();
            self.overflow = true;
        }
    }
    pub fn take(&mut self) -> Update {
        Update { upserts: self.present.drain().map(|(_, e)| e).collect(), removed: std::mem::take(&mut self.removed) }
    }
}
/// Aplica as mudanças ao índice (ordenado por `searchable`, como o `scan` o entrega). `true` se algo mudou.
/// Pasta apagada leva todo o conteúdo junto.
pub fn apply_update(entries: &mut Vec<Entry>, update: Update) -> bool {
    let Update { upserts: mut fresh, removed } = update;
    // Trechos do índice a descartar: caminhos apagados, seus filhos, e versões antigas do que foi recriado.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut add_range = |entries: &[Entry], k: &str, with_children: bool| {
        let lo = entries.partition_point(|e| &*e.searchable < k);
        let hi = lo + entries[lo..].partition_point(|e| &*e.searchable == k);
        if hi > lo {
            ranges.push((lo, hi));
        }
        if with_children {
            let inside = format!("{k}\\");
            let lo = entries.partition_point(|e| &*e.searchable < inside.as_str());
            let hi = lo + entries[lo..].partition_point(|e| e.searchable.starts_with(&inside));
            if hi > lo {
                ranges.push((lo, hi));
            }
        }
    };
    for k in &removed {
        add_range(entries, k, true);
    }
    for e in &fresh {
        add_range(entries, &e.searchable, false);
    }
    if ranges.is_empty() && fresh.is_empty() {
        return false;
    }
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (lo, hi) in ranges {
        match merged.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => merged.push((lo, hi)),
        }
    }
    fresh.sort_unstable_by(|a, b| a.searchable.cmp(&b.searchable));
    fresh.dedup_by(|a, b| a.searchable == b.searchable);
    let old = std::mem::take(entries);
    let mut out = Vec::with_capacity(old.len() + fresh.len());
    let (mut fresh, mut skip) = (fresh.into_iter().peekable(), merged.into_iter().peekable());
    for (i, e) in old.into_iter().enumerate() {
        while skip.peek().is_some_and(|r| i >= r.1) {
            skip.next();
        }
        if skip.peek().is_some_and(|r| i >= r.0) {
            continue;
        }
        while fresh.peek().is_some_and(|f| f.searchable <= e.searchable) {
            out.push(fresh.next().unwrap());
        }
        out.push(e);
    }
    out.extend(fresh);
    *entries = out;
    true
}
/// Acha arquivos duplicados (mesmo conteúdo) entre `candidates`. Primeiro agrupa pelo tamanho; depois compara
/// o começo e o fim; só então lê o arquivo inteiro. Devolve grupos de 2 ou mais, do maior tamanho para o menor,
/// cada grupo em ordem de caminho. Nunca altera nada.
pub fn find_duplicates(entries: &[Entry], candidates: &[usize], cancel: &AtomicBool, progress: &AtomicUsize) -> Vec<Vec<usize>> {
    use sha2::{Digest, Sha256};
    use std::{collections::HashMap, io::{Read, Seek, SeekFrom}};
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for &i in candidates {
        let e = &entries[i];
        if e.size_known && e.bytes > 0 {
            by_size.entry(e.bytes).or_default().push(i);
        }
    }
    let groups: Vec<(u64, Vec<usize>)> = by_size.into_iter().filter(|(_, v)| v.len() > 1).collect();
    const EDGE: u64 = 64 * 1024;
    // Impressão digital curta. Arquivo pequeno (até 128 KB) é lido inteiro aqui mesmo e dispensa a 2ª leitura;
    // arquivo grande usa só o começo e o fim. Devolve também se a leitura já cobriu o arquivo todo.
    let edge_hash = |i: usize| -> Option<([u8; 32], bool)> {
        let e = &entries[i];
        let mut f = fs::File::open(e.path()).ok()?;
        let mut hasher = Sha256::new();
        let complete = e.bytes <= 2 * EDGE;
        if complete {
            let mut data = Vec::with_capacity(e.bytes as usize);
            f.take(2 * EDGE + 1).read_to_end(&mut data).ok()?;
            if data.len() as u64 != e.bytes {
                return None; // o arquivo mudou desde a leitura do disco
            }
            hasher.update(&data);
        } else {
            let mut buf = vec![0u8; EDGE as usize];
            let n = f.read(&mut buf).ok()?;
            hasher.update(&buf[..n]);
            f.seek(SeekFrom::End(-(EDGE as i64))).ok()?;
            let n = f.read(&mut buf).ok()?;
            hasher.update(&buf[..n]);
        }
        Some((hasher.finalize().into(), complete))
    };
    let full_hash = |i: usize| -> Option<[u8; 32]> {
        let mut f = fs::File::open(entries[i].path()).ok()?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Some(hasher.finalize().into())
    };
    let mut found: Vec<(u64, Vec<usize>)> = groups
        .par_iter()
        .flat_map(|(size, members)| {
            if cancel.load(Ordering::Relaxed) {
                return Vec::new();
            }
            let mut by_edge: HashMap<[u8; 32], Vec<usize>> = HashMap::new();
            let mut whole_file = false;
            for &i in members {
                if let Some((h, complete)) = edge_hash(i) {
                    whole_file = complete; // todos os membros do grupo têm o mesmo tamanho
                    by_edge.entry(h).or_default().push(i);
                }
            }
            let mut result = Vec::new();
            for (_, same_edge) in by_edge.into_iter().filter(|(_, v)| v.len() > 1) {
                if whole_file {
                    result.push((*size, same_edge));
                    continue;
                }
                let mut by_full: HashMap<[u8; 32], Vec<usize>> = HashMap::new();
                for i in same_edge {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Some(h) = full_hash(i) {
                        by_full.entry(h).or_default().push(i);
                    }
                }
                for (_, same) in by_full.into_iter().filter(|(_, v)| v.len() > 1) {
                    result.push((*size, same));
                }
            }
            progress.fetch_add(members.len(), Ordering::Relaxed);
            result
        })
        .collect();
    for (_, g) in &mut found {
        g.sort_unstable_by(|a, b| entries[*a].searchable.cmp(&entries[*b].searchable));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1[0].cmp(&b.1[0])));
    found.into_iter().map(|(_, g)| g).collect()
}
/// Atalho para testes e usos simples: resolve e aplica um lote de uma vez.
pub fn apply_changes(entries: &mut Vec<Entry>, changes: Vec<Change>) -> bool {
    let mut pending = Pending::default();
    for change in changes {
        pending.merge(resolve(change));
    }
    apply_update(entries, pending.take())
}

// Hard links allow a same-volume move without replacing an existing destination.
// If removing the source fails, remove only the newly created link.
pub fn move_without_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(from)?.file_type().is_symlink() {
        return Err(std::io::Error::other("Links simbólicos não são movidos."));
    }
    fs::hard_link(from, to)?;
    if let Err(err) = fs::remove_file(from) {
        let _ = fs::remove_file(to);
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn duplicates_are_found_by_content_not_by_name_or_size_alone() {
        let dir = std::env::temp_dir().join(format!("seek-dup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a")).unwrap();
        fs::create_dir_all(dir.join("b")).unwrap();
        let big: Vec<u8> = (0..300_000u32).map(|n| (n % 251) as u8).collect();
        let mut other = big.clone();
        let mid = other.len() / 2;
        other[mid] ^= 0xFF; // mesmo tamanho, começo e fim iguais, só o meio muda
        fs::write(dir.join("a/foto.bin"), &big).unwrap();
        fs::write(dir.join("b/copia-com-outro-nome.bin"), &big).unwrap();
        fs::write(dir.join("b/quase.bin"), &other).unwrap();
        fs::write(dir.join("a/pequeno.txt"), b"mesmo texto").unwrap();
        fs::write(dir.join("b/pequeno.txt"), b"mesmo texto").unwrap();
        fs::write(dir.join("a/vazio.txt"), b"").unwrap();
        fs::write(dir.join("b/vazio.txt"), b"").unwrap();
        let entries = scan(&dir, &AtomicBool::new(false), &AtomicUsize::new(0)).entries.to_vec();
        let all: Vec<usize> = (0..entries.len()).collect();
        let groups = find_duplicates(&entries, &all, &AtomicBool::new(false), &AtomicUsize::new(0));
        let names: Vec<Vec<&str>> = groups.iter().map(|g| g.iter().map(|&i| entries[i].name()).collect()).collect();
        assert_eq!(names, vec![vec!["foto.bin", "copia-com-outro-nome.bin"], vec!["pequeno.txt", "pequeno.txt"]]);
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn date_filters_select_by_modified_and_created() {
        let at = |y, m, d| (clock::days_from_civil(y, m, d) * 86_400 + 12 * 3600) as u64;
        let mk = |name: &str, modified: u64, created: u64| Entry::new(PathBuf::from(format!("C:\\x\\{name}")), 1).with_modified(modified).with_created(created);
        let entries = vec![
            mk("velho.txt", at(2022, 3, 10), at(2020, 1, 1)),
            mk("de2023.txt", at(2023, 5, 14), at(2023, 5, 1)),
            mk("recente.txt", clock::now_secs(), clock::now_secs()),
            mk("semdata.txt", 0, 0),
        ];
        let run = |q: &str| search(&entries, &Query::parse(q).unwrap(), None);
        assert_eq!(run("modificado:2023"), vec![1]);
        assert_eq!(run("modificado:2023-05-14"), vec![1]);
        assert_eq!(run("modificado:2022..2023"), vec![0, 1]);
        assert_eq!(run("data:>2023-06-01"), vec![2]);
        assert_eq!(run("criado:<2021"), vec![0]);
        assert_eq!(run("modificado:hoje"), vec![2]);
        assert!(Query::parse("modificado:abc").is_err());
    }
    #[test]
    fn live_new_folder_is_not_duplicated_and_dir_modified_is_ignored() {
        let dir = std::env::temp_dir().join(format!("seek-live2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), b"1").unwrap();
        let mut idx = scan(&dir, &AtomicBool::new(false), &AtomicUsize::new(0)).entries.to_vec();
        fs::create_dir_all(dir.join("nova")).unwrap();
        fs::write(dir.join("nova/b.txt"), b"22").unwrap();
        // o Windows avisa a pasta nova e também cada arquivo dentro dela
        let changes = vec![
            Change::Upsert(dir.join("nova")),
            Change::Upsert(dir.join("nova/b.txt")),
            Change::Modified(dir.clone()),
            Change::Modified(dir.join("nova")),
        ];
        assert!(apply_changes(&mut idx, changes));
        let names: Vec<_> = idx.iter().map(|e| e.name()).collect();
        assert_eq!(names, vec!["a.txt", "b.txt"]);
        // só "pasta modificada": nada muda
        assert!(!apply_changes(&mut idx, vec![Change::Modified(dir.clone())]));
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn live_changes_keep_index_sorted_and_correct() {
        let dir = std::env::temp_dir().join(format!("seek-live-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a/sub")).unwrap();
        fs::write(dir.join("a/um.txt"), b"1").unwrap();
        fs::write(dir.join("a/sub/dois.txt"), b"22").unwrap();
        fs::write(dir.join("b.txt"), b"333").unwrap();
        let cancel = AtomicBool::new(false);
        let mut idx = scan(&dir, &cancel, &AtomicUsize::new(0)).entries.to_vec();
        assert_eq!(idx.len(), 3);
        // pasta apagada leva o conteúdo; arquivo novo entra no lugar certo; arquivo alterado troca o tamanho
        fs::remove_dir_all(dir.join("a")).unwrap();
        fs::write(dir.join("c.txt"), b"4444").unwrap();
        fs::write(dir.join("b.txt"), b"55555").unwrap();
        let changed = apply_changes(
            &mut idx,
            vec![
                Change::Remove(dir.join("a")),
                Change::Upsert(dir.join("c.txt")),
                Change::Upsert(dir.join("b.txt")),
                Change::Remove(dir.join("nunca-existiu.txt")),
            ],
        );
        assert!(changed);
        let names: Vec<_> = idx.iter().map(|e| (e.name(), e.bytes)).collect();
        assert_eq!(names, vec![("b.txt", 5), ("c.txt", 4)]);
        assert!(idx.windows(2).all(|w| w[0].searchable <= w[1].searchable));
        assert!(!apply_changes(&mut idx, vec![Change::Overflow]));
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn similar_names_rank_closest_first() {
        let entries: Vec<Entry> = ["E:/f/IMG_2049.jpg", "E:/f/relatório final.pdf", "E:/f/img_2051.png", "E:/f/Foto Praia 1.jpg"]
            .into_iter()
            .map(|p| Entry::new(PathBuf::from(p), 1))
            .collect();
        let all: Vec<usize> = (0..entries.len()).collect();
        let ranked = rank_similar_names(&entries, &all, "IMG_2048.jpg", 0.4);
        assert_eq!(ranked, vec![0, 2]);
        assert!(rank_similar_names(&entries, &all, "relatorio final.docx", 0.4).contains(&1));
    }
    use super::*;
    fn entry(name: &str, n: u64) -> Entry {
        Entry::new(PathBuf::from(name), n)
    }
    #[test]
    fn semantic_and_accents() {
        assert!(
            Query::parse("foto ferias")
                .unwrap()
                .matches(&entry("Férias/IMG_001.JPG", 10))
        );
        assert!(
            !Query::parse("foto")
                .unwrap()
                .matches(&entry("foto.txt", 10))
        );
        assert!(
            Query::parse("nome:foto")
                .unwrap()
                .matches(&entry("foto.txt", 10))
        );
        assert!(
            Query::parse("VÍDEO")
                .unwrap()
                .matches(&entry("clip.MP4", 10))
        );
    }
    #[test]
    fn operators() {
        let q = Query::parse("foto -ext:png maior:1MB | ext:pdf menor:2MB").unwrap();
        assert!(q.matches(&entry("a.jpg", 2_000_000)));
        assert!(!q.matches(&entry("a.png", 2_000_000)));
        assert!(q.matches(&entry("a.pdf", 200)));
        assert!(!q.matches(&entry("a.pdf", 4_000_000)));
        assert!(
            Query::parse("\"viagem praia\"")
                .unwrap()
                .matches(&entry("viagem praia.jpg", 1))
        );
    }
    #[test]
    fn bare_extensions_are_exact() {
        assert!(Query::parse(".PNG").unwrap().matches(&entry("a.png", 0)));
        assert!(
            !Query::parse(".png")
                .unwrap()
                .matches(&entry("pasta.png/a.jpg", 0))
        );
        assert!(
            Query::parse(".exe")
                .unwrap()
                .matches(&entry("programa.EXE", 0))
        );
        assert!(
            !Query::parse(".exe")
                .unwrap()
                .matches(&entry("programa.exe.txt", 0))
        );
        assert!(
            !Query::parse("nome:foto")
                .unwrap()
                .matches(&entry("foto/a.txt", 0))
        );
    }
    #[test]
    fn persistent_index_roundtrip() {
        let file = std::env::temp_dir().join(format!("velo-cache-{}.bin", std::process::id()));
        let root = Path::new("D:/Férias");
        let entries = vec![
            entry("D:/Férias/imagem.PNG", 45),
            entry("D:/Férias/a.exe", 100),
        ];
        cache::save(&file, root, &entries, 3).unwrap();
        let loaded =
            cache::load(&file, root, &AtomicBool::new(false), &AtomicUsize::new(0)).unwrap();
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries[0].path, entries[0].path);
        assert_eq!(loaded.errors, 3);
        assert_eq!(
            search(&loaded.entries, &Query::parse("foto").unwrap(), None),
            vec![0]
        );
        assert!(
            cache::load(
                &file,
                Path::new("other"),
                &AtomicBool::new(false),
                &AtomicUsize::new(0)
            )
            .is_err()
        );
        fs::write(&file, "corrupt").unwrap();
        assert!(cache::load(&file, root, &AtomicBool::new(false), &AtomicUsize::new(0)).is_err());
        fs::remove_file(file).unwrap();
    }
    #[test]
    fn scan_nested_folders_and_cancel() {
        let root = std::env::temp_dir().join(format!("velo-scan-{}", std::process::id()));
        fs::create_dir_all(root.join("perdida/subpasta")).unwrap();
        fs::create_dir_all(root.join(".oculta")).unwrap();
        fs::write(root.join("perdida/subpasta/IMG_008.JPG"), "image").unwrap();
        fs::write(root.join(".oculta/a.exe"), "program").unwrap();
        let result = scan(&root, &AtomicBool::new(false), &AtomicUsize::new(0));
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.errors, 0);
        assert_eq!(
            search(&result.entries, &Query::parse("foto").unwrap(), None).len(),
            1
        );
        assert_eq!(
            search(&result.entries, &Query::parse(".exe").unwrap(), None).len(),
            1
        );
        let result = scan(&root, &AtomicBool::new(true), &AtomicUsize::new(0));
        assert!(result.cancelled);
        assert!(result.entries.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_queries() {
        for q in [
            "foto |",
            "| foto",
            "\"abc",
            "ext:",
            "tipo:nada",
            "maior:xx",
            "maior:2tb",
            "-",
        ] {
            assert!(Query::parse(q).is_err(), "{q}");
        }
    }
    #[test]
    fn moves_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("velo-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, "original").unwrap();
        fs::write(&b, "existente").unwrap();
        assert!(move_without_replace(&a, &b).is_err());
        assert_eq!(fs::read_to_string(&b).unwrap(), "existente");
        assert!(a.exists());
        fs::remove_file(&b).unwrap();
        move_without_replace(&a, &b).unwrap();
        assert!(!a.exists());
        assert_eq!(fs::read_to_string(&b).unwrap(), "original");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn benchmark_million() {
        let entries: Vec<_> = (0..1_000_000)
            .map(|i| {
                entry(
                    &format!("Férias/IMG_{i}.{}", if i % 2 == 0 { "jpg" } else { "txt" }),
                    2_000_000,
                )
            })
            .collect();
        let q = Query::parse("foto ferias maior:1MB").unwrap();
        let _ = search(&entries, &q, None);
        let start = Instant::now();
        for _ in 0..20 {
            assert_eq!(search(&entries, &q, None).len(), 500_000);
        }
        println!(
            "1.000.000 arquivos simulados, média de 20 buscas: {:.2} ms",
            start.elapsed().as_secs_f64() * 1000. / 20.
        );
    }
    #[test]
    fn plain_text_returns_every_match_without_cap() {
        let mut entries: Vec<_> = (0..25_000)
            .map(|i| entry(&format!("pasta/perdida/AbC_{i}.qualquercoisa"), i))
            .collect();
        entries.push(entry("outra/coisa.txt", 0));
        let results = search(&entries, &Query::parse("abc").unwrap(), None);
        assert_eq!(results.len(), 25_000);
        assert!(results.iter().all(|&i| i < 25_000));
        let mut unknown = entry("abc.txt", 0);
        unknown.size_known = false;
        assert!(Query::parse("abc").unwrap().matches(&unknown));
        assert!(!Query::parse("menor:1KB").unwrap().matches(&unknown));
    }
}
