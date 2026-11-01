//! Aba "Espaço": o que ocupa cada disco, o que dá para liberar, o histórico e os jogos.
//! Nada é apagado ou movido sem a pessoa ver item por item e confirmar; tudo vai para o registro, com Desfazer.
use super::*;
use crate::ui::{bytes, c_bg, c_border, c_dim, c_hover, c_line, c_mute, c_panel, c_sel, c_text, c_tile, clipped, flat, fmt_date, list_drives, open_file, primary, reveal};
use egui::{Align, Align2, FontId, Rect, Sense, Stroke, StrokeKind, vec2};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    sync::{Weak, atomic::AtomicU64},
};
use velo::{
    cleanup,
    games::Game,
    history::{self, DaySum, Delta},
    knowledge::{self, Act, Seal},
    space::{self, Child, Item, Sizes},
};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Home,
    Browse,
    History,
    Games,
    Actions,
}
struct Listing {
    folder: PathBuf,
    children: Vec<Child>,
    added: Vec<u64>, // por filho: quando o arquivo mais novo entrou (criação)
    kinds: Vec<(Kind, u64, usize)>,
    total: u64,
    files: usize,
}
struct Review {
    title: String,
    items: Vec<(String, Item, Option<String>)>, // (grupo, item, motivo do bloqueio)
    checked: HashSet<usize>,
    confirm: bool,
}
struct MoveAsk {
    src: PathBuf,
    name: String,
    bytes: u64,
    exe: String, // executável do jogo, para saber se está aberto
    target: Option<PathBuf>,
}
enum JobDone {
    Recycled(Vec<(PathBuf, u64, Result<(), String>)>),
    Moved(PathBuf, u64, Result<PathBuf, String>),
    Undone(usize, Result<(), String>),
}
struct Job {
    label: String,
    done: Arc<AtomicU64>,
    total: u64,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<JobDone>,
}
struct Suggest {
    junk: Vec<Item>,
    installers: Vec<Item>,
    forgotten: Vec<Item>,
}
#[derive(Default)]
struct Hist {
    summary: Option<(Instant, BTreeMap<String, DaySum>)>,
    created: Option<(Weak<Vec<Entry>>, BTreeMap<i64, u64>)>,
    disks: Option<BTreeMap<String, Vec<(u64, u64)>>>,
    from: String,
    to: String,
    rx: Option<mpsc::Receiver<(String, Vec<Delta>)>>,
    view: Option<(String, Arc<Vec<Delta>>)>,
    folder: Option<PathBuf>,
    grouped: Option<(PathBuf, Vec<(String, i64, usize)>)>,
    files: bool,
    size: u64, // espaço do próprio histórico
}
pub struct SpaceState {
    pub on: bool,
    page: Page,
    folder: Option<PathBuf>,
    sizes: Option<(Weak<Vec<Entry>>, Arc<Sizes>)>,
    listing: Option<(Weak<Vec<Entry>>, Arc<Listing>)>,
    drives: Vec<(PathBuf, u64, u64)>,
    drives_at: Option<Instant>,
    growth: Option<(Weak<Vec<Entry>>, Vec<(bool, u64, u64)>)>, // por disco: no índice, entrou em 7 e em 30 dias
    hover: Option<usize>,
    suggest: Option<(Instant, Arc<Suggest>)>, // refeitas no máximo a cada 2 minutos (o disco muda o tempo todo)
    suggest_rx: Option<mpsc::Receiver<Suggest>>,
    dup_rx: Option<mpsc::Receiver<Vec<Item>>>,
    dup_cancel: Arc<AtomicBool>,
    dup_progress: Arc<AtomicUsize>,
    dups: Option<Vec<Item>>,
    bin: Option<(Instant, (u64, u64))>,
    review: Option<Review>,
    moving: Option<MoveAsk>,
    job: Option<Job>,
    actions: cleanup::Actions,
    log: Option<history::Log>,
    hist: Hist,
    games: Option<Vec<Game>>,
    games_rx: Option<mpsc::Receiver<Vec<Game>>>,
    notice: String,
    alerted: HashSet<PathBuf>,
    last_sample: Option<Instant>,
    last_notify: Option<Instant>,
}
impl SpaceState {
    pub fn new() -> Self {
        let log = history::Log::open();
        if let Some(l) = &log {
            // Dias anteriores viram .gz em segundo plano.
            let dir = l.dir.clone();
            thread::spawn(move || {
                if let Err(e) = (history::Log { dir }).compact(&history::day_name(velo::clock::now_secs())) {
                    velo::log::line(&format!("histórico: falha ao compactar dias antigos: {e}"));
                }
            });
        }
        let today = velo::clock::now_secs();
        Self {
            on: false,
            page: Page::Home,
            folder: None,
            sizes: None,
            listing: None,
            drives: Vec::new(),
            drives_at: None,
            growth: None,
            hover: None,
            suggest: None,
            suggest_rx: None,
            dup_rx: None,
            dup_cancel: Arc::new(AtomicBool::new(false)),
            dup_progress: Arc::new(AtomicUsize::new(0)),
            dups: None,
            bin: None,
            review: None,
            moving: None,
            job: None,
            actions: cleanup::Actions::open(),
            log,
            hist: Hist { from: history::day_name(today - 7 * 86_400), to: history::day_name(today), ..Default::default() },
            games: None,
            games_rx: None,
            notice: String::new(),
            alerted: HashSet::new(),
            last_sample: None,
            last_notify: None,
        }
    }
    /// Mudanças ao vivo: vão para o histórico (em segundo plano) e, se algo grande entrou, avisam na bandeja.
    pub fn record(&mut self, deltas: Vec<Delta>) {
        if deltas.is_empty() {
            return;
        }
        let big: Vec<&Delta> = deltas.iter().filter(|d| d.change() >= 1 << 30).collect();
        // no máximo um aviso a cada 10 minutos (mover um jogo, por exemplo, traz vários arquivos grandes seguidos)
        let quiet = self.last_notify.is_some_and(|t| t.elapsed() < Duration::from_secs(600));
        if !quiet && let Some(d) = big.iter().max_by_key(|d| d.change()) {
            self.last_notify = Some(Instant::now());
            let name = Path::new(&*d.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let more = if big.len() > 1 { format!(" (e mais {})", big.len() - 1) } else { String::new() };
            crate::tray::notify("Arquivo grande no disco", &format!("{name}: +{}{more}\n{}", bytes(d.change() as u64), d.path));
        }
        self.hist.summary = None;
        if let Some(dir) = self.log.as_ref().map(|l| l.dir.clone()) {
            thread::spawn(move || {
                if let Err(e) = (history::Log { dir }).record(&deltas) {
                    velo::log::line(&format!("histórico: falha ao gravar: {e}"));
                }
            });
        }
    }
    /// Releitura: compara o índice antigo com o novo e grava a diferença. O horário de cada mudança vem do
    /// diário do NTFS quando ele cobre o período (Seek fechado); senão fica o horário da releitura.
    pub fn record_diff(&mut self, old: Arc<Vec<Entry>>, new: Arc<Vec<Entry>>) {
        self.hist.summary = None;
        let Some(dir) = self.log.as_ref().map(|l| l.dir.clone()) else { return };
        thread::spawn(move || {
            let mut deltas = history::diff_sorted(&old, &new, velo::clock::now_secs());
            drop((old, new));
            let usn_file = dir.join("usn.txt");
            let saved = velo::usn::load(&usn_file);
            let mut times: HashMap<char, HashMap<String, u64>> = HashMap::new();
            for d in &mut deltas {
                let Some(letter) = d.path.chars().next().map(|c| c.to_ascii_uppercase()) else { continue };
                let map = times.entry(letter).or_insert_with(|| saved.get(&letter).map(|&(id, usn)| velo::usn::times_since(letter, id, usn)).unwrap_or_default());
                let name = velo::normalize(Path::new(&*d.path).file_name().map(|n| n.to_string_lossy()).unwrap_or_default().as_ref());
                if let Some(&t) = map.get(&name) {
                    d.at = t;
                }
            }
            if !deltas.is_empty() {
                velo::log::line(&format!("histórico: {} mudanças achadas na releitura", deltas.len()));
            }
            let _ = (history::Log { dir: dir.clone() }).record(&deltas);
            let letters: Vec<char> = velo::fixed_drives().iter().filter_map(|d| velo::mft::drive_letter(d)).collect();
            velo::usn::save(&usn_file, &letters);
        });
    }
}

fn seal_color(s: Seal) -> Color32 {
    match s {
        Seal::Delete => Color32::from_rgb(70, 170, 100),
        Seal::Move => Color32::from_rgb(80, 140, 230),
        Seal::Keep => Color32::from_rgb(215, 80, 70),
        Seal::Info => c_mute(),
    }
}
fn seal_label(s: Seal) -> &'static str {
    match s {
        Seal::Delete => "pode apagar",
        Seal::Move => "pode mover",
        Seal::Keep => "não mexa",
        Seal::Info => "",
    }
}
fn kind_color(k: Kind) -> Color32 {
    let (r, g, b) = match k {
        Kind::Photo => (196, 140, 70),
        Kind::Video => (190, 90, 110),
        Kind::Audio => (150, 110, 190),
        Kind::Document => (90, 150, 200),
        Kind::App => (110, 170, 120),
        Kind::Code => (120, 130, 150),
        Kind::Archive => (180, 160, 90),
        Kind::Disk => (160, 120, 90),
        Kind::Design => (90, 170, 170),
        Kind::System => (110, 110, 110),
        _ => (95, 95, 95),
    };
    Color32::from_rgb(r, g, b)
}
fn drive_of(path: &Path) -> String {
    path.to_string_lossy().trim_start_matches("\\\\?\\").chars().take(2).collect()
}
fn pct(part: u64, total: u64) -> f32 {
    if total == 0 { 0. } else { part as f32 / total as f32 }
}
fn bar(ui: &egui::Ui, rect: Rect, fill: f32, color: Color32) {
    ui.painter().rect_filled(rect, 2, c_line());
    let w = rect.width() * fill.clamp(0., 1.);
    if w > 0.5 {
        ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(w, rect.height())), 2, color);
    }
}
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(14.);
    ui.label(RichText::new(text).size(16.).color(c_text()));
    ui.add_space(6.);
}

impl App {
    fn sizes(&mut self) -> Arc<Sizes> {
        if let Some((w, s)) = &self.space.sizes
            && w.as_ptr() == Arc::as_ptr(&self.entries)
        {
            return s.clone();
        }
        let s = Arc::new(Sizes::new(&self.entries));
        self.space.sizes = Some((Arc::downgrade(&self.entries), s.clone()));
        s
    }
    fn listing(&mut self, folder: &Path) -> Arc<Listing> {
        if let Some((w, l)) = &self.space.listing
            && w.as_ptr() == Arc::as_ptr(&self.entries)
            && l.folder == folder
        {
            return l.clone();
        }
        let sizes = self.sizes();
        let e = &self.entries;
        let children = space::children(e, &sizes, folder);
        let (lo, hi) = space::range(e, &space::prefix_of(folder));
        let added = children.iter().map(|c| e[c.lo..c.hi].iter().map(|x| x.created).max().unwrap_or(0)).collect();
        let l = Arc::new(Listing { folder: folder.to_path_buf(), kinds: space::by_kind(&e[lo..hi]), total: sizes.sum(lo, hi), files: hi - lo, children, added });
        self.space.listing = Some((Arc::downgrade(&self.entries), l.clone()));
        l
    }
    fn folder_size(&mut self, folder: &Path) -> u64 {
        let sizes = self.sizes();
        let (lo, hi) = space::range(&self.entries, &space::prefix_of(folder));
        sizes.sum(lo, hi)
    }
    pub(crate) fn space_tabs(&mut self, ui: &mut egui::Ui) {
        for (page, label) in [(Page::Home, "Início"), (Page::Browse, "Pastas"), (Page::History, "Histórico"), (Page::Games, "Jogos"), (Page::Actions, "Registro")] {
            if ui.add(flat(label).selected(self.space.page == page)).clicked() {
                self.space.page = page;
            }
        }
    }
    /// Chamado a cada quadro, com a aba aberta ou não: trabalhos em segundo plano, amostra dos discos e alertas.
    pub(crate) fn space_tick(&mut self, ctx: &egui::Context) {
        let s = &mut self.space;
        if s.last_sample.is_none_or(|t| t.elapsed() > Duration::from_secs(3600)) {
            s.last_sample = Some(Instant::now());
            let drives = list_drives();
            for (d, total, free) in &drives {
                let used = pct(total - free, *total);
                if used >= 0.9 && s.alerted.insert(d.clone()) {
                    crate::tray::notify("Disco quase cheio", &format!("{} está {:.0}% cheio: sobram {}.", d.display(), used * 100., bytes(*free)));
                } else if used < 0.88 {
                    s.alerted.remove(d);
                }
            }
            if let Some(dir) = s.log.as_ref().map(|l| l.dir.clone()) {
                thread::spawn(move || {
                    let _ = (history::Log { dir: dir.clone() }).sample_disks(velo::clock::now_secs(), &drives);
                    // a posição do diário do NTFS anda junto: ao reabrir, o horário das mudanças sai dela
                    let letters: Vec<char> = velo::fixed_drives().iter().filter_map(|d| velo::mft::drive_letter(d)).collect();
                    velo::usn::save(&dir.join("usn.txt"), &letters);
                });
            }
            s.hist.disks = None;
        }
        ctx.request_repaint_after(Duration::from_secs(60));
        if let Some(sug) = s.suggest_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            s.suggest_rx = None;
            s.suggest = Some((Instant::now(), Arc::new(sug)));
        }
        if let Some(d) = s.dup_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            s.dup_rx = None;
            s.dups = Some(d);
        }
        if let Some(g) = s.games_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            s.games_rx = None;
            s.games = Some(g);
        }
        if let Some((label, list)) = s.hist.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            s.hist.rx = None;
            s.hist.view = Some((label, Arc::new(list)));
            s.hist.grouped = None;
        }
        if s.suggest_rx.is_some() || s.dup_rx.is_some() || s.games_rx.is_some() || s.hist.rx.is_some() || s.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        let Some(done) = s.job.as_ref().and_then(|j| j.rx.try_recv().ok()) else { return };
        s.job = None;
        let at = velo::clock::now_secs();
        match done {
            JobDone::Recycled(list) => {
                let (mut n, mut freed, mut failed) = (0, 0u64, Vec::new());
                for (path, size, r) in list {
                    match r {
                        Ok(()) => {
                            n += 1;
                            freed += size;
                            s.actions.push(cleanup::Action { at, kind: cleanup::Kind::Recycle, undone: false, bytes: size, from: path, to: PathBuf::new() });
                        }
                        Err(e) => failed.push(format!("{} ({e})", path.display())),
                    }
                }
                s.notice = format!("{n} itens foram para a Lixeira ({}). O espaço volta ao esvaziar a Lixeira.", bytes(freed));
                if !failed.is_empty() {
                    s.notice += &format!(" {} não foram: {}", failed.len(), failed.iter().take(3).cloned().collect::<Vec<_>>().join("; "));
                }
                s.suggest = None;
                s.dups = None;
                s.bin = None;
            }
            JobDone::Moved(from, size, r) => match r {
                Ok(to) => {
                    s.notice = format!("Movido para {}. No lugar antigo ficou um atalho (junction).", to.display());
                    s.actions.push(cleanup::Action { at, kind: cleanup::Kind::Junction, undone: false, bytes: size, from, to });
                }
                Err(e) => s.notice = format!("Não foi movido: {e}"),
            },
            JobDone::Undone(i, r) => match r {
                Ok(()) => {
                    s.actions.mark_undone(i);
                    s.notice = "Desfeito.".into();
                }
                Err(e) => s.notice = format!("Não deu para desfazer: {e}"),
            },
        }
    }
    fn start_job(&mut self, label: String, total: u64, work: impl FnOnce(&AtomicU64, &AtomicBool) -> JobDone + Send + 'static, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let (done, cancel) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
        let (d, c, ctx) = (done.clone(), cancel.clone(), ctx.clone());
        thread::spawn(move || {
            let _ = tx.send(work(&d, &c));
            ctx.request_repaint();
        });
        self.space.job = Some(Job { label, done, total, cancel, rx });
        self.space.notice.clear();
    }
    fn open_review(&mut self, title: &str, groups: Vec<(String, Vec<Item>)>) {
        let mut items = Vec::new();
        for (g, list) in groups {
            for it in list {
                let blocked = knowledge::protected(&it.path, false).map(str::to_owned).or_else(|| (!cleanup::has_recycle_bin(&it.path)).then(|| "Disco sem Lixeira: seria apagado de vez.".to_owned()));
                items.push((g.clone(), it, blocked));
            }
        }
        self.space.review = Some(Review { title: title.into(), items, checked: HashSet::new(), confirm: false });
    }
    fn ask_move(&mut self, src: PathBuf, exe: String) {
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = self.folder_size(&src);
        self.space.drives = list_drives();
        self.space.moving = Some(MoveAsk { src, name, bytes, exe, target: None });
    }

    pub(crate) fn space_ui(&mut self, ctx: &egui::Context) {
        if self.space.drives_at.is_none_or(|t| t.elapsed() > Duration::from_secs(10)) {
            self.space.drives = list_drives();
            self.space.drives_at = Some(Instant::now());
        }
        egui::TopBottomPanel::bottom("space_status")
            .exact_height(34.)
            .frame(egui::Frame::new().fill(c_bg()).inner_margin(egui::Margin::symmetric(18, 0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(j) = &self.space.job {
                        let done = j.done.load(Ordering::Relaxed);
                        let text = if j.total > 0 { format!("{} · {} de {}", j.label, bytes(done), bytes(j.total)) } else { format!("{}…", j.label) };
                        ui.label(RichText::new(text).monospace().color(c_text()));
                        if j.total > 0 && ui.add(flat("Cancelar")).clicked() {
                            j.cancel.store(true, Ordering::Relaxed);
                        }
                    } else if !self.space.notice.is_empty() {
                        ui.add(egui::Label::new(RichText::new(&self.space.notice).color(c_text())).truncate());
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if self.scan_rx.is_some() {
                            ui.label(RichText::new(format!("lendo o disco… {} arquivos", self.progress.load(Ordering::Relaxed))).monospace().small().color(c_mute()));
                        }
                    });
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(c_bg()).inner_margin(egui::Margin::symmetric(18, 8))).show(ctx, |ui| {
            if self.entries.is_empty() {
                ui.add_space(40.);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(if self.scan_rx.is_some() { "Lendo o disco…" } else { "Escolha um disco para começar." }).color(c_dim()));
                });
                return;
            }
            match self.space.page {
                Page::Home => self.page_home(ui, ctx),
                Page::Browse => self.page_browse(ui, ctx),
                Page::History => self.page_history(ui, ctx),
                Page::Games => self.page_games(ui, ctx),
                Page::Actions => self.page_actions(ui, ctx),
            }
        });
        self.review_window(ctx);
        self.move_window(ctx);
    }

    // ---- Início ----
    fn page_home(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let now = velo::clock::now_secs();
        if self.space.suggest.as_ref().is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(120)) && self.space.suggest_rx.is_none() && self.scan_rx.is_none() {
            let (tx, rx) = mpsc::channel();
            let (entries, ctx2) = (self.entries.clone(), ctx.clone());
            thread::spawn(move || {
                let s = Suggest { junk: space::junk(&entries), installers: space::old_installers(&entries, now), forgotten: space::forgotten(&entries, now) };
                let _ = tx.send(s);
                ctx2.request_repaint();
            });
            self.space.suggest_rx = Some(rx);
        }
        if self.space.games.is_none() && self.space.games_rx.is_none() {
            self.load_games(ctx);
        }
        if self.space.bin.as_ref().is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(30)) {
            self.space.bin = Some((Instant::now(), cleanup::recycle_bin_size()));
        }
        let sizes = self.sizes();
        let drives = self.space.drives.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            heading(ui, "Discos");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(12., 12.);
                if self.space.growth.as_ref().is_none_or(|(w, _)| w.as_ptr() != Arc::as_ptr(&self.entries)) {
                    let g = drives
                        .iter()
                        .map(|(d, _, _)| {
                            let (lo, hi) = space::range(&self.entries, &space::prefix_of(d));
                            let e = &self.entries[lo..hi];
                            (hi > lo, space::created_since(e, now.saturating_sub(7 * 86_400)), space::created_since(e, now.saturating_sub(30 * 86_400)))
                        })
                        .collect();
                    self.space.growth = Some((Arc::downgrade(&self.entries), g));
                }
                let growth = self.space.growth.as_ref().unwrap().1.clone();
                for ((d, total, free), (indexed, week, month)) in drives.iter().zip(growth) {
                    let used = total - free;
                    let (rect, resp) = ui.allocate_exact_size(vec2(340., 96.), if indexed { Sense::click() } else { Sense::hover() });
                    let p = ui.painter();
                    p.rect_filled(rect, 6, if resp.hovered() && indexed { c_hover() } else { c_panel() });
                    p.text(rect.min + vec2(14., 12.), Align2::LEFT_TOP, d.display().to_string(), FontId::proportional(18.), c_text());
                    p.text(rect.right_top() + vec2(-14., 16.), Align2::RIGHT_TOP, format!("{} livres de {}", bytes(*free), bytes(*total)), FontId::proportional(12.), c_dim());
                    let fill = pct(used, *total);
                    let color = if fill >= 0.9 { Color32::from_rgb(215, 80, 70) } else { c_text() };
                    bar(ui, Rect::from_min_size(rect.min + vec2(14., 44.), vec2(rect.width() - 28., 8.)), fill, color);
                    let info = if indexed {
                        format!("{:.0}% cheio · entrou {} em 7 dias, {} em 30", fill * 100., bytes(week), bytes(month))
                    } else {
                        format!("{:.0}% cheio · fora do índice", fill * 100.)
                    };
                    ui.painter().text(rect.min + vec2(14., 64.), Align2::LEFT_TOP, info, FontId::proportional(12.), c_mute());
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && indexed {
                        self.space.folder = Some(d.clone());
                        self.space.page = Page::Browse;
                    }
                }
            });
            heading(ui, "Dá para liberar");
            let sug = self.space.suggest.clone();
            let mut open: Option<(&str, Vec<(String, Vec<Item>)>)> = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(12., 12.);
                let total = |v: &[Item]| v.iter().map(|i| i.bytes).sum::<u64>();
                // `size`: None = ainda calculando; o botão só aparece quando há algo
                let card = |ui: &mut egui::Ui, title: &str, size: Option<u64>, count: String, button: &str| -> bool {
                    let mut clicked = false;
                    // tamanho fixo e conhecido antes: é o que deixa a linha quebrar quando não cabe
                    ui.allocate_ui(vec2(228., 112.), |ui| egui::Frame::new().fill(c_panel()).corner_radius(6).inner_margin(14).show(ui, |ui| {
                        ui.set_width(200.);
                        ui.set_height(84.);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(title).color(c_dim()));
                            ui.label(RichText::new(match size { None => "…".into(), Some(u64::MAX) => "—".into(), Some(s) => bytes(s) }).size(20.).color(c_text()));
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(count).small().color(c_mute()));
                                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                    if !button.is_empty() && size.is_some_and(|s| s > 0) {
                                        clicked = ui.add(flat(button)).clicked();
                                    }
                                });
                            });
                        });
                    }));
                    clicked
                };
                let (j, i, f) = match &sug {
                    Some((_, s)) => (Some(&s.junk), Some(&s.installers), Some(&s.forgotten)),
                    None => (None, None, None),
                };
                let n = |v: Option<&Vec<Item>>| v.map_or(String::new(), |v| format!("{} itens", v.len()));
                if card(ui, "Lixo e caches", j.map(|v| total(v)), n(j), "Revisar") {
                    open = Some(("Lixo e caches", group_by_what(j.unwrap())));
                }
                if card(ui, "Instaladores velhos", i.map(|v| total(v)), n(i), "Revisar") {
                    open = Some(("Instaladores velhos (Downloads)", vec![("Downloads".into(), i.unwrap().clone())]));
                }
                if card(ui, "Grandes e esquecidos", f.map(|v| total(v)), n(f), "Revisar") {
                    open = Some(("Arquivos grandes esquecidos", vec![("Mais de 500 MB, sem mudar há 6 meses".into(), f.unwrap().clone())]));
                }
                let dup_size = self.space.dups.as_ref().map(|d| total(d));
                let dup_count = match (&self.space.dups, &self.space.dup_rx) {
                    (_, Some(_)) => format!("comparando… {}", self.space.dup_progress.load(Ordering::Relaxed)),
                    (Some(d), _) => format!("{} cópias", d.len()),
                    _ => "50 MB ou mais".into(),
                };
                if self.space.dups.is_some() {
                    if card(ui, "Duplicados", dup_size, dup_count, "Revisar") {
                        open = Some(("Arquivos duplicados", group_dups(self.space.dups.as_ref().unwrap())));
                    }
                } else {
                    let busy = self.space.dup_rx.is_some();
                    if card(ui, "Duplicados", Some(u64::MAX), dup_count, if busy { "" } else { "Procurar" }) {
                        self.find_dups(ctx);
                    }
                }
                let (bin_size, bin_n) = self.space.bin.map_or((0, 0), |b| b.1);
                if card(ui, "Lixeira", Some(bin_size), format!("{bin_n} itens"), "Esvaziar…") {
                    let _ = cleanup::run_official(Act::EmptyBin, "C:");
                    self.space.bin = None;
                }
                let idle: Vec<&Game> = self.space.games.as_ref().map_or(Vec::new(), |g| g.iter().filter(|g| g.last_played > 0 && g.last_played + 90 * 86_400 < now).collect());
                let idle_size: u64 = idle.iter().map(|g| { let (lo, hi) = space::range(&self.entries, &space::prefix_of(&g.dir)); sizes.sum(lo, hi) }).sum();
                if card(ui, "Jogos parados há 90 dias", self.space.games.as_ref().map(|_| idle_size), format!("{} jogos", idle.len()), "Ver jogos") {
                    self.space.page = Page::Games;
                }
            });
            if let Some((title, groups)) = open {
                self.open_review(title, groups);
            }
            heading(ui, "Coisas do Windows");
            self.windows_rows(ui);
        });
    }
    fn windows_rows(&mut self, ui: &mut egui::Ui) {
        let Some(sys) = std::env::var("SystemDrive").ok().filter(|s| s.len() == 2) else { return };
        let items = [
            ("\\Windows\\WinSxS", true),
            ("\\hiberfil.sys", false),
            ("\\Windows.old", true),
            ("\\$WINDOWS.~BT", true),
            ("\\Windows\\SoftwareDistribution", true),
            ("\\Windows\\Temp", true),
            ("\\System Volume Information", true),
        ];
        for (rel, dir) in items {
            let path = PathBuf::from(format!("{sys}{rel}"));
            let size = if dir {
                self.folder_size(&path)
            } else {
                let key = velo::normalize(&path.to_string_lossy());
                let i = self.entries.partition_point(|e| *e.searchable < *key);
                self.entries.get(i).filter(|e| *e.searchable == *key).map_or(0, |e| e.bytes)
            };
            let Some(rule) = knowledge::explain(&path) else { continue };
            let always = matches!(rule.act, Act::Restore);
            if size == 0 && !always {
                continue;
            }
            egui::Frame::new().fill(c_panel()).corner_radius(6).inner_margin(12).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(rule.what).color(c_text()));
                    ui.label(RichText::new(if size > 0 { bytes(size) } else { "tamanho não informado".into() }).monospace().color(c_dim()));
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if rule.act != Act::None && ui.add(flat(cleanup::official_label(rule.act))).clicked() {
                            if let Err(e) = cleanup::run_official(rule.act, &sys) {
                                self.space.notice = e;
                            }
                        }
                    });
                });
                ui.label(RichText::new(rule.why).small().color(c_mute()));
            });
            ui.add_space(6.);
        }
    }
    fn find_dups(&mut self, ctx: &egui::Context) {
        let candidates: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].bytes >= 50 << 20).collect();
        let entries = self.entries.clone();
        let (tx, rx) = mpsc::channel();
        self.space.dup_cancel = Arc::new(AtomicBool::new(false));
        self.space.dup_progress = Arc::new(AtomicUsize::new(0));
        let (cancel, progress, ctx) = (self.space.dup_cancel.clone(), self.space.dup_progress.clone(), ctx.clone());
        thread::spawn(move || {
            let groups = velo::find_duplicates(&entries, &candidates, &cancel, &progress);
            let mut items = Vec::new();
            for g in groups {
                let first = entries[g[0]].path_str().to_owned();
                for &i in &g[1..] {
                    let e = &entries[i];
                    items.push(Item { path: e.path().to_path_buf(), bytes: e.bytes, date: e.modified, what: "Cópia idêntica".into(), why: format!("Mesmo conteúdo de {first}") });
                }
            }
            let _ = tx.send(items);
            ctx.request_repaint();
        });
        self.space.dup_rx = Some(rx);
    }
    fn load_games(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(velo::games::all());
            ctx.request_repaint();
        });
        self.space.games_rx = Some(rx);
    }

    // ---- Pastas ----
    fn page_browse(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(folder) = self.space.folder.clone() else {
            // sem pasta: lista os discos que estão no índice
            ui.label(RichText::new("Escolha um disco").color(c_dim()));
            for (d, _, _) in self.space.drives.clone() {
                let size = self.folder_size(&d);
                if size > 0 && ui.add(flat(&format!("{}  {}", d.display(), bytes(size)))).clicked() {
                    self.space.folder = Some(d);
                }
            }
            return;
        };
        let l = self.listing(&folder);
        let mut go: Option<PathBuf> = None;
        // trilha
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.;
            if ui.add(flat("Discos")).clicked() {
                self.space.folder = None;
            }
            let mut acc = PathBuf::new();
            for (i, part) in folder.components().enumerate() {
                acc.push(part);
                if matches!(part, std::path::Component::RootDir) {
                    continue;
                }
                ui.label(RichText::new("›").color(c_mute()));
                let label = part.as_os_str().to_string_lossy().into_owned();
                let label = if i == 0 { format!("{label}\\") } else { label };
                if ui.add(flat(&label)).clicked() {
                    go = Some(if i == 0 { PathBuf::from(format!("{}\\", part.as_os_str().to_string_lossy())) } else { acc.clone() });
                }
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{} · {} arquivos", bytes(l.total), l.files)).monospace().color(c_dim()));
            });
        });
        // categorias
        let total = l.total.max(1);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.), Sense::hover());
        let mut x = rect.left();
        for (k, b, _) in &l.kinds {
            let w = rect.width() * *b as f32 / total as f32;
            ui.painter().rect_filled(Rect::from_min_size(egui::pos2(x, rect.top()), vec2(w, rect.height())), 0, kind_color(*k));
            x += w;
        }
        ui.horizontal_wrapped(|ui| {
            for (k, b, _) in l.kinds.iter().take(7) {
                let (r, _) = ui.allocate_exact_size(vec2(9., 9.), Sense::hover());
                ui.painter().rect_filled(r, 2, kind_color(*k));
                ui.label(RichText::new(format!("{} {}", k.label(), bytes(*b))).small().color(c_dim()));
            }
        });
        if let Some(rule) = knowledge::explain(&folder) {
            ui.label(RichText::new(format!("{}: {}", rule.what, rule.why)).small().color(c_mute()));
        }
        ui.add_space(6.);
        let avail = ui.available_size();
        let list_w = (avail.x * 0.58).max(360.);
        let drive_root = folder.parent().is_none();
        let outside = if drive_root {
            self.space.drives.iter().find(|d| d.0 == folder).map_or(0, |(_, t, f)| (t - f).saturating_sub(l.total))
        } else {
            0
        };
        let mut menu: Option<(PathBuf, bool, u8)> = None; // (caminho, é pasta, ação)
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(list_w, avail.y), egui::Layout::top_down(Align::Min), |ui| {
                let max = l.children.first().map_or(1, |c| c.bytes.max(1));
                let rows = l.children.len() + usize::from(outside > 0);
                let mut hovered = None;
                egui::ScrollArea::vertical().id_salt("space_list").auto_shrink([false, false]).show_rows(ui, 30., rows, |ui, range| {
                    for row in range {
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.), Sense::click());
                        if row == l.children.len() {
                            ui.painter().text(rect.left_center() + vec2(22., 0.), Align2::LEFT_CENTER, "Fora do índice", FontId::proportional(14.), c_dim());
                            ui.painter().text(rect.right_center() - vec2(8., 0.), Align2::RIGHT_CENTER, bytes(outside), FontId::monospace(12.), c_dim());
                            resp.on_hover_text("Arquivos do sistema e pastas que o Seek não consegue ler (pontos de restauração, Lixeira, outros usuários). Como administrador entra mais coisa.");
                            continue;
                        }
                        let c = &l.children[row];
                        let hot = resp.hovered() || self.space.hover == Some(row);
                        if resp.hovered() {
                            hovered = Some(row);
                        }
                        if hot {
                            ui.painter().rect_filled(rect, 4, c_hover());
                        }
                        let rule = knowledge::explain(&c.path);
                        if let Some(r) = rule.filter(|r| r.seal != Seal::Info) {
                            ui.painter().circle_filled(rect.left_center() + vec2(8., 0.), 4., seal_color(r.seal));
                        }
                        let name_w = rect.width() * 0.38;
                        let name = if c.is_dir { format!("{}\\", c.name) } else { c.name.clone() };
                        let g = clipped(ui, &name, FontId::proportional(14.), if c.is_dir { c_text() } else { c_dim() }, name_w);
                        ui.painter().galley(egui::pos2(rect.left() + 20., rect.center().y - g.size().y / 2.), g, c_text());
                        let bar_r = Rect::from_min_size(egui::pos2(rect.left() + 26. + name_w, rect.center().y - 3.), vec2(rect.width() * 0.16, 6.));
                        bar(ui, bar_r, pct(c.bytes, max), if c.is_dir { c_dim() } else { c_mute() });
                        let x = bar_r.right() + 10.;
                        ui.painter().text(egui::pos2(x + 70., rect.center().y), Align2::RIGHT_CENTER, bytes(c.bytes), FontId::monospace(12.), c_text());
                        ui.painter().text(egui::pos2(x + 112., rect.center().y), Align2::RIGHT_CENTER, format!("{:.0}%", pct(c.bytes, total) * 100.), FontId::monospace(11.), c_mute());
                        let extra = match rule {
                            Some(r) => r.what.to_string(),
                            None if l.added[row] > 0 => format!("entrou {}", &fmt_date(l.added[row])[..10]),
                            None => String::new(),
                        };
                        let g = clipped(ui, &extra, FontId::proportional(12.), c_mute(), (rect.right() - x - 124.).max(10.));
                        ui.painter().galley(egui::pos2(x + 124., rect.center().y - g.size().y / 2.), g, c_mute());
                        let resp = resp.on_hover_text(match rule {
                            Some(r) => format!("{}\n{}{}\nentrou: {} · {} arquivos", r.what, r.why, match seal_label(r.seal) { "" => String::new(), s => format!("\n({s})") }, fmt_date(l.added[row]), c.files),
                            None => format!("{}\nentrou: {} · {} arquivos", c.path.display(), fmt_date(l.added[row]), c.files),
                        });
                        if resp.clicked() && c.is_dir {
                            go = Some(c.path.clone());
                        }
                        if resp.double_clicked() && !c.is_dir {
                            open_file(&c.path);
                        }
                        let (path, is_dir) = (c.path.clone(), c.is_dir);
                        resp.context_menu(|ui| {
                            if ui.button("Mostrar no Explorer").clicked() {
                                menu = Some((path.clone(), is_dir, 0));
                                ui.close();
                            }
                            if ui.button("Revisar para apagar…").clicked() {
                                menu = Some((path.clone(), is_dir, 1));
                                ui.close();
                            }
                            if is_dir && ui.button("Mover para outro disco…").clicked() {
                                menu = Some((path.clone(), is_dir, 2));
                                ui.close();
                            }
                        });
                    }
                });
                self.space.hover = hovered;
            });
            // treemap
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), avail.y), Sense::click());
            let shown: Vec<&Child> = l.children.iter().filter(|c| c.bytes > 0).take(80).collect();
            let sizes: Vec<u64> = shown.iter().map(|c| c.bytes).collect();
            let rects = space::squarify(&sizes, (rect.left(), rect.top(), rect.width(), rect.height()));
            let pointer = resp.hover_pos();
            let mut tip = None;
            for (i, (c, r)) in shown.iter().zip(&rects).enumerate() {
                let r = Rect::from_min_size(egui::pos2(r.0, r.1), vec2(r.2, r.3));
                let base = if c.is_dir { c_tile() } else { kind_color(self.entries[c.lo].kind).gamma_multiply(0.55) };
                let hot = pointer.is_some_and(|p| r.contains(p)) || self.space.hover == Some(i);
                ui.painter().rect_filled(r.shrink(1.), 3, if hot { c_sel() } else { base });
                ui.painter().rect_stroke(r.shrink(1.), 3, Stroke::new(1_f32, c_bg()), StrokeKind::Inside);
                if r.width() > 54. && r.height() > 30. {
                    let g = clipped(ui, &c.name, FontId::proportional(12.), c_text(), r.width() - 10.);
                    ui.painter().galley(r.min + vec2(5., 4.), g, c_text());
                    ui.painter().text(r.min + vec2(5., 19.), Align2::LEFT_TOP, bytes(c.bytes), FontId::monospace(10.), c_dim());
                }
                if pointer.is_some_and(|p| r.contains(p)) {
                    tip = Some(i);
                }
            }
            if let Some(i) = tip {
                let c = shown[i];
                let resp = resp.on_hover_text(format!("{}\n{}", c.path.display(), bytes(c.bytes)));
                if resp.clicked() && c.is_dir {
                    go = Some(c.path.clone());
                }
            }
        });
        if let Some((path, is_dir, action)) = menu {
            match action {
                0 => reveal(&path),
                1 => {
                    let size = if is_dir { self.folder_size(&path) } else { l.children.iter().find(|c| c.path == path).map_or(0, |c| c.bytes) };
                    let rule = knowledge::explain(&path);
                    let item = Item {
                        path: path.clone(),
                        bytes: size,
                        date: l.children.iter().position(|c| c.path == path).map_or(0, |i| l.added[i]),
                        what: rule.map_or(if is_dir { "Pasta" } else { "Arquivo" }.into(), |r| r.what.into()),
                        why: rule.map_or("Escolhido por você.".into(), |r| r.why.into()),
                    };
                    self.open_review("Apagar", vec![("Escolhido por você".into(), vec![item])]);
                }
                _ => self.ask_move(path, String::new()),
            }
        }
        if let Some(p) = go {
            self.space.folder = Some(p);
            self.space.hover = None;
        }
        let _ = ctx;
    }

    // ---- Histórico ----
    fn page_history(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(log) = &self.space.log else {
            ui.label("Sem pasta para o histórico.");
            return;
        };
        if self.space.hist.summary.as_ref().is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(10)) {
            self.space.hist.summary = Some((Instant::now(), log.summary()));
            self.space.hist.size = log.size();
        }
        if self.space.hist.disks.is_none() {
            self.space.hist.disks = Some(log.disks());
        }
        if self.space.hist.created.as_ref().is_none_or(|(w, _)| w.as_ptr() != Arc::as_ptr(&self.entries)) {
            let mut m: BTreeMap<i64, u64> = BTreeMap::new();
            let off = velo::clock::local_offset_secs();
            for e in self.entries.iter().filter(|e| e.created > 0) {
                *m.entry((e.created as i64 + off).div_euclid(86_400)).or_default() += e.bytes;
            }
            self.space.hist.created = Some((Arc::downgrade(&self.entries), m));
        }
        let summary = self.space.hist.summary.as_ref().unwrap().1.clone();
        let first_logged = summary.keys().next().cloned();
        let today = (velo::clock::now_secs() as i64 + velo::clock::local_offset_secs()).div_euclid(86_400);
        const DAYS: i64 = 45;
        let mut pick: Option<String> = None;
        {
            let created = &self.space.hist.created.as_ref().unwrap().1;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 150.), Sense::click());
            ui.painter().rect_filled(rect, 6, c_panel());
            let inner = rect.shrink2(vec2(10., 14.));
            let mid = inner.top() + inner.height() * 0.68;
            let day_name = |d: i64| {
                let (y, m, dd) = velo::clock::civil_from_days(d);
                format!("{y:04}-{m:02}-{dd:02}")
            };
            let vals: Vec<(i64, u64, u64, bool)> = (today - DAYS + 1..=today)
                .map(|d| match summary.get(&day_name(d)) {
                    Some(s) => (d, s.grew, s.shrank, true),
                    None if first_logged.as_ref().is_none_or(|f| day_name(d) < *f) => (d, created.get(&d).copied().unwrap_or(0), 0, false),
                    None => (d, 0, 0, true),
                })
                .collect();
            let top = vals.iter().map(|v| v.1.max(v.2)).max().unwrap_or(1).max(1) as f32;
            let w = inner.width() / DAYS as f32;
            ui.painter().line_segment([egui::pos2(inner.left(), mid), egui::pos2(inner.right(), mid)], Stroke::new(1_f32, c_border()));
            let pointer = resp.hover_pos();
            for (i, (d, up, down, logged)) in vals.iter().enumerate() {
                let x = inner.left() + i as f32 * w;
                let col = Rect::from_min_max(egui::pos2(x, inner.top()), egui::pos2(x + w, inner.bottom()));
                let hot = pointer.is_some_and(|p| col.contains(p));
                let selected = self.space.hist.view.as_ref().is_some_and(|v| v.0 == day_name(*d));
                if hot || selected {
                    ui.painter().rect_filled(col, 2, c_hover());
                }
                let hu = (mid - inner.top()) * (*up as f32 / top).sqrt();
                let hd = (inner.bottom() - mid) * (*down as f32 / top).sqrt();
                let color = if *logged { c_text() } else { c_border() };
                if hu > 0.5 {
                    ui.painter().rect_filled(Rect::from_min_max(egui::pos2(x + 1., mid - hu), egui::pos2(x + w - 1., mid)), 1, color);
                }
                if hd > 0.5 {
                    ui.painter().rect_filled(Rect::from_min_max(egui::pos2(x + 1., mid), egui::pos2(x + w - 1., mid + hd)), 1, c_mute());
                }
                let (_, m, dd) = velo::clock::civil_from_days(*d);
                if dd == 1 || i % 7 == 0 {
                    ui.painter().text(egui::pos2(x, rect.bottom() - 2.), Align2::LEFT_BOTTOM, format!("{dd:02}/{m:02}"), FontId::monospace(9.), c_mute());
                }
                if hot {
                    let tip = if *logged { format!("{dd:02}/{m:02}: entrou {}, saiu {}", bytes(*up), bytes(*down)) } else { format!("{dd:02}/{m:02} (antes do registro): {} em arquivos criados neste dia", bytes(*up)) };
                    let r = resp.clone().on_hover_text(tip);
                    if r.clicked() && *logged {
                        pick = Some(day_name(*d));
                    }
                }
            }
        }
        if let Some(day) = pick {
            self.load_history(ctx, day.clone(), day);
        }
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.label(RichText::new("De").color(c_dim()));
            ui.add(egui::TextEdit::singleline(&mut self.space.hist.from).desired_width(90.));
            ui.label(RichText::new("até").color(c_dim()));
            ui.add(egui::TextEdit::singleline(&mut self.space.hist.to).desired_width(90.));
            if ui.add_enabled(self.space.hist.rx.is_none(), flat("Comparar")).clicked() {
                let (a, b) = (self.space.hist.from.trim().to_owned(), self.space.hist.to.trim().to_owned());
                if history::day_start(&a).is_some() && history::day_start(&b).is_some() && a <= b {
                    self.load_history(ctx, a, b);
                } else {
                    self.space.notice = "Use datas como 2026-10-20 (a primeira antes da segunda).".into();
                }
            }
            if self.space.hist.rx.is_some() {
                ui.label(RichText::new("carregando…").color(c_mute()));
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("histórico: {}", bytes(self.space.hist.size))).small().color(c_mute()));
            });
        });
        // uso dos discos registrado de hora em hora
        if let Some(disks) = &self.space.hist.disks {
            let week = velo::clock::now_secs().saturating_sub(7 * 86_400);
            ui.horizontal_wrapped(|ui| {
                for (d, samples) in disks {
                    let (Some(first), Some(last)) = (samples.iter().find(|s| s.0 >= week).or(samples.first()), samples.last()) else { continue };
                    let diff = last.1 as i64 - first.1 as i64;
                    ui.label(RichText::new(format!("{d} desde {}: {}{}", fmt_date(first.0), if diff >= 0 { "+" } else { "−" }, bytes(diff.unsigned_abs()))).small().color(c_dim()));
                }
            });
        }
        let Some((label, deltas)) = self.space.hist.view.clone() else {
            ui.add_space(20.);
            ui.label(RichText::new(if summary.is_empty() { "O registro começa agora: cada arquivo que entrar, sair ou mudar de tamanho fica anotado." } else { "Clique num dia ou compare duas datas." }).color(c_dim()));
            return;
        };
        ui.add_space(8.);
        let folder = self.space.hist.folder.clone();
        ui.horizontal(|ui| {
            ui.label(RichText::new(&label).color(c_text()));
            ui.label(RichText::new(format!("{} mudanças", deltas.len())).small().color(c_mute()));
            ui.add_space(10.);
            if ui.add(flat("Tudo").selected(folder.is_none())).clicked() {
                self.space.hist.folder = None;
            }
            if let Some(f) = &folder {
                let mut acc = PathBuf::new();
                for (i, part) in f.components().filter(|c| !matches!(c, std::path::Component::RootDir)).enumerate() {
                    if i == 0 {
                        acc = PathBuf::from(format!("{}\\", part.as_os_str().to_string_lossy()));
                    } else {
                        acc.push(part);
                    }
                    ui.label(RichText::new("›").color(c_mute()));
                    if ui.add(flat(&part.as_os_str().to_string_lossy())).clicked() {
                        self.space.hist.folder = Some(acc.clone());
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.checkbox(&mut self.space.hist.files, "Ver arquivos");
            });
        });
        let folder = self.space.hist.folder.clone();
        if self.space.hist.files {
            let prefix = folder.as_ref().map(|f| space::prefix_of(f));
            let mut list: Vec<&Delta> = deltas.iter().filter(|d| prefix.as_ref().is_none_or(|p| velo::normalize(&d.path).starts_with(p.as_str()))).collect();
            list.sort_by_key(|d| std::cmp::Reverse(d.change().abs()));
            list.truncate(1000);
            egui::ScrollArea::vertical().id_salt("hist_files").auto_shrink([false, false]).show_rows(ui, 24., list.len(), |ui, range| {
                for d in &list[range] {
                    ui.horizontal(|ui| {
                        let c = d.change();
                        let what = match (d.old, d.new) {
                            (None, Some(_)) => "entrou",
                            (Some(_), None) => "saiu",
                            _ if c > 0 => "cresceu",
                            _ => "diminuiu",
                        };
                        ui.label(RichText::new(format!("{}{}", if c >= 0 { "+" } else { "−" }, bytes(c.unsigned_abs()))).monospace().color(if c >= 0 { c_text() } else { c_dim() }));
                        ui.label(RichText::new(what).small().color(c_mute()));
                        ui.label(RichText::new(fmt_date(d.at)).small().monospace().color(c_mute()));
                        let r = ui.add(egui::Label::new(RichText::new(&*d.path).color(c_dim())).truncate().sense(Sense::click()));
                        if r.double_clicked() {
                            reveal(Path::new(&*d.path));
                        }
                    });
                }
            });
            return;
        }
        let key = folder.clone().unwrap_or_default();
        if self.space.hist.grouped.as_ref().is_none_or(|g| g.0 != key) {
            let rows = match &folder {
                Some(f) => history::by_folder(&deltas, f),
                None => {
                    let mut m: BTreeMap<String, (i64, usize)> = BTreeMap::new();
                    for d in deltas.iter() {
                        let e = m.entry(format!("{}\\", drive_of(Path::new(&*d.path)))).or_default();
                        e.0 += d.change();
                        e.1 += 1;
                    }
                    m.into_iter().map(|(k, (c, n))| (k, c, n)).collect()
                }
            };
            self.space.hist.grouped = Some((key, rows));
        }
        let rows = self.space.hist.grouped.as_ref().unwrap().1.clone();
        let mut enter = None;
        egui::ScrollArea::vertical().id_salt("hist_rows").auto_shrink([false, false]).show_rows(ui, 28., rows.len(), |ui, range| {
            for (name, change, n) in &rows[range] {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.), Sense::click());
                let dir = name.ends_with('\\');
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4, c_hover());
                }
                let sign = if *change >= 0 { "+" } else { "−" };
                ui.painter().text(rect.left_center() + vec2(8., 0.), Align2::LEFT_CENTER, format!("{sign}{}", bytes(change.unsigned_abs())), FontId::monospace(12.), if *change >= 0 { c_text() } else { c_dim() });
                let g = clipped(ui, name, FontId::proportional(14.), if dir { c_text() } else { c_dim() }, rect.width() - 220.);
                ui.painter().galley(egui::pos2(rect.left() + 110., rect.center().y - g.size().y / 2.), g, c_text());
                ui.painter().text(rect.right_center() - vec2(8., 0.), Align2::RIGHT_CENTER, format!("{n} mudanças"), FontId::proportional(11.), c_mute());
                if resp.clicked() && dir {
                    let base = folder.clone().map_or_else(|| PathBuf::from(name.as_str()), |f| f.join(name.trim_end_matches('\\')));
                    enter = Some(base);
                }
            }
        });
        if let Some(p) = enter {
            self.space.hist.folder = Some(p);
        }
    }
    fn load_history(&mut self, ctx: &egui::Context, from: String, to: String) {
        let Some(dir) = self.space.log.as_ref().map(|l| l.dir.clone()) else { return };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let log = history::Log { dir };
            let (a, b) = (history::day_start(&from).unwrap_or(0), history::day_start(&to).unwrap_or(0));
            let mut all = Vec::new();
            let mut t = a;
            while t <= b {
                all.extend(log.day(&history::day_name(t + 3600)));
                t += 86_400;
            }
            let label = if from == to { format!("Dia {from}") } else { format!("De {from} até {to}") };
            let _ = tx.send((label, all));
            ctx.request_repaint();
        });
        self.space.hist.rx = Some(rx);
        self.space.hist.folder = None;
    }

    // ---- Jogos ----
    fn page_games(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.space.games.is_none() && self.space.games_rx.is_none() {
            self.load_games(ctx);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Steam e Epic").color(c_dim()));
            if ui.add_enabled(self.space.games_rx.is_none(), flat("Atualizar lista")).clicked() {
                self.space.games = None;
                self.load_games(ctx);
            }
        });
        let Some(games) = self.space.games.clone() else {
            ui.label(RichText::new("Procurando jogos…").color(c_mute()));
            return;
        };
        if games.is_empty() {
            ui.label(RichText::new("Nenhum jogo da Steam ou da Epic encontrado.").color(c_mute()));
            return;
        }
        let mut rows: Vec<(Game, u64, Option<PathBuf>)> = games
            .into_iter()
            .map(|g| {
                let size = self.folder_size(&g.dir);
                let moved = std::fs::read_link(&g.dir).ok();
                (g, size, moved)
            })
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        let mut ask = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("games").num_columns(6).spacing(vec2(18., 10.)).striped(false).show(ui, |ui| {
                for h in ["Jogo", "Loja", "Disco", "Tamanho", "Jogado pela última vez", ""] {
                    ui.label(RichText::new(h).small().color(c_mute()));
                }
                ui.end_row();
                for (g, size, moved) in &rows {
                    ui.label(RichText::new(&g.name).color(c_text()));
                    ui.label(RichText::new(g.launcher).color(c_dim()));
                    ui.label(RichText::new(moved.as_ref().map_or(drive_of(&g.dir), |m| format!("{} → {}", drive_of(&g.dir), drive_of(m)))).monospace().color(c_dim()));
                    ui.label(RichText::new(if *size > 0 { bytes(*size) } else { "—".into() }).monospace().color(c_text()));
                    ui.label(RichText::new(if g.last_played > 0 { fmt_date(g.last_played) } else { "não se sabe".into() }).color(c_dim()))
                        .on_hover_text("Steam: data que ela mesma guarda. Epic: pelo Prefetch do Windows (precisa de administrador).");
                    if moved.is_none() {
                        if ui.add_enabled(self.space.job.is_none(), flat("Mover…")).clicked() {
                            ask = Some((g.dir.clone(), g.exe.clone()));
                        }
                    } else {
                        ui.label(RichText::new("movido").small().color(c_mute()));
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((dir, exe)) = ask {
            self.ask_move(dir, exe);
        }
    }

    // ---- Registro ----
    fn page_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.space.actions.list.is_empty() {
            ui.label(RichText::new("Nada foi apagado ou movido pelo Seek ainda.").color(c_mute()));
            return;
        }
        let mut undo = None;
        let list = self.space.actions.list.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 30., list.len(), |ui, range| {
            for i in range.map(|r| list.len() - 1 - r) {
                let a = &list[i];
                ui.horizontal(|ui| {
                    ui.label(RichText::new(fmt_date(a.at)).monospace().small().color(c_mute()));
                    ui.label(RichText::new(match a.kind { cleanup::Kind::Recycle => "Lixeira", cleanup::Kind::Junction => "Movido" }).color(c_dim()));
                    ui.label(RichText::new(bytes(a.bytes)).monospace().color(c_text()));
                    let text = match a.kind {
                        cleanup::Kind::Recycle => a.from.display().to_string(),
                        cleanup::Kind::Junction => format!("{} → {}", a.from.display(), a.to.display()),
                    };
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if a.undone {
                            ui.label(RichText::new("desfeito").small().color(c_mute()));
                        } else if ui.add_enabled(self.space.job.is_none(), flat("Desfazer")).clicked() {
                            undo = Some(i);
                        }
                        ui.add(egui::Label::new(RichText::new(text).color(c_dim())).truncate());
                    });
                });
            }
        });
        if let Some(i) = undo {
            let a = list[i].clone();
            match a.kind {
                cleanup::Kind::Recycle => self.start_job("Restaurando da Lixeira".into(), 0, move |_, _| JobDone::Undone(i, cleanup::restore(&a.from)), ctx),
                cleanup::Kind::Junction => {
                    let total = a.bytes;
                    self.start_job(format!("Trazendo {} de volta", a.from.display()), total, move |d, c| JobDone::Undone(i, cleanup::undo_junction(&a.from, &a.to, d, c)), ctx)
                }
            }
        }
    }

    // ---- janelas ----
    fn review_window(&mut self, ctx: &egui::Context) {
        let Some(review) = self.space.review.as_mut() else { return };
        let mut open = true;
        let mut send = false;
        let (n, size) = (review.checked.len(), review.checked.iter().map(|&i| review.items[i].1.bytes).sum::<u64>());
        egui::Window::new(review.title.clone())
            .id(egui::Id::new("space_review"))
            .open(&mut open)
            .default_size(vec2(920., 560.))
            .anchor(Align2::CENTER_CENTER, vec2(0., 0.))
            .show(ctx, |ui| {
                // a caixa de marcar precisa de borda para aparecer no tema escuro
                let v = ui.visuals_mut();
                v.widgets.inactive.bg_stroke = Stroke::new(1_f32, c_dim());
                v.widgets.hovered.bg_stroke = Stroke::new(1_f32, c_text());
                if review.confirm {
                    ui.label(RichText::new(format!("Vai para a Lixeira: {n} itens, {}", bytes(size))).size(16.).color(c_text()));
                    ui.label(RichText::new("O espaço só volta quando a Lixeira for esvaziada. Até lá, dá para desfazer no Registro.").color(c_dim()));
                    ui.add_space(6.);
                    let mut list: Vec<usize> = review.checked.iter().copied().collect();
                    list.sort_unstable();
                    egui::ScrollArea::vertical().max_height(320.).show_rows(ui, 20., list.len(), |ui, range| {
                        for &i in &list[range] {
                            let it = &review.items[i].1;
                            ui.label(RichText::new(format!("{}  ·  {}", it.path.display(), bytes(it.bytes))).monospace().small().color(c_dim()));
                        }
                    });
                    ui.add_space(10.);
                    ui.horizontal(|ui| {
                        if ui.add(flat("Voltar")).clicked() {
                            review.confirm = false;
                        }
                        if ui.add(primary("Mandar para a Lixeira")).clicked() {
                            send = true;
                        }
                    });
                    return;
                }
                ui.horizontal(|ui| {
                    if ui.add(flat("Marcar todos")).clicked() {
                        review.checked = (0..review.items.len()).filter(|&i| review.items[i].2.is_none()).collect();
                    }
                    if ui.add(flat("Desmarcar todos")).clicked() {
                        review.checked.clear();
                    }
                });
                // linhas: cabeçalho de grupo (None) ou item (Some)
                let mut rows: Vec<Option<usize>> = Vec::new();
                for (i, (g, _, _)) in review.items.iter().enumerate() {
                    if i == 0 || review.items[i - 1].0 != *g {
                        rows.push(None);
                    }
                    rows.push(Some(i));
                }
                let group_at = |row: usize, rows: &[Option<usize>]| rows[row + 1..].iter().find_map(|r| *r);
                let mut toggle_group: Option<String> = None;
                egui::ScrollArea::vertical().max_height(ui.available_height() - 44.).auto_shrink([false, false]).show_rows(ui, 40., rows.len(), |ui, range| {
                    for row in range {
                        match rows[row] {
                            None => {
                                let Some(first) = group_at(row, &rows) else { continue };
                                let g = review.items[first].0.clone();
                                let members: Vec<usize> = (0..review.items.len()).filter(|&i| review.items[i].0 == g).collect();
                                let total: u64 = members.iter().map(|&i| review.items[i].1.bytes).sum();
                                ui.allocate_ui(vec2(ui.available_width(), 40.), |ui| {
                                    ui.horizontal_centered(|ui| {
                                        ui.label(RichText::new(&g).color(c_text()).strong());
                                        ui.label(RichText::new(format!("{} itens · {}", members.len(), bytes(total))).small().color(c_mute()));
                                        if ui.add(flat("Marcar este grupo")).clicked() {
                                            toggle_group = Some(g.clone());
                                        }
                                    });
                                });
                            }
                            Some(i) => {
                                let (_, it, blocked) = &review.items[i];
                                ui.allocate_ui(vec2(ui.available_width(), 40.), |ui| {
                                    ui.horizontal_centered(|ui| {
                                        let mut on = review.checked.contains(&i);
                                        if ui.add_enabled(blocked.is_none(), egui::Checkbox::without_text(&mut on)).changed() {
                                            if on {
                                                review.checked.insert(i);
                                            } else {
                                                review.checked.remove(&i);
                                            }
                                        }
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 1.;
                                            let name = it.path.file_name().map_or(it.path.display().to_string(), |n| n.to_string_lossy().into_owned());
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new(name).color(c_text()));
                                                ui.label(RichText::new(bytes(it.bytes)).monospace().color(c_text()));
                                                ui.label(RichText::new(if it.date > 0 { format!("mudou {}", fmt_date(it.date)) } else { String::new() }).small().color(c_mute()));
                                                ui.label(RichText::new(match blocked { Some(b) => format!("bloqueado: {b}"), None => format!("{} · {}", it.what, it.why) }).small().color(c_dim()));
                                            });
                                            let r = ui.add(egui::Label::new(RichText::new(it.path.display().to_string()).monospace().small().color(c_mute())).truncate().sense(Sense::click()));
                                            if r.on_hover_text("Clique para mostrar no Explorer").clicked() {
                                                reveal(&it.path);
                                            }
                                        });
                                    });
                                });
                            }
                        }
                    }
                });
                if let Some(g) = toggle_group {
                    for i in 0..review.items.len() {
                        if review.items[i].0 == g && review.items[i].2.is_none() {
                            review.checked.insert(i);
                        }
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{n} marcados · {}", bytes(size))).color(c_text()));
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(n > 0, primary("Mandar para a Lixeira…")).clicked() {
                            review.confirm = true;
                        }
                    });
                });
            });
        if send {
            let review = self.space.review.take().unwrap();
            let list: Vec<(PathBuf, u64)> = review.checked.iter().map(|&i| (review.items[i].1.path.clone(), review.items[i].1.bytes)).collect();
            let total = list.iter().map(|x| x.1).sum();
            self.start_job(
                format!("Mandando {} itens para a Lixeira", list.len()),
                total,
                move |done, cancel| {
                    let mut out = Vec::new();
                    for (p, b) in list {
                        if cancel.load(Ordering::Relaxed) {
                            out.push((p, b, Err("cancelado".into())));
                            continue;
                        }
                        let r = cleanup::to_recycle_bin(&p);
                        done.fetch_add(b, Ordering::Relaxed);
                        out.push((p, b, r));
                    }
                    JobDone::Recycled(out)
                },
                ctx,
            );
        } else if !open {
            self.space.review = None;
        }
    }
    fn move_window(&mut self, ctx: &egui::Context) {
        let Some(m) = self.space.moving.as_mut() else { return };
        let mut open = true;
        let mut go = false;
        let from = drive_of(&m.src);
        let drives = self.space.drives.clone();
        egui::Window::new("Mover para outro disco")
            .id(egui::Id::new("space_move"))
            .open(&mut open)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0., 0.))
            .show(ctx, |ui| {
                ui.set_width(460.);
                ui.label(RichText::new(format!("{}  ·  {}", m.name, bytes(m.bytes))).size(16.).color(c_text()));
                ui.label(RichText::new(m.src.display().to_string()).monospace().small().color(c_mute()));
                ui.add_space(6.);
                ui.label(RichText::new("No lugar antigo fica um atalho (junction): programas e jogos continuam achando tudo no mesmo caminho.").color(c_dim()));
                if let Some(why) = knowledge::protected(&m.src, true) {
                    ui.label(RichText::new(format!("Bloqueado: {why}")).color(c_text()));
                    return;
                }
                ui.add_space(8.);
                for (d, _, free) in drives.iter().filter(|d| drive_of(&d.0) != from) {
                    let fits = *free > m.bytes + (1 << 30);
                    let label = format!("{}  {} livres{}", d.display(), bytes(*free), if fits { "" } else { " (não cabe)" });
                    if ui.add_enabled(fits, flat(&label).selected(m.target.as_ref() == Some(d))).clicked() {
                        m.target = Some(d.clone());
                    }
                }
                if let Some(t) = &m.target {
                    ui.add_space(6.);
                    ui.label(RichText::new(format!("Vai para {}", t.join("Seek - Movidos").join(&m.name).display())).monospace().small().color(c_dim()));
                    ui.label(RichText::new("Feche o programa ou jogo (e a Steam ou a Epic) antes de mover.").color(c_dim()));
                    ui.add_space(6.);
                    if ui.add(primary("Mover")).clicked() {
                        go = true;
                    }
                }
            });
        if go {
            let m = self.space.moving.take().unwrap();
            // jogo: o launcher e o jogo precisam estar fechados; outra pasta: nada a conferir aqui
            let game = !m.exe.is_empty() || knowledge::explain(&m.src).is_some_and(|r| r.seal == Seal::Move);
            let names = if game { vec!["steam.exe", "EpicGamesLauncher.exe", m.exe.as_str()] } else { Vec::new() };
            if let Some(p) = velo::games::running(&names) {
                self.space.notice = format!("Feche {p} antes de mover.");
                self.space.moving = Some(m);
                return;
            }
            let dest = m.target.clone().unwrap().join("Seek - Movidos");
            let (src, size) = (m.src.clone(), m.bytes);
            self.start_job(format!("Movendo {}", m.name), size, move |d, c| JobDone::Moved(src.clone(), size, cleanup::move_with_junction(&src, &dest, d, c)), ctx);
        } else if !open {
            self.space.moving = None;
        }
    }
}
fn group_by_what(items: &[Item]) -> Vec<(String, Vec<Item>)> {
    let mut map: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    for it in items {
        map.entry(it.what.clone()).or_default().push(it.clone());
    }
    let mut v: Vec<(String, Vec<Item>)> = map.into_iter().collect();
    v.sort_by_key(|(_, l)| std::cmp::Reverse(l.iter().map(|i| i.bytes).sum::<u64>()));
    v
}
fn group_dups(items: &[Item]) -> Vec<(String, Vec<Item>)> {
    let mut map: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    for it in items {
        map.entry(it.why.clone()).or_default().push(it.clone());
    }
    let mut v: Vec<(String, Vec<Item>)> = map.into_iter().collect();
    v.sort_by_key(|(_, l)| std::cmp::Reverse(l.iter().map(|i| i.bytes).sum::<u64>()));
    v
}
