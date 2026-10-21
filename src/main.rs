#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod hotkey;
mod startup;
mod thumbs;
mod tray;
mod ui;
use eframe::egui::{self, Color32, RichText};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use velo::{Entry, Kind, Query};

enum PreviewData {
    Image(egui::ColorImage),
    Text(String),
    Nothing,
}
const TEXT_EXTS: [&str; 24] = [
    "txt", "md", "log", "json", "csv", "ini", "xml", "html", "htm", "css", "js", "py", "rs", "toml", "yaml", "yml", "cfg", "conf", "bat", "cmd",
    "ps1", "sql", "srt", "c",
];
/// Resultado de "Achar duplicados": cada grupo tem 2 ou mais arquivos com o mesmo conteúdo.
struct DupState {
    groups: usize,
    copies: Vec<usize>, // tudo menos o primeiro de cada grupo
    wasted: u64,        // bytes que as cópias ocupam
    first: HashSet<usize>,
    group_of: HashMap<usize, usize>,
}
struct SearchJob {
    generation: u64,
    entries: Arc<Vec<Entry>>,
    query: Query,
    kind: Option<Kind>,
    icons: Option<Arc<Vec<Option<velo::icon_search::Signature>>>>,
    reference: Option<velo::icon_search::Signature>,
    tolerance: f32,
    name_like: Option<String>,
    sort: Option<(velo::SortKey, bool)>,
}
struct SearchResult {
    generation: u64,
    indices: Vec<usize>,
    elapsed: Duration,
}
struct MoveResult {
    undo: Vec<(PathBuf, PathBuf)>,
    message: String,
    root: PathBuf,
}
struct App {
    root: Option<PathBuf>,
    entries: Arc<Vec<Entry>>,
    query: String,
    kind: Option<Kind>,
    results: Vec<usize>,
    selected: HashSet<usize>,
    scan_rx: Option<mpsc::Receiver<velo::Scan>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
    jobs: mpsc::Sender<SearchJob>,
    answers: mpsc::Receiver<SearchResult>,
    generation: u64,
    searching: bool,
    status: String,
    error: String,
    elapsed: Duration,
    preview: bool,
    undo: Vec<(PathBuf, PathBuf)>,
    help: bool,
    move_rx: Option<mpsc::Receiver<MoveResult>>,
    notice: String,
    icon_rx: Option<mpsc::Receiver<velo::icon_search::Catalog>>,
    icon_cancel: Arc<AtomicBool>,
    icon_progress: Arc<AtomicUsize>,
    icon_catalog: Option<Arc<Vec<Option<velo::icon_search::Signature>>>>,
    icon_catalog_complete: bool,
    icon_reference: Option<velo::icon_search::Signature>,
    icon_texture: Option<egui::TextureHandle>,
    icon_name: String,
    icon_tolerance: f32,
    icon_status: String,
    similar: Option<String>,
    visual_photo: bool,
    view: ui::View,
    prev_view: ui::View,
    update_rx: Option<mpsc::Receiver<Result<Option<String>, String>>>,
    update_notice: String,
    dup: Option<DupState>,
    dup_rx: Option<mpsc::Receiver<Vec<Vec<usize>>>>,
    dup_cancel: Arc<AtomicBool>,
    dup_progress: Arc<AtomicUsize>,
    viewer: Option<usize>, // índice do arquivo em pré-visualização
    viewer_tex: Option<egui::TextureHandle>,
    viewer_text: Option<String>,
    viewer_rx: Option<mpsc::Receiver<PreviewData>>,
    autostart: bool,
    hide_until: Option<Instant>, // início com --tray: a janela é escondida até este momento
    sort: Option<(velo::SortKey, bool)>,
    cursor: usize,
    last_cols: usize,
    scroll_y: f32,
    view_h: f32,
    scroll_to_cursor: bool,
    keep: Option<(std::collections::HashSet<PathBuf>, Option<PathBuf>)>, // seleção e cursor a restaurar após uma busca ao vivo
    hl_terms: Vec<String>,
    hotkeys: Option<hotkey::Hotkeys>,
    settings: bool,
    capturing: bool,
    hotkey_error: String,
    quick: bool,
    quick_prev: Option<(ui::View, Option<egui::Vec2>)>,
    quick_focus_seen: bool,
    quick_lost_focus: Option<Instant>, // desde quando a janelinha está sem foco
    focus_search: bool,
    tray_ready: bool,
    admin_hint: bool,
    watchers: Vec<velo::watch::Watcher>,
    live: Arc<Mutex<velo::Pending>>, // mudanças do disco, juntadas pelo vigia (também com a janela escondida)
    live_overflow: bool,
    last_input: f64,
    live_dirty_at: Option<f64>,
    disks: bool,
    drive_list: Vec<(PathBuf, u64, u64)>,
    thumbs: thumbs::Thumbs,
    kind_counts: [usize; Kind::ALL.len()],
    ext_counts: Vec<(String, Kind, usize)>,
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::theme(&cc.egui_ctx);
        let (jobs, input) = mpsc::channel::<SearchJob>();
        let (output, answers) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        thread::spawn(move || {
            while let Ok(mut job) = input.recv() {
                while let Ok(newer) = input.try_recv() {
                    job = newer;
                }
                let start = Instant::now();
                let mut indices = velo::search(&job.entries, &job.query, job.kind);
                let visual = job.reference.is_some();
                if let Some(reference) = job.reference {
                    if let Some(icons) = job.icons {
                        // Mais parecidos primeiro.
                        let mut scored: Vec<(usize, f32)> = indices
                            .iter()
                            .filter_map(|&i| {
                                let d = reference.distance((*icons.get(i)?)?);
                                (d <= job.tolerance).then_some((i, d))
                            })
                            .collect();
                        scored.sort_by(|a, b| a.1.total_cmp(&b.1));
                        indices = scored.into_iter().map(|(i, _)| i).collect();
                    } else {
                        indices.clear();
                    }
                }
                if let Some(name) = &job.name_like {
                    indices = velo::rank_similar_names(&job.entries, &indices, name, 0.4);
                }
                if let Some((key, desc)) = job.sort {
                    velo::sort_results(&job.entries, &mut indices, key, desc);
                } else if job.name_like.is_none() && !visual {
                    velo::rank_relevance(&job.entries, &mut indices, &job.query);
                }
                if output
                    .send(SearchResult {
                        generation: job.generation,
                        indices,
                        elapsed: start.elapsed(),
                    })
                    .is_err()
                {
                    break;
                }
                ctx.request_repaint();
            }
        });
        let mut app = Self {
            root: None,
            entries: Arc::new(Vec::new()),
            query: String::new(),
            kind: None,
            results: Vec::new(),
            selected: HashSet::new(),
            scan_rx: None,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(AtomicUsize::new(0)),
            jobs,
            answers,
            generation: 0,
            searching: false,
            status: "Escolha uma pasta para começar.".into(),
            error: String::new(),
            elapsed: Duration::ZERO,
            preview: false,
            undo: Vec::new(),
            help: false,
            move_rx: None,
            notice: String::new(),
            icon_rx: None,
            icon_cancel: Arc::new(AtomicBool::new(false)),
            icon_progress: Arc::new(AtomicUsize::new(0)),
            icon_catalog: None,
            icon_catalog_complete: false,
            icon_reference: None,
            icon_texture: None,
            icon_name: String::new(),
            icon_tolerance: 0.12,
            icon_status: String::new(),
            similar: None,
            visual_photo: false,
            view: if ui::saved_compact() { ui::View::Compact } else { ui::View::List },
            prev_view: ui::View::List,
            update_rx: None,
            update_notice: String::new(),
            dup: None,
            dup_rx: None,
            dup_cancel: Arc::new(AtomicBool::new(false)),
            dup_progress: Arc::new(AtomicUsize::new(0)),
            viewer: None,
            viewer_tex: None,
            viewer_text: None,
            viewer_rx: None,
            autostart: startup::is_enabled(),
            hide_until: std::env::args_os().any(|a| a == "--tray").then(|| Instant::now() + Duration::from_secs(3)),
            sort: None,
            cursor: 0,
            last_cols: 1,
            scroll_y: 0.,
            view_h: 0.,
            scroll_to_cursor: false,
            keep: None,
            hl_terms: Vec::new(),
            hotkeys: hotkey::Hotkeys::install(&cc.egui_ctx),
            settings: false,
            capturing: false,
            hotkey_error: String::new(),
            quick: false,
            quick_prev: None,
            quick_focus_seen: false,
            quick_lost_focus: None,
            focus_search: false,
            tray_ready: tray::install(&cc.egui_ctx),
            admin_hint: false,
            watchers: Vec::new(),
            live: Arc::default(),
            live_overflow: false,
            last_input: 0.,
            live_dirty_at: None,
            disks: false,
            drive_list: Vec::new(),
            thumbs: thumbs::Thumbs::new(&cc.egui_ctx),
            kind_counts: [0; Kind::ALL.len()],
            ext_counts: Vec::new(),
        };
        // Atualização automática: confere o GitHub em segundo plano; falhas (sem internet) são silenciosas.
        if let Ok(exe) = std::env::current_exe() {
            velo::update::cleanup(&exe);
            let (tx, rx) = mpsc::channel();
            app.update_rx = Some(rx);
            let ctx = cc.egui_ctx.clone();
            thread::spawn(move || {
                loop {
                    let result = velo::update::check_and_install(&exe);
                    match &result {
                        Ok(Some(v)) => velo::log::line(&format!("atualizado para a versão {v}")),
                        Err(e) => velo::log::line(&format!("checagem de atualização falhou: {e}")),
                        Ok(None) => {}
                    }
                    let installed = matches!(result, Ok(Some(_)));
                    let _ = tx.send(result);
                    ctx.request_repaint();
                    if installed {
                        break;
                    }
                    thread::sleep(Duration::from_secs(6 * 3600));
                }
            });
        }
        if let Some(root) = std::env::args_os()
            .skip(1)
            .find(|a| !a.to_string_lossy().starts_with("--")) // opções como --tray e --relaunch não são pastas
            .map(PathBuf::from)
            .or_else(velo::cache::previous_root)
        {
            app.start_scan(&cc.egui_ctx, root, true);
        }
        app
    }
    fn refresh_search(&mut self) {
        self.generation += 1;
        self.dup = None;
        self.dup_cancel.store(true, Ordering::Relaxed);
        self.dup_rx = None;
        self.selected.clear();
        self.results.clear();
        self.cursor = 0;
        self.scroll_to_cursor = true;
        self.send_search();
    }
    // Mudança ao vivo no disco: refaz a busca sem mexer na rolagem, no cursor nem na seleção (guardados por caminho).
    fn refresh_search_live(&mut self, selected: std::collections::HashSet<PathBuf>, cursor: Option<PathBuf>) {
        if self.dup.is_some() {
            self.refresh_search();
            return;
        }
        self.generation += 1;
        self.keep = Some((selected, cursor));
        self.send_search();
    }
    fn send_search(&mut self) {
        match Query::parse(&self.query) {
            Ok(query) => {
                self.hl_terms = query.terms();
                self.error.clear();
                self.searching = true;
                let _ = self.jobs.send(SearchJob {
                    generation: self.generation,
                    entries: self.entries.clone(),
                    query,
                    kind: self.kind,
                    icons: self.icon_catalog.clone(),
                    reference: self.icon_reference,
                    tolerance: self.icon_tolerance,
                    name_like: self.similar.clone(),
                    sort: self.sort,
                });
            }
            Err(error) => {
                self.error = error;
                self.searching = false;
            }
        }
    }
    fn start_scan(&mut self, ctx: &egui::Context, root: PathBuf, use_cache: bool) {
        if !use_cache && !velo::is_all(&root) && !root.is_dir() {
            self.error = "A pasta não está acessível. Verifique se o HD está conectado.".into();
            return;
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.icon_cancel.store(true, Ordering::Relaxed);
        self.icon_rx = None;
        self.icon_catalog = None;
        self.icon_catalog_complete = false;
        self.icon_reference = None;
        self.icon_texture = None;
        self.icon_name.clear();
        self.icon_status.clear();
        self.cancel = Arc::new(AtomicBool::new(false));
        self.progress = Arc::new(AtomicUsize::new(0));
        // Reler a mesma pasta/disco (Atualizar): o índice antigo continua pesquisável durante a leitura.
        let same_root = self.root.as_ref() == Some(&root) && !self.entries.is_empty();
        self.root = Some(root.clone());
        if !same_root {
            self.entries = Arc::new(Vec::new());
            self.results.clear();
            self.kind_counts = [0; Kind::ALL.len()];
            self.ext_counts.clear();
        }
        self.selected.clear();
        self.thumbs = thumbs::Thumbs::new(ctx);
        self.watchers.clear();
        *self.live.lock().unwrap() = Default::default();
        self.live_overflow = false;
        self.live_dirty_at = None;
        self.generation += 1;
        self.searching = false;
        let (tx, rx) = mpsc::channel();
        self.scan_rx = Some(rx);
        let cancel = self.cancel.clone();
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        self.status = if use_cache {
            "Carregando índice salvo ou fazendo a primeira leitura…"
        } else {
            "Lendo todas as subpastas em segundo plano…"
        }
        .into();
        thread::spawn(move || {
            let file = velo::cache::location(&root);
            if use_cache
                && let Some(file) = &file
                    && let Ok(scan) = velo::cache::load(file, &root, &cancel, &progress) {
                        let _ = tx.send(scan);
                        ctx.request_repaint();
                        return;
                    }
            progress.store(0, Ordering::Relaxed);
            let mut scan = velo::scan(&root, &cancel, &progress);
            if !scan.cancelled && (root.is_dir() || velo::is_all(&root))
                && let Some(file) = file {
                    scan.cache_saved =
                        velo::cache::save(&file, &root, &scan.entries, scan.errors).is_ok();
                }
            let _ = tx.send(scan);
            ctx.request_repaint();
        });
        velo::cache::remember_root(self.root.as_ref().unwrap());
    }
    // Duplicados entre os resultados atuais: lê os arquivos em segundo plano (só leitura).
    fn start_duplicates(&mut self, ctx: &egui::Context) {
        let candidates = self.results.clone();
        let entries = self.entries.clone();
        let (tx, rx) = mpsc::channel();
        self.dup_rx = Some(rx);
        self.dup_cancel = Arc::new(AtomicBool::new(false));
        self.dup_progress = Arc::new(AtomicUsize::new(0));
        let (cancel, progress, ctx) = (self.dup_cancel.clone(), self.dup_progress.clone(), ctx.clone());
        thread::spawn(move || {
            let groups = velo::find_duplicates(&entries, &candidates, &cancel, &progress);
            let _ = tx.send(groups);
            ctx.request_repaint();
        });
    }
    fn apply_duplicates(&mut self, groups: Vec<Vec<usize>>) {
        if groups.is_empty() {
            self.notice = "Nenhum arquivo duplicado nesta lista.".into();
            return;
        }
        let (mut first, mut group_of, mut copies, mut wasted, mut results) = (HashSet::new(), HashMap::new(), Vec::new(), 0u64, Vec::new());
        for (g, members) in groups.iter().enumerate() {
            first.insert(members[0]);
            for (k, &i) in members.iter().enumerate() {
                group_of.insert(i, g);
                results.push(i);
                if k > 0 {
                    copies.push(i);
                    wasted += self.entries[i].bytes;
                }
            }
        }
        self.results = results;
        self.selected.clear();
        self.cursor = 0;
        self.scroll_to_cursor = true;
        self.searching = false;
        self.notice.clear();
        self.dup = Some(DupState { groups: groups.len(), copies, wasted, first, group_of });
    }
    fn organize(&mut self, ctx: &egui::Context) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let selected: Vec<_> = self.selected.iter().copied().collect();
        let entries = self.entries.clone();
        let (tx, rx) = mpsc::channel();
        self.move_rx = Some(rx);
        self.preview = false;
        self.notice = "Organizando em segundo plano…".into();
        let ctx = ctx.clone();
        let previous_undo = self.undo.clone();
        let group_of = self.dup.as_ref().map(|d| d.group_of.clone());
        thread::spawn(move || {
            let mut moved = Vec::new();
            let mut failed = 0;
            let mut skipped = 0;
            for i in selected {
                let e = &entries[i];
                let base = if velo::is_all(&root) {
                    // "C:\\..." -> "C:\\": cada arquivo é organizado no seu próprio disco
                    PathBuf::from(e.path_str().chars().take(3).collect::<String>())
                } else {
                    root.clone()
                };
                // duplicados: uma pasta por grupo (os nomes costumam ser iguais); senão, uma pasta por tipo
                let dir = match &group_of {
                    Some(g) => base.join("Seek - Duplicados").join(format!("grupo-{:04}", g.get(&i).copied().unwrap_or(0) + 1)),
                    None => base.join("Seek - Organizados").join(e.kind.label()),
                };
                let to = dir.join(e.name());
                if e.path() == to {
                    skipped += 1;
                    continue;
                }
                let result = std::fs::create_dir_all(&dir)
                    .and_then(|_| velo::move_without_replace(e.path(), &to));
                match result {
                    Ok(()) => moved.push((e.path().to_path_buf(), to)),
                    Err(_) => failed += 1,
                }
            }
            let n = moved.len();
            let message = format!(
                "{n} movidos · {skipped} já organizados · {failed} não movidos (nome repetido, permissão ou volume incompatível)."
            );
            let _ = tx.send(MoveResult {
                undo: if moved.is_empty() {
                    previous_undo
                } else {
                    moved
                },
                message,
                root,
            });
            ctx.request_repaint();
        });
    }
    fn choose_icon(&mut self, ctx: &egui::Context, path: PathBuf) {
        match velo::icon_search::load_reference(&path) {
            Ok((image, signature)) => {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.set_visual(ctx, image, signature, name, false);
            }
            Err(error) => self.error = format!("Não foi possível ler o ícone: {error}"),
        }
    }
    // Referência visual (ícone ou foto) e catálogo de assinaturas do disco; o catálogo é feito uma vez.
    fn set_visual(
        &mut self,
        ctx: &egui::Context,
        image: image::RgbaImage,
        signature: velo::icon_search::Signature,
        name: String,
        photo: bool,
    ) {
        let size = [image.width() as usize, image.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        self.icon_texture = Some(ctx.load_texture("icone-referencia", color, egui::TextureOptions::LINEAR));
        if photo != self.visual_photo {
            self.icon_cancel.store(true, Ordering::Relaxed);
            self.icon_rx = None;
            self.icon_catalog = None;
            self.icon_catalog_complete = false;
            self.visual_photo = photo;
            self.icon_tolerance = if photo { 0.25 } else { 0.12 };
        }
        self.icon_name = name;
        self.icon_reference = Some(signature);
        self.similar = None;
        self.error.clear();
        if !self.icon_catalog_complete && self.icon_rx.is_none() {
            let Some(root) = self.root.clone() else {
                return;
            };
            let entries = self.entries.clone();
            let (tx, rx) = mpsc::channel();
            self.icon_rx = Some(rx);
            self.icon_cancel = Arc::new(AtomicBool::new(false));
            self.icon_progress = Arc::new(AtomicUsize::new(0));
            let cancel = self.icon_cancel.clone();
            let progress = self.icon_progress.clone();
            let ctx = ctx.clone();
            self.icon_status = if photo {
                "Analisando as fotos pela primeira vez; depois fica salvo…"
            } else {
                "Preparando o catálogo de ícones pela primeira vez…"
            }
            .into();
            thread::spawn(move || {
                let catalog = if photo {
                    velo::icon_search::build_photos(&entries, &root, &cancel, &progress)
                } else {
                    velo::icon_search::build(&entries, &root, &cancel, &progress)
                };
                let _ = tx.send(catalog);
                ctx.request_repaint();
            });
        }
        self.refresh_search();
    }
    // 0 = nome parecido, 1 = foto parecida, 2 = ícone parecido
    fn start_similar(&mut self, ctx: &egui::Context, index: usize, mode: u8) {
        let Some(e) = self.entries.get(index) else {
            return;
        };
        let (path, name) = (e.path().to_path_buf(), e.name().to_string());
        match mode {
            0 => {
                self.clear_visual();
                self.similar = Some(name);
                self.refresh_search();
            }
            1 => match velo::icon_search::photo_reference(&path) {
                Ok((image, signature)) => self.set_visual(ctx, image, signature, name, true),
                Err(error) => self.error = error,
            },
            _ => match velo::icon_search::Extractor::new().image(&path) {
                Some(image) => {
                    let signature = velo::icon_search::Signature::from_image(&image);
                    self.set_visual(ctx, image, signature, name, false);
                }
                None => self.error = "Não foi possível ler o ícone deste arquivo.".into(),
            },
        }
    }
    // Pré-visualização: imagem grande (miniatura do Windows ou decodificada) ou o começo de um arquivo de texto.
    fn open_viewer(&mut self, ctx: &egui::Context, index: usize) {
        let Some(e) = self.entries.get(index) else { return };
        let (path, ext) = (e.path().to_path_buf(), e.ext().to_owned());
        self.viewer = Some(index);
        self.viewer_tex = None;
        self.viewer_text = None;
        let (tx, rx) = mpsc::channel();
        self.viewer_rx = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let data = if TEXT_EXTS.contains(&ext.as_str()) {
                use std::io::Read;
                let mut buf = vec![0u8; 6000];
                let n = std::fs::File::open(&path).and_then(|mut f| f.read(&mut buf)).unwrap_or(0);
                buf.truncate(n);
                PreviewData::Text(String::from_utf8_lossy(&buf).into_owned())
            } else {
                let image = velo::shellthumb::thumbnail(&path, 1024, true).or_else(|| {
                    // sem miniatura do Windows: tenta decodificar a própria imagem
                    let img = image::ImageReader::open(&path).ok()?.with_guessed_format().ok()?.decode().ok()?;
                    Some(img.thumbnail(1600, 1600).to_rgba8())
                });
                match image {
                    Some(i) => PreviewData::Image(egui::ColorImage::from_rgba_unmultiplied(
                        [i.width() as usize, i.height() as usize],
                        i.as_raw(),
                    )),
                    None => PreviewData::Nothing,
                }
            };
            let _ = tx.send(data);
            ctx.request_repaint();
        });
    }
    fn close_viewer(&mut self) {
        self.focus_search = true; // depois da prévia, o que se digita volta para a busca
        self.viewer = None;
        self.viewer_tex = None;
        self.viewer_text = None;
        self.viewer_rx = None;
    }
    fn clear_visual(&mut self) {
        self.icon_cancel.store(true, Ordering::Relaxed);
        self.icon_rx = None;
        self.icon_reference = None;
        self.icon_texture = None;
        self.icon_name.clear();
        self.icon_status.clear();
    }
    fn clear_icon(&mut self) {
        self.clear_visual();
        self.similar = None;
        self.refresh_search();
    }
    fn undo_moves(&mut self, ctx: &egui::Context) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let undo = std::mem::take(&mut self.undo);
        let (tx, rx) = mpsc::channel();
        self.move_rx = Some(rx);
        self.notice = "Desfazendo em segundo plano…".into();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let mut remaining = Vec::new();
            let mut count = 0;
            for (from, to) in undo {
                match velo::move_without_replace(&to, &from) {
                    Ok(()) => count += 1,
                    Err(_) => remaining.push((from, to)),
                }
            }
            let message = format!(
                "{count} movimentos desfeitos. {} pendentes.",
                remaining.len()
            );
            let _ = tx.send(MoveResult {
                undo: remaining,
                message,
                root,
            });
            ctx.request_repaint();
        });
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.icon_cancel.store(true, Ordering::Relaxed);
    }
}
// Uma única cópia do Seek por vez: abrir de novo só traz a janela que já está rodando na bandeja.
// `--relaunch` (usado ao reabrir como administrador ou após atualizar) espera a cópia antiga sair.
fn primary_instance(relaunch: bool) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
        System::Threading::CreateMutexW,
    };
    let name: Vec<u16> = "Local\\SeekSingleInstance\0".encode_utf16().collect();
    for _ in 0..if relaunch { 75 } else { 1 } {
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        // Sem handle = acesso negado: o mutex é de uma cópia que roda como administrador, ou seja, já existe uma.
        let exists = handle.is_null() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if !exists {
            return true; // o handle fica aberto até o programa terminar
        }
        if !handle.is_null() {
            unsafe { CloseHandle(handle) };
        }
        thread::sleep(Duration::from_millis(200));
    }
    relaunch
}
// Se algo der errado, grava o motivo em seek-log.txt e (na janela principal) avisa em vez de sumir calado.
fn install_panic_log() {
    std::panic::set_hook(Box::new(|info| {
        let thread = thread::current();
        let name = thread.name().unwrap_or("?").to_owned();
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let text = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        velo::log::line(&format!("ERRO na thread {name} em {place}: {text}"));
        if name == "main" {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Seek")
                .set_description("O Seek encontrou um erro e vai fechar. Os detalhes ficaram em seek-log.txt, ao lado do programa.")
                .show();
        }
    }));
}
fn main() -> eframe::Result {
    // `Seek.exe --quit`: pede à cópia que está rodando que se encerre direito (tira o ícone da bandeja).
    if std::env::args_os().any(|a| a == "--quit") {
        tray::signal_quit();
        return Ok(());
    }
    install_panic_log();
    velo::log::line(&format!("Seek {} iniciado", velo::update::VERSION));
    let relaunch = std::env::args_os().any(|a| a == "--relaunch");
    if !primary_instance(relaunch) {
        tray::show_window();
        return Ok(());
    }
    let (rgba, width, height) = tray::icon_rgba();
    // `--tray` (início com o Windows): sobe escondido; o ícone da bandeja ou o atalho abrem a janela.
    let hidden = std::env::args_os().any(|a| a == "--tray");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_visible(!hidden)
            .with_icon(egui::IconData { rgba, width, height })
            .with_inner_size([1120., 740.])
            .with_min_inner_size([850., 520.]),
        ..Default::default()
    };
    let result = eframe::run_native(
        tray::TITLE,
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    );
    tray::remove();
    velo::log::line("Seek encerrado");
    result
}
