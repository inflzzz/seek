// Interface da aplicação. O design pode ser alterado aqui sem mudar o motor de busca.
use super::*;
use crate::PreviewData;
use crate::hotkey;
use crate::thumbs::{self, State};
use egui::{Align, Align2, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, vec2};

// Tema: as cores vêm de funções porque mudam quando o usuário troca entre escuro e claro.
static LIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn pick(dark: (u8, u8, u8), light: (u8, u8, u8)) -> Color32 {
    let (r, g, b) = if LIGHT.load(std::sync::atomic::Ordering::Relaxed) { light } else { dark };
    Color32::from_rgb(r, g, b)
}
fn c_bg() -> Color32 { pick((12, 12, 12), (250, 250, 250)) }
fn c_panel() -> Color32 { pick((17, 17, 17), (241, 241, 241)) }
fn c_hover() -> Color32 { pick((24, 24, 24), (233, 233, 233)) }
fn c_sel() -> Color32 { pick((34, 34, 34), (221, 221, 221)) }
fn c_tile() -> Color32 { pick((28, 28, 28), (229, 229, 229)) }
fn c_line() -> Color32 { pick((30, 30, 30), (224, 224, 224)) }
fn c_text() -> Color32 { pick((237, 237, 237), (26, 26, 26)) }
fn c_dim() -> Color32 { pick((154, 154, 154), (96, 96, 96)) }
fn c_mute() -> Color32 { pick((102, 102, 102), (140, 140, 140)) }
fn c_border() -> Color32 { pick((42, 42, 42), (205, 205, 205)) }
fn c_hover2() -> Color32 { pick((48, 48, 48), (210, 210, 210)) }
fn c_mark_bg() -> Color32 { pick((70, 70, 70), (255, 226, 130)) }
fn c_mark_fg() -> Color32 { pick((255, 255, 255), (20, 20, 20)) }
fn theme_file() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("tema.txt"))
}
pub fn is_light() -> bool {
    LIGHT.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn set_light(ctx: &egui::Context, light: bool) {
    LIGHT.store(light, std::sync::atomic::Ordering::Relaxed);
    if let Some(f) = theme_file() {
        let _ = std::fs::write(f, if light { "claro" } else { "escuro" });
    }
    apply_palette(ctx);
}

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    List,
    Small,
    Medium,
    Large,
    Compact, // modo simples: só busca e lista, estilo Everything
}
impl View {
    const ALL: [Self; 4] = [Self::List, Self::Small, Self::Medium, Self::Large];
    fn label(self) -> &'static str {
        match self {
            Self::List => "Lista",
            Self::Small => "Pequena",
            Self::Medium => "Média",
            Self::Large => "Grande",
            Self::Compact => "Simples",
        }
    }
    // (lado da miniatura, altura da legenda)
    fn tile(self) -> (f32, f32) {
        match self {
            Self::List => (36., 0.),
            Self::Small => (84., 0.),
            Self::Medium => (150., 22.),
            Self::Large => (240., 40.),
            Self::Compact => (0., 0.),
        }
    }
    fn step(self, bigger: bool) -> Self {
        if self == Self::Compact {
            return self;
        }
        let i = Self::ALL.iter().position(|v| *v == self).unwrap();
        Self::ALL[if bigger { (i + 1).min(3) } else { i.saturating_sub(1) }]
    }
}

fn apply_palette(ctx: &egui::Context) {
    let mut v = if is_light() { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.panel_fill = c_bg();
    v.window_fill = c_panel();
    v.extreme_bg_color = c_bg();
    v.faint_bg_color = c_panel();
    v.window_stroke = Stroke::new(1_f32, c_border());
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(6);
    v.selection.bg_fill = c_sel();
    v.selection.stroke = Stroke::new(1_f32, c_text());
    v.widgets.noninteractive.bg_stroke = Stroke::new(1_f32, c_line());
    v.widgets.noninteractive.fg_stroke = Stroke::new(1_f32, c_dim());
    for (w, fg, bg) in [
        (&mut v.widgets.inactive, c_dim(), c_bg()),
        (&mut v.widgets.hovered, c_text(), c_hover()),
        (&mut v.widgets.active, c_text(), c_sel()),
    ] {
        w.fg_stroke = Stroke::new(1_f32, fg);
        w.bg_fill = bg;
        w.weak_bg_fill = bg;
        w.bg_stroke = Stroke::NONE;
        w.corner_radius = CornerRadius::same(4);
    }
    ctx.set_visuals(v);
}
pub fn theme(ctx: &egui::Context) {
    if let Some(Ok(saved)) = theme_file().map(std::fs::read_to_string) {
        LIGHT.store(saved.trim() == "claro", std::sync::atomic::Ordering::Relaxed);
    }
    // Segoe UI do Windows, se existir; senão fica a fonte embutida do egui.
    if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("segoe".into(), std::sync::Arc::new(egui::FontData::from_owned(data)));
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "segoe".into());
        ctx.set_fonts(fonts);
    }
    apply_palette(ctx);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(10., 8.);
        s.spacing.button_padding = vec2(8., 4.);
        s.interaction.selectable_labels = false;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(13.));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.5));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(12.));
    });
}

fn bytes(n: u64) -> String {
    if n >= 1_073_741_824 {
        format!("{:.1} GB", n as f64 / 1_073_741_824.)
    } else if n >= 1_048_576 {
        format!("{:.1} MB", n as f64 / 1_048_576.)
    } else if n >= 1024 {
        format!("{:.0} KB", n as f64 / 1024.)
    } else {
        format!("{n} B")
    }
}
fn short_count(n: usize) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{} mil", n / 1000),
        _ => format!("{:.1} mi", n as f64 / 1e6).replace('.', ","),
    }
}
// Discos locais (letra, total, livre) via Windows; ignora leitores sem mídia.
fn list_drives() -> Vec<(PathBuf, u64, u64)> {
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        let (mut free, mut total, mut _all) = (0u64, 0u64, 0u64);
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut _all) };
        if ok != 0 && total > 0 {
            out.push((PathBuf::from(root), total, free));
        }
    }
    out
}
// Itens "buscar semelhantes" de um arquivo; devolve (modo) escolhido: 0 nome, 1 foto, 2 ícone.
fn similar_menu(ui: &mut egui::Ui, e: &Entry) -> Option<u8> {
    let mut pick = None;
    if ui.button("Nome parecido").clicked() {
        pick = Some(0);
    }
    if thumbs::supported(e.ext()) && ui.button("Foto parecida").clicked() {
        pick = Some(1);
    }
    if ui.button("Ícone parecido").clicked() {
        pick = Some(2);
    }
    if pick.is_some() {
        ui.close();
    }
    pick
}
// ---- datas ----
fn fmt_date(secs: u64) -> String {
    velo::clock::format_local(secs)
}
fn sort_label(k: velo::SortKey) -> &'static str {
    match k {
        velo::SortKey::Path => "Caminho",
        velo::SortKey::Name => "Nome",
        velo::SortKey::Size => "Tamanho",
        velo::SortKey::Date => "Modificado",
        velo::SortKey::Kind => "Tipo",
    }
}
// ---- destaque do trecho achado ----
fn match_ranges(name: &str, terms: &[String]) -> Vec<(usize, usize)> {
    if terms.is_empty() {
        return Vec::new();
    }
    // texto normalizado + de onde cada byte dele veio no nome original (acentos e maiúsculas mudam o tamanho)
    let (mut norm, mut origin) = (String::with_capacity(name.len()), Vec::with_capacity(name.len()));
    for (i, c) in name.char_indices() {
        let n = velo::normalize(c.encode_utf8(&mut [0; 4]));
        origin.extend(std::iter::repeat_n(i, n.len()));
        norm.push_str(&n);
    }
    let mut found: Vec<(usize, usize)> = Vec::new();
    for t in terms.iter().filter(|t| !t.is_empty()) {
        let mut from = 0;
        while let Some(p) = norm[from..].find(t.as_str()) {
            let (s, e) = (from + p, from + p + t.len());
            let last = origin[e - 1];
            found.push((origin[s], last + name[last..].chars().next().map_or(1, char::len_utf8)));
            from = e;
        }
    }
    found.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in found {
        match merged.last_mut() {
            Some(l) if s <= l.1 => l.1 = l.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}
/// Nome em uma linha, com os trechos achados em branco sobre fundo cinza.
fn name_galley(ui: &egui::Ui, name: &str, terms: &[String], font: FontId, color: Color32, w: f32) -> std::sync::Arc<egui::Galley> {
    let ranges = match_ranges(name, terms);
    if ranges.is_empty() {
        return clipped(ui, name, font, color, w);
    }
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = w.max(10.);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let plain = egui::TextFormat { font_id: font.clone(), color, ..Default::default() };
    let marked = egui::TextFormat { font_id: font, color: c_mark_fg(), background: c_mark_bg(), ..Default::default() };
    let mut pos = 0;
    for (s, e) in ranges {
        if s > pos {
            job.append(&name[pos..s], 0., plain.clone());
        }
        job.append(&name[s..e], 0., marked.clone());
        pos = e;
    }
    if pos < name.len() {
        job.append(&name[pos..], 0., plain);
    }
    ui.painter().layout_job(job)
}
// O modo simples é lembrado por um arquivo vazio ao lado do executável.
fn mode_file() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("modo-simples"))
}
pub fn saved_compact() -> bool {
    mode_file().is_some_and(|f| f.exists())
}
fn save_compact(on: bool) {
    if let Some(f) = mode_file() {
        let _ = if on { std::fs::write(f, b"") } else { std::fs::remove_file(f) };
    }
}
// Reabre o programa pedindo permissão de administrador (janela de confirmação do Windows).
fn relaunch_as_admin(root: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(Some(0)).collect() };
    let Ok(exe) = std::env::current_exe() else { return };
    // Uma barra no fim ("D:\") escaparia a aspa final; dobrar mantém o caminho certo.
    let mut arg = root.display().to_string();
    if arg.ends_with('\\') {
        arg.push('\\');
    }
    let params = wide(std::ffi::OsStr::new(&format!("\"{arg}\" --relaunch")));
    let verb = wide(std::ffi::OsStr::new("runas"));
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), wide(exe.as_os_str()).as_ptr(), params.as_ptr(), std::ptr::null(), 1);
    }
}
fn open_file(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}
fn reveal(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}
// Texto de uma linha que termina em "…" quando não cabe em `w`.
fn clipped(ui: &egui::Ui, text: &str, font: FontId, color: Color32, w: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = w.max(10.);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}
fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(c_bg()).strong())
        .fill(c_text())
        .corner_radius(4)
}
fn flat(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned())).frame(false)
}
fn check(painter: &egui::Painter, at: Pos2, boxed: bool) {
    let mut color = c_text();
    if boxed {
        painter.rect_filled(Rect::from_min_size(at, vec2(16., 16.)), 3, c_text());
        color = c_bg();
    }
    let p = |x: f32, y: f32| at + vec2(x, y);
    painter.line_segment([p(4., 8.5), p(7., 11.5)], Stroke::new(1.8_f32, color));
    painter.line_segment([p(7., 11.5), p(12., 5.)], Stroke::new(1.8_f32, color));
}
fn draw_tile(painter: &egui::Painter, rect: Rect, e: &Entry, state: Option<&State>) {
    painter.rect_filled(rect, 3, c_tile());
    if let Some(State::Ready(tex)) = state {
        let [w, h] = tex.size();
        let (w, h) = (w as f32, h as f32);
        let uv = if w > h {
            let u = (1. - h / w) / 2.;
            Rect::from_min_max(Pos2::new(u, 0.), Pos2::new(1. - u, 1.))
        } else {
            let v = (1. - w / h) / 2.;
            Rect::from_min_max(Pos2::new(0., v), Pos2::new(1., 1. - v))
        };
        painter.image(tex.id(), rect, uv, Color32::WHITE);
    } else if let Some(State::Fit(tex)) = state {
        // documento ou vídeo: cabe inteiro no bloco, centralizado
        let [w, h] = tex.size();
        let scale = ((rect.width() - 8.) / w as f32).min((rect.height() - 8.) / h as f32).min(4.);
        let shown = Rect::from_center_size(rect.center(), vec2(w as f32 * scale, h as f32 * scale));
        painter.image(tex.id(), shown, Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)), Color32::WHITE);
    } else if let Some(State::Icon(tex)) = state {
        let side = (rect.width() * 0.6).clamp(20., 96.);
        let icon = Rect::from_center_size(rect.center(), vec2(side, side));
        painter.image(
            tex.id(),
            icon,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
    } else {
        let label: String = if e.ext().is_empty() {
            "—".into()
        } else {
            e.ext().chars().take(4).collect::<String>().to_uppercase()
        };
        let size = (rect.height() * 0.2).clamp(9., 20.);
        painter.text(rect.center(), Align2::CENTER_CENTER, label, FontId::monospace(size), c_mute());
    }
}
fn cat_row(ui: &mut egui::Ui, label: &str, count: usize, active: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.), Sense::click());
    if active || resp.hovered() {
        ui.painter()
            .rect_filled(rect, 4, if active { c_sel() } else { c_hover() });
    }
    let p = ui.painter();
    p.text(
        rect.left_center() + vec2(10., 0.),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.),
        if active { c_text() } else { c_dim() },
    );
    p.text(
        rect.right_center() - vec2(10., 0.),
        Align2::RIGHT_CENTER,
        short_count(count),
        FontId::monospace(11.5),
        c_mute(),
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

impl App {
    // Janelinha de busca (atalho global): vista simples, janela pequena no alto da tela, busca vazia.
    fn enter_quick(&mut self, ctx: &egui::Context) {
        if !self.quick {
            let size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
            self.quick_prev = Some((self.view, size));
        }
        self.quick = true;
        self.quick_focus_seen = false;
        self.quick_lost_focus = None;
        self.view = View::Compact;
        self.query.clear();
        self.refresh_search();
        self.focus_search = true;
        // antes de desenhar o campo: texto digitado logo após o atalho já entra nele
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("busca")));
        let (w, h) = (780., 470.);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(w, h)));
        if let Some(m) = ctx.input(|i| i.viewport().monitor_size) {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new((m.x - w) / 2., m.y * 0.16)));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
    fn leave_quick(&mut self, ctx: &egui::Context) {
        self.quick = false;
        if let Some((view, size)) = self.quick_prev.take() {
            self.view = view;
            if let Some(s) = size {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(s));
            }
        }
        crate::tray::hide_window();
    }
    // Recalcula as contagens da lateral depois que o índice mudou ao vivo.
    fn recount(&mut self) {
        self.kind_counts = [0; Kind::ALL.len()];
        let mut exts: std::collections::HashMap<&str, usize> = Default::default();
        for e in self.entries.iter() {
            if let Some(k) = Kind::ALL.iter().position(|k| *k == e.kind) {
                self.kind_counts[k] += 1;
            }
            if !e.ext().is_empty() {
                *exts.entry(e.ext()).or_default() += 1;
            }
        }
        let mut list: Vec<_> = exts.into_iter().map(|(e, n)| (e.to_owned(), Kind::for_ext(e), n)).collect();
        list.sort_unstable_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        self.ext_counts = list;
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if let Some(catalog) = self.icon_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.icon_status = format!(
                "{} {} verificados em {:.1}s · {} indisponíveis · {}",
                catalog.signatures.len(),
                if self.visual_photo { "arquivos" } else { "ícones" },
                catalog.elapsed.as_secs_f64(),
                catalog.errors,
                if catalog.cancelled {
                    "cancelado: resultados parciais"
                } else if catalog.cached {
                    "catálogo salvo carregado"
                } else if catalog.saved {
                    "catálogo salvo"
                } else {
                    "catálogo apenas nesta sessão"
                }
            );
            self.icon_catalog_complete = !catalog.cancelled;
            self.icon_catalog = Some(catalog.signatures);
            self.icon_rx = None;
            self.refresh_search();
        }
        if let Some(result) = self.move_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.move_rx = None;
            self.undo = result.undo;
            self.notice = result.message;
            self.start_scan(ctx, result.root, false);
        }
        if let Some(scan) = self.scan_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            if !scan.cached && !scan.cancelled
                && let Some(root) = &self.root {
                    velo::icon_search::invalidate(root);
                }
            self.status = if scan.cached {
                format!(
                    "{} arquivos · índice salvo carregado em {:.2}s · use Atualizar índice se o HD mudou",
                    scan.entries.len(),
                    scan.elapsed.as_secs_f64()
                )
            } else {
                format!(
                    "{} arquivos · leitura {}{:.2}s · {} inacessíveis · {}",
                    scan.entries.len(),
                    if scan.turbo { "turbo " } else { "" },
                    scan.elapsed.as_secs_f64(),
                    scan.errors,
                    if scan.cancelled {
                        "cancelado: índice parcial, não salvo"
                    } else if scan.cache_saved {
                        "índice salvo"
                    } else {
                        "índice apenas em memória: não foi possível salvar"
                    }
                )
            };
            self.kind_counts = [0; Kind::ALL.len()];
            let mut exts: std::collections::HashMap<&str, usize> = Default::default();
            for e in scan.entries.iter() {
                if let Some(k) = Kind::ALL.iter().position(|k| *k == e.kind) {
                    self.kind_counts[k] += 1;
                }
                if !e.ext().is_empty() {
                    *exts.entry(e.ext()).or_default() += 1;
                }
            }
            let mut list: Vec<_> = exts
                .into_iter()
                .map(|(e, n)| (e.to_owned(), Kind::for_ext(e), n))
                .collect();
            list.sort_unstable_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
            self.ext_counts = list;
            self.admin_hint = !scan.cached
                && !scan.turbo
                && !scan.cancelled
                && self.root.as_deref().is_some_and(|r| velo::mft::drive_letter(r).is_some() || velo::is_all(r))
                && !velo::mft::can_read_volumes();
            velo::log::line(&format!("índice pronto: {} arquivos", scan.entries.len()));
            self.entries = scan.entries;
            if let Some(root) = &self.root {
                crate::tray::remember(root, &self.entries);
            }
            crate::tray::DIRTY.store(false, Ordering::SeqCst);
            self.scan_rx = None;
            self.watchers = match (&self.root, scan.cancelled) {
                (_, true) | (None, _) => Vec::new(),
                (Some(r), _) if velo::is_all(r) => velo::fixed_drives().iter().filter_map(|d| velo::watch::start(d, self.live.clone())).collect(),
                (Some(r), _) => velo::watch::start(r, self.live.clone()).into_iter().collect(),
            };
            *self.live.lock().unwrap() = Default::default();
            self.live_overflow = false;
            self.refresh_search();
        }
        while let Ok(answer) = self.answers.try_recv() {
            if answer.generation == self.generation {
                self.results = answer.indices;
                self.elapsed = answer.elapsed;
                self.searching = false;
            }
        }
        if let Some(result) = self.update_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.update_rx = None;
            if let Ok(Some(version)) = result {
                self.update_notice = format!("Seek {version} instalado");
            }
        }
        if let Some(groups) = self.dup_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.dup_rx = None;
            self.apply_duplicates(groups);
        }
        if self.dup_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        self.thumbs.poll(ctx);
        let scanning = self.scan_rx.is_some();
        let busy = self.move_rx.is_some();
        let icon_scanning = self.icon_rx.is_some();
        if !scanning && !busy && self.root.is_some() {
            let dropped = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
            if let Some(path) = dropped {
                self.choose_icon(ctx, path);
            }
        }
        if scanning || icon_scanning {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
        // Mudanças ao vivo: junta os avisos do Windows e aplica em lote quando o usuário parou de mexer.
        let now = ctx.input(|i| i.time);
        if ctx.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
            self.last_input = now;
        }
        let (pending_len, overflow) = {
            let p = self.live.lock().unwrap();
            (p.len(), p.overflow)
        };
        if overflow {
            // O Windows (ou o limite de memória) fez perder avisos: o índice pode estar desatualizado.
            self.live.lock().unwrap().overflow = false;
            let can_rescan = self.root.as_deref().is_some_and(|r| velo::mft::drive_letter(r).is_some() || velo::is_all(r))
                && velo::mft::can_read_volumes();
            if let (true, Some(root), false) = (can_rescan, self.root.clone(), scanning) {
                velo::log::line("avisos de mudança perdidos: lendo o disco de novo");
                self.start_scan(ctx, root, false); // leitura turbo: segundos
            } else {
                self.live_overflow = true;
            }
        }
        if pending_len > 0 {
            ctx.request_repaint_after(Duration::from_millis(500));
            let calm = now - self.last_input > 1.5 || pending_len > 20_000;
            if calm && !scanning && !busy && !icon_scanning && self.icon_reference.is_none() {
                match Arc::try_unwrap(std::mem::take(&mut self.entries)) {
                    Ok(mut v) => {
                        let update = self.live.lock().unwrap().take();
                        let (added, removed) = (update.upserts.len(), update.removed.len());
                        let changed = velo::apply_update(&mut v, update);
                        if changed {
                            velo::log::line(&format!("mudanças ao vivo aplicadas: {added} novos ou alterados, {removed} removidos"));
                        }
                        self.entries = Arc::new(v);
                        if let Some(root) = &self.root {
                            crate::tray::remember(root, &self.entries);
                        }
                        if changed {
                            crate::tray::DIRTY.store(true, Ordering::SeqCst);
                            self.icon_catalog = None; // as assinaturas são por posição no índice
                            self.icon_catalog_complete = false;
                            self.recount();
                            self.thumbs = thumbs::Thumbs::new(ctx);
                            self.refresh_search();
                            self.live_dirty_at = Some(now);
                        }
                    }
                    Err(shared) => self.entries = shared, // uma busca ainda usa o índice: tenta no próximo quadro
                }
            }
        } else if let (Some(t), Some(root)) = (self.live_dirty_at, self.root.clone()) {
            // Um minuto sem novidades: grava o índice atualizado para a próxima abertura.
            if now - t > 60. {
                self.live_dirty_at = None;
                let entries = self.entries.clone();
                thread::spawn(move || {
                    if let Some(file) = velo::cache::location(&root)
                        && velo::cache::save(&file, &root, &entries, 0).is_ok() {
                            crate::tray::DIRTY.store(false, Ordering::SeqCst);
                        }
                });
            } else {
                ctx.request_repaint_after(Duration::from_secs(5));
            }
        }
        if self.viewer.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.close_viewer();
        }
        // Atalho global: abre a janelinha de busca. Esc ou clicar fora a esconde de novo.
        if hotkey::SUMMON.swap(false, Ordering::SeqCst) {
            self.hide_until = None; // o atalho vale mais que o "esconder após iniciar"
            self.enter_quick(ctx);
        }
        if self.quick {
            let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            // foco pelo Windows: o aviso de foco do egui oscila logo após mostrar a janela pelo atalho
            let focused = Some(crate::tray::has_focus());
            if focused == Some(true) {
                self.quick_focus_seen = true;
                self.quick_lost_focus = None;
            } else if self.quick_focus_seen && focused == Some(false) {
                // o egui costuma avisar "sem foco" por um instante logo após o atalho: só vale se durar
                self.quick_lost_focus.get_or_insert_with(Instant::now);
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            let lost = self.quick_lost_focus.is_some_and(|t| t.elapsed() > Duration::from_millis(500));
            if esc || lost {
                self.leave_quick(ctx);
            }
        }
        // Captura do novo atalho (Configurações).
        if self.capturing {
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                    if key == egui::Key::Escape {
                        self.capturing = false;
                        break;
                    }
                    let Some(name) = hotkey::key_name(key) else { continue };
                    if !(modifiers.ctrl || modifiers.alt || modifiers.shift) {
                        self.hotkey_error = "Use Ctrl, Alt ou Shift junto com a tecla.".into();
                        continue;
                    }
                    let spec = format!(
                        "{}{}{}{name}",
                        if modifiers.ctrl { "ctrl+" } else { "" },
                        if modifiers.alt { "alt+" } else { "" },
                        if modifiers.shift { "shift+" } else { "" }
                    );
                    if let Some(h) = self.hotkeys.as_mut() {
                        match h.set(&spec) {
                            Ok(()) => {
                                self.capturing = false;
                                self.hotkey_error.clear();
                            }
                            Err(e) => self.hotkey_error = e,
                        }
                    }
                    break;
                }
            }
        }
        crate::tray::keep_parked();
        if crate::tray::PARKED.load(Ordering::Relaxed) {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        // Início com o Windows (--tray): o eframe mostra a janela depois do 1º quadro; escondemos de novo.
        if self.focus_search {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("busca")));
        }
        if let Some(until) = self.hide_until {
            if Instant::now() < until {
                crate::tray::hide_window();
                ctx.request_repaint_after(Duration::from_millis(80));
            } else {
                self.hide_until = None;
            }
        }
        // Fechar a janela só a esconde na bandeja; "Sair" no menu do ícone encerra de verdade.
        if self.tray_ready && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            crate::tray::hide_window();
        }
        // Teclado na lista: setas (e PageUp/PageDown) movem o cursor; Enter abre; Ctrl+Shift+C copia o caminho.
        {
            let grid = !matches!(self.view, View::List | View::Compact);
            let cols = self.last_cols.max(1) as i64;
            let typing = ctx.wants_keyboard_input();
            let previewing = self.viewer.is_some();
            let mut step = 0i64;
            // Conta cada toque (várias teclas iguais no mesmo quadro valem várias vezes) e as tira dos eventos.
            let take = |i: &mut egui::InputState, key: egui::Key| -> i64 {
                let hit = |e: &egui::Event| matches!(e, egui::Event::Key { key: k, modifiers, .. } if *k == key && modifiers.is_none());
                let n = i.events.iter().filter(|e| hit(e) && matches!(e, egui::Event::Key { pressed: true, .. })).count();
                i.events.retain(|e| !hit(e));
                n as i64
            };
            ctx.input_mut(|i| {
                let unit = if grid { cols } else { 1 };
                step += take(i, egui::Key::ArrowDown) * unit;
                step -= take(i, egui::Key::ArrowUp) * unit;
                step += take(i, egui::Key::PageDown) * 10 * unit;
                step -= take(i, egui::Key::PageUp) * 10 * unit;
                if (grid && !typing) || previewing {
                    step += take(i, egui::Key::ArrowRight);
                    step -= take(i, egui::Key::ArrowLeft);
                }
            });
            if step != 0 && !self.results.is_empty() {
                let last = self.results.len() as i64 - 1;
                self.cursor = (self.cursor as i64 + step).clamp(0, last) as usize;
                self.scroll_to_cursor = true;
                if previewing
                    && let Some(&i) = self.results.get(self.cursor) {
                        self.open_viewer(ctx, i);
                    }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F3) || (!typing && i.key_pressed(egui::Key::Space))) {
                if self.viewer.is_some() {
                    self.close_viewer();
                } else if let Some(&i) = self.results.get(self.cursor) {
                    self.open_viewer(ctx, i);
                }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::C))
                && let Some(&i) = self.results.get(self.cursor) {
                    ctx.copy_text(self.entries[i].path_str().to_owned());
                }
        }
        // Ctrl + scroll troca o tamanho das miniaturas (no máximo um passo a cada 0,25 s).
        let (zoom, now) = ctx.input(|i| (i.zoom_delta(), i.time));
        let last = ctx.data(|d| d.get_temp::<f64>(egui::Id::new("zoom_t")).unwrap_or(0.));
        if zoom != 1. && now - last > 0.25 {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("zoom_t"), now));
            self.view = self.view.step(zoom > 1.);
        }

        let compact = self.view == View::Compact;
        egui::TopBottomPanel::top("top")
            .exact_height(52.)
            .frame(egui::Frame::new().fill(c_bg()).inner_margin(egui::Margin::symmetric(18, 0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("seek").size(15.).color(c_dim()));
                    ui.add_space(14.);
                    let w = (ui.available_width() - 400.).max(200.);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id(egui::Id::new("busca"))
                            .frame(false)
                            .font(FontId::proportional(17.))
                            .desired_width(w)
                            .hint_text("foto praia, .pdf, vídeo maior:1GB"),
                    );
                    let first = ctx.data_mut(|d| d.get_temp::<bool>(egui::Id::new("focus")).is_none());
                    if first {
                        ctx.data_mut(|d| d.insert_temp(egui::Id::new("focus"), true));
                        response.request_focus();
                    }
                    if self.focus_search {
                        self.focus_search = false;
                        response.request_focus();
                    }
                    if response.changed() {
                        self.refresh_search();
                    }
                    if response.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if let Some(&i) = self.results.get(self.cursor) {
                            open_file(self.entries[i].path());
                            if self.quick {
                                self.leave_quick(ctx);
                            }
                        }
                        response.request_focus();
                    }
                    // Sugestões de extensão: ".p" -> .png .pdf .psd ... (as mais frequentes no disco).
                    let hot = ctx.data(|d| d.get_temp::<bool>(egui::Id::new("sug_hot")).unwrap_or(false));
                    let start = self.query.rfind(char::is_whitespace).map_or(0, |i| i + 1);
                    let token = self.query[start..].to_owned();
                    let mut sug: Vec<(String, usize)> = Vec::new();
                    if (response.has_focus() || hot) && token.starts_with('.') && !token[1..].contains(['.', '"']) {
                        let prefix = velo::normalize(&token[1..]);
                        sug = self
                            .ext_counts
                            .iter()
                            .filter(|(e, _, _)| e.starts_with(&prefix) && *e != prefix)
                            .take(8)
                            .map(|(e, _, n)| (e.clone(), *n))
                            .collect();
                    }
                    let mut over = false;
                    if !sug.is_empty() {
                        let area = egui::Area::new(egui::Id::new("sug"))
                            .order(egui::Order::Foreground)
                            .fixed_pos(response.rect.left_bottom() + vec2(0., 8.))
                            .show(ctx, |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    ui.set_width(220.);
                                    ui.spacing_mut().item_spacing.y = 2.;
                                    let mut chosen = None;
                                    for (e, n) in &sug {
                                        if cat_row(ui, &format!(".{e}"), *n, false) {
                                            chosen = Some(e.clone());
                                        }
                                    }
                                    chosen
                                })
                            });
                        over = ctx.pointer_hover_pos().is_some_and(|p| area.response.rect.contains(p));
                        if let Some(e) = area.inner.inner {
                            self.query.truncate(start);
                            self.query.push_str(&format!(".{e}"));
                            self.refresh_search();
                        }
                    }
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("sug_hot"), over));
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(flat(if compact { "Modo completo" } else { "Modo simples" }))
                            .clicked()
                        {
                            self.view = if compact {
                                self.prev_view
                            } else {
                                self.prev_view = self.view;
                                View::Compact
                            };
                            save_compact(!compact);
                        }
                        if ui.add(flat("Configurações")).clicked() {
                            self.settings = true;
                        }
                        if !compact && ui.add_enabled(!self.help, flat("Ajuda")).clicked() {
                            self.help = true;
                        }
                        if !compact && ui
                            .add_enabled(self.root.is_some() && !scanning && !busy, flat("Buscar por ícone"))
                            .clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("Imagem do ícone", &["png", "jpg", "jpeg", "ico", "bmp", "webp"])
                                .pick_file()
                            {
                                self.choose_icon(ctx, path);
                            }
                        if !compact && ui
                            .add_enabled(self.root.is_some() && !scanning && !busy, flat("Atualizar"))
                            .clicked()
                        {
                            self.start_scan(ctx, self.root.clone().unwrap(), false);
                        }
                        if ui.add_enabled(!busy, flat("Escolher disco")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.drive_list = list_drives();
                            self.disks = true;
                        }
                    });
                });
            });
        if self.icon_reference.is_some() {
            egui::TopBottomPanel::top("iconbar")
                .frame(egui::Frame::new().fill(c_panel()).inner_margin(egui::Margin::symmetric(18, 8)))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(texture) = &self.icon_texture {
                            ui.image((texture.id(), vec2(28., 28.)));
                        }
                        ui.label(RichText::new(format!("{}: {}", if self.visual_photo { "Foto parecida com" } else { "Ícone parecido com" }, self.icon_name)).color(c_dim()));
                        ui.label(RichText::new("Tolerância").color(c_mute()));
                        if ui
                            .add(egui::Slider::new(&mut self.icon_tolerance, 0.02..=0.35).fixed_decimals(2))
                            .on_hover_text("Menor: mais parecido. Maior: aceita mais diferenças.")
                            .changed()
                        {
                            self.refresh_search();
                        }
                        if icon_scanning {
                            ui.label(
                                RichText::new(format!(
                                    "verificando {} / {}",
                                    self.icon_progress.load(Ordering::Relaxed),
                                    self.entries.len()
                                ))
                                .monospace()
                                .color(c_mute()),
                            );
                            if ui.add(flat("Cancelar")).clicked() {
                                self.icon_cancel.store(true, Ordering::Relaxed);
                            }
                        } else {
                            ui.label(RichText::new(&self.icon_status).small().color(c_mute()));
                        }
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(flat("Remover")).clicked() {
                                self.clear_icon();
                            }
                        });
                    });
                });
        }
        if let Some(d) = &self.dup {
            let (groups, copies, wasted) = (d.groups, d.copies.len(), d.wasted);
            egui::TopBottomPanel::top("dupbar")
                .frame(egui::Frame::new().fill(c_panel()).inner_margin(egui::Margin::symmetric(18, 8)))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{groups} grupos de duplicados · {copies} cópias · {} podem ser liberados", bytes(wasted)))
                                .color(c_dim()),
                        );
                        if ui.add(flat("Selecionar cópias (mantém uma de cada)")).clicked()
                            && let Some(d) = &self.dup {
                                self.selected = d.copies.iter().copied().collect();
                            }
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(flat("Fechar duplicados")).clicked() {
                                self.refresh_search();
                            }
                        });
                    });
                });
        }
        if let Some(name) = self.similar.clone() {
            egui::TopBottomPanel::top("simbar")
                .frame(egui::Frame::new().fill(c_panel()).inner_margin(egui::Margin::symmetric(18, 8)))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Nome parecido com: {name}")).color(c_dim()));
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(flat("Remover")).clicked() {
                                self.clear_icon();
                            }
                        });
                    });
                });
        }
        if !compact {
        egui::SidePanel::left("filters")
            .exact_width(184.)
            .frame(egui::Frame::new().fill(c_panel()).inner_margin(egui::Margin::symmetric(8, 12)))
            .show(ctx, |ui| {
                if let Some(root) = &self.root {
                    ui.add(
                        egui::Label::new(RichText::new(root.display().to_string()).monospace().small().color(c_dim()))
                            .truncate(),
                    );
                    ui.add_space(6.);
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.;
                    if cat_row(ui, "Tudo", self.entries.len(), self.kind.is_none()) {
                        self.kind = None;
                        self.refresh_search();
                    }
                    for (i, kind) in Kind::ALL.into_iter().enumerate() {
                        if self.kind_counts[i] == 0 && self.kind != Some(kind) {
                            continue;
                        }
                        if cat_row(ui, kind.label(), self.kind_counts[i], self.kind == Some(kind)) {
                            self.kind = Some(kind);
                            self.refresh_search();
                        }
                    }
                    if !self.ext_counts.is_empty() {
                        ui.add_space(14.);
                        ui.horizontal(|ui| {
                            ui.add_space(10.);
                            ui.label(RichText::new("EXTENSÕES").small().color(c_mute()));
                        });
                        let current = self.query.trim().to_owned();
                        let mut pick = None;
                        for (ext, _, n) in self
                            .ext_counts
                            .iter()
                            .filter(|(_, k, _)| self.kind.is_none_or(|sel| sel == *k))
                            .take(300)
                        {
                            let label = format!(".{ext}");
                            if cat_row(ui, &label, *n, current == label) {
                                pick = Some(label);
                            }
                        }
                        if let Some(label) = pick {
                            self.kind = None;
                            self.query = label;
                            self.refresh_search();
                        }
                    }
                });
            });
        }
        let mut similar_req: Option<(usize, u8)> = None;
        egui::TopBottomPanel::bottom("status")
            .exact_height(34.)
            .frame(egui::Frame::new().fill(c_bg()).inner_margin(egui::Margin::symmetric(18, 0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if !self.selected.is_empty() {
                        ui.label(
                            RichText::new(format!("{} selecionados", self.selected.len()))
                                .monospace()
                                .color(c_text()),
                        );
                        let can = !scanning && !self.searching && !busy && !icon_scanning;
                        if ui.add_enabled(can, primary(if self.dup.is_some() { "Mover cópias" } else { "Organizar" })).clicked() {
                            self.preview = true;
                        }
                        if ui.add(flat("Limpar")).clicked() {
                            self.selected.clear();
                        }
                        if self.selected.len() == 1 {
                            let i = *self.selected.iter().next().unwrap();
                            if let Some(e) = self.entries.get(i) {
                                ui.menu_button("Buscar semelhantes", |ui| {
                                    if let Some(mode) = similar_menu(ui, e) {
                                        similar_req = Some((i, mode));
                                    }
                                });
                            }
                        }
                    }
                    if self.dup_rx.is_some() {
                        ui.label(
                            RichText::new(format!("comparando arquivos… {}", self.dup_progress.load(Ordering::Relaxed)))
                                .monospace()
                                .color(c_text()),
                        );
                        if ui.add(flat("Cancelar")).clicked() {
                            self.dup_cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    if self.admin_hint && ui.add(flat("Leitura turbo: reabrir como administrador")).clicked()
                        && let Some(root) = &self.root {
                            relaunch_as_admin(root);
                            crate::tray::quit();
                        }
                    if !self.update_notice.is_empty() {
                        ui.label(RichText::new(&self.update_notice).monospace().color(c_text()));
                        if ui.add(flat("Reabrir agora")).clicked() {
                            if let Ok(exe) = std::env::current_exe() {
                                let _ = std::process::Command::new(exe).arg("--relaunch").spawn();
                            }
                            crate::tray::quit();
                        }
                    }
                    if !self.undo.is_empty()
                        && ui
                            .add_enabled(!scanning && !busy && !icon_scanning, flat("Desfazer última organização"))
                            .clicked()
                    {
                        self.undo_moves(ctx);
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let text = if scanning {
                            if ui.add(flat("Cancelar")).clicked() {
                                self.cancel.store(true, Ordering::Relaxed);
                            }
                            format!("{} arquivos encontrados…", self.progress.load(Ordering::Relaxed))
                        } else if !self.searching && !self.entries.is_empty() {
                            format!(
                                "{}{}{} · busca {:.1} ms",
                                self.status,
                                if self.watchers.is_empty() { "" } else { " · ao vivo" },
                                if self.live_overflow { " (muitas mudanças: use Atualizar)" } else { "" },
                                self.elapsed.as_secs_f64() * 1000.
                            )
                        } else {
                            self.status.clone()
                        };
                        ui.add(
                            egui::Label::new(RichText::new(text).monospace().small().color(c_mute())).truncate(),
                        );
                    });
                });
            });
        let mut want: Vec<(usize, PathBuf)> = Vec::new();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(c_bg()))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(10., 0.);
                ui.allocate_ui(vec2(ui.available_width(), 40.), |ui| {
                    ui.add_space(0.);
                    ui.horizontal_centered(|ui| {
                        ui.add_space(18.);
                        ui.label(
                            RichText::new(format!("{} resultados", self.results.len()))
                                .size(15.)
                                .color(c_text()),
                        );
                        if self.searching {
                            ui.add(egui::Spinner::new().size(12.));
                        }
                        if !self.results.is_empty()
                            && !scanning
                            && ui.add(flat("Selecionar todos")).clicked()
                        {
                            self.selected.extend(self.results.iter().copied());
                        }
                        let sort_text = match self.sort {
                            None => "Ordenar".to_string(),
                            Some((k, desc)) => format!("Ordenar: {} {}", sort_label(k), if desc { "↓" } else { "↑" }),
                        };
                        let mut new_sort = None;
                        ui.menu_button(sort_text, |ui| {
                            for k in [velo::SortKey::Path, velo::SortKey::Name, velo::SortKey::Size, velo::SortKey::Date, velo::SortKey::Kind] {
                                if ui.button(sort_label(k)).clicked() {
                                    // clicar de novo na mesma coluna inverte a direção
                                    let desc = matches!(self.sort, Some((cur, false)) if cur == k);
                                    new_sort = Some(Some((k, desc)));
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.button("Padrão").clicked() {
                                new_sort = Some(None);
                                ui.close();
                            }
                        });
                        if let Some(s) = new_sort {
                            self.sort = s;
                            self.refresh_search();
                        }
                        if self.dup.is_none() && self.dup_rx.is_none() && self.results.len() > 1 && !scanning && ui.add(flat("Achar duplicados")).clicked() {
                            self.start_duplicates(ctx);
                        }
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(18.);
                            for v in View::ALL.into_iter().rev().filter(|_| !compact) {
                                let active = self.view == v;
                                let r = ui
                                    .add(
                                        egui::Label::new(
                                            RichText::new(v.label()).color(if active { c_text() } else { c_mute() }),
                                        )
                                        .sense(Sense::click()),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if active {
                                    ui.painter().line_segment(
                                        [r.rect.left_bottom() + vec2(0., 3.), r.rect.right_bottom() + vec2(0., 3.)],
                                        Stroke::new(1_f32, c_text()),
                                    );
                                }
                                if r.clicked() {
                                    self.view = v;
                                }
                            }
                        });
                    });
                });
                if !self.error.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(18.);
                        ui.colored_label(c_dim(), &self.error);
                    });
                }
                if !self.notice.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(18.);
                        ui.colored_label(c_mute(), &self.notice);
                    });
                }
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.cursor().top(),
                    Stroke::new(1_f32, c_line()),
                );
                if self.root.is_none() {
                    ui.add_space(ui.available_height() * 0.28);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Encontre o que você perdeu.").size(24.).color(c_text()));
                        ui.add_space(10.);
                        ui.label(
                            RichText::new("Escolha um disco inteiro. Depois é só digitar.").color(c_dim()),
                        );
                        ui.add_space(18.);
                        if ui.add(primary("Escolher disco")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.drive_list = list_drives();
                            self.disks = true;
                        }
                    });
                    return;
                }
                if self.results.is_empty() && !scanning && !self.searching && !icon_scanning && self.error.is_empty() {
                    ui.add_space(24.);
                    ui.horizontal(|ui| {
                        ui.add_space(18.);
                        ui.label(RichText::new("Nada encontrado. Tente outra palavra ou outro tipo.").color(c_dim()));
                    });
                }
                let view = self.view;
                let n = self.results.len();
                let (tile, cap) = view.tile();
                let gap = 10.;
                let width = ui.available_width() - 36.;
                let cols = if matches!(view, View::List | View::Compact) {
                    1
                } else {
                    (((width + gap) / (tile + gap)).floor() as usize).max(1)
                };
                let row_h = match view {
                    View::List => 44.,
                    View::Compact => 24.,
                    _ => tile + cap + gap,
                };
                let rows = n.div_ceil(cols);
                let results = &self.results;
                let entries = &self.entries;
                let thumb_map = &self.thumbs.map;
                let selected = &mut self.selected;
                let hl_terms = &self.hl_terms;
                let dup_first = self.dup.as_ref().map(|d| &d.first);
                let cursor = self.cursor;
                let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
                if self.scroll_to_cursor {
                    self.scroll_to_cursor = false;
                    let top = (cursor / cols) as f32 * row_h;
                    let (off, vh) = (self.scroll_y, self.view_h);
                    if top < off {
                        area = area.vertical_scroll_offset(top);
                    } else if vh > 0. && top + row_h > off + vh {
                        area = area.vertical_scroll_offset(top + row_h - vh);
                    }
                }
                self.last_cols = cols;
                let scrolled = area
                    .show_rows(ui, row_h, rows, |ui, range| {
                        ui.spacing_mut().item_spacing = vec2(gap, 0.);
                        for row in range {
                            let mut draw = |ui: &mut egui::Ui, slot: usize, want: &mut Vec<(usize, PathBuf)>, req: &mut Option<(usize, u8)>| {
                                let i = results[slot];
                                let e = &entries[i];
                                if view != View::Compact && !e.ext().is_empty() {
                                    want.push((i, e.path().to_path_buf()));
                                }
                                let state = thumb_map.get(&i);
                                let is_sel = selected.contains(&i);
                                let size = if matches!(view, View::List | View::Compact) {
                                    vec2(ui.available_width(), row_h)
                                } else {
                                    vec2(tile, tile + cap)
                                };
                                let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                                let hot = ui.rect_contains_pointer(rect);
                                let mut act = None;
                                let is_cur = slot == cursor;
                                if view == View::Compact {
                                    if is_sel || hot || is_cur {
                                        ui.painter().rect_filled(rect, 0, if is_sel { c_sel() } else { c_hover() });
                                    }
                                    if slot > 0 && dup_first.is_some_and(|f| f.contains(&i)) {
                                        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1_f32, c_mute()));
                                    }
                                    let right = rect.right() - 14.;
                                    let dg = ui.painter().layout_no_wrap(fmt_date(e.modified), FontId::monospace(12.), c_mute());
                                    ui.painter().galley(Pos2::new(right - dg.size().x, rect.center().y - dg.size().y / 2.), dg, c_mute());
                                    let right = right - 130.;
                                    let meta = if e.size_known { bytes(e.bytes) } else { "—".into() };
                                    let g = ui.painter().layout_no_wrap(meta, FontId::monospace(12.), c_mute());
                                    let gw = g.size().x;
                                    ui.painter().galley(Pos2::new(right - gw, rect.center().y - g.size().y / 2.), g, c_mute());
                                    let left = rect.left() + 14.;
                                    let name_w = (right - gw - 24. - left) * 0.42;
                                    let g = name_galley(ui, e.name(), hl_terms, FontId::proportional(13.5), c_text(), name_w);
                                    ui.painter().galley(Pos2::new(left, rect.center().y - g.size().y / 2.), g, c_text());
                                    let folder = e.folder().to_string();
                                    let px = left + name_w + 16.;
                                    let g = clipped(ui, &folder, FontId::monospace(11.5), c_mute(), right - gw - 24. - px);
                                    ui.painter().galley(Pos2::new(px, rect.center().y - g.size().y / 2.), g, c_mute());
                                } else if view == View::List {
                                    if is_sel || hot || is_cur {
                                        ui.painter().rect_filled(rect, 0, if is_sel { c_sel() } else { c_hover() });
                                    }
                                    if is_sel {
                                        check(ui.painter(), rect.left_center() + vec2(18., -8.), false);
                                    }
                                    let t = Rect::from_min_size(rect.left_center() + vec2(44., -18.), vec2(36., 36.));
                                    draw_tile(ui.painter(), t, e, state);
                                    if slot > 0 && dup_first.is_some_and(|f| f.contains(&i)) {
                                        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1_f32, c_mute()));
                                    }
                                    let right = rect.right() - 18.;
                                    let dg = ui.painter().layout_no_wrap(fmt_date(e.modified), FontId::monospace(12.), c_mute());
                                    ui.painter().galley(Pos2::new(right - dg.size().x, rect.center().y - dg.size().y / 2.), dg, c_mute());
                                    let right = right - 130.;
                                    let meta = if e.size_known { bytes(e.bytes) } else { "—".into() };
                                    let g = ui.painter().layout_no_wrap(meta, FontId::monospace(12.), c_mute());
                                    let gw = g.size().x;
                                    ui.painter().galley(
                                        Pos2::new(right - gw, rect.center().y - g.size().y / 2.),
                                        g,
                                        c_mute(),
                                    );
                                    let tx = t.right() + 14.;
                                    let max_w = (right - gw - 24.) - tx;
                                    let g = name_galley(ui, e.name(), hl_terms, FontId::proportional(14.), c_text(), max_w);
                                    ui.painter().galley(Pos2::new(tx, rect.top() + 4.), g, c_text());
                                    let g = clipped(
                                        ui,
                                        &e.path().display().to_string(),
                                        FontId::monospace(11.5),
                                        c_mute(),
                                        max_w,
                                    );
                                    ui.painter().galley(Pos2::new(tx, rect.top() + 25.), g, c_mute());
                                    if hot {
                                        let mut x = right - gw - 24.;
                                        for (k, label) in ["Copiar caminho", "Mostrar na pasta"].into_iter().enumerate() {
                                            let g = ui.painter().layout_no_wrap(
                                                label.into(),
                                                FontId::proportional(12.),
                                                c_dim(),
                                            );
                                            let r = Rect::from_min_size(
                                                Pos2::new(x - g.size().x - 12., rect.center().y - 12.),
                                                vec2(g.size().x + 12., 24.),
                                            );
                                            let a = ui.interact(r, resp.id.with(k), Sense::click());
                                            ui.painter().rect_filled(r, 4, if a.hovered() { c_hover2() } else { c_sel() });
                                            ui.painter().galley(
                                                r.center() - g.size() / 2.,
                                                g,
                                                if a.hovered() { c_text() } else { c_dim() },
                                            );
                                            if a.clicked() {
                                                act = Some(k);
                                            }
                                            x = r.left() - 6.;
                                        }
                                    }
                                } else {
                                    let t = Rect::from_min_size(rect.min, vec2(tile, tile));
                                    draw_tile(ui.painter(), t, e, state);
                                    if is_sel {
                                        ui.painter().rect_stroke(
                                            t,
                                            3,
                                            Stroke::new(2_f32, c_text()),
                                            StrokeKind::Inside,
                                        );
                                        check(ui.painter(), t.left_top() + vec2(6., 6.), true);
                                    } else if hot || is_cur {
                                        ui.painter().rect_stroke(t, 3, Stroke::new(1_f32, if is_cur { c_text() } else { c_mute() }), StrokeKind::Inside);
                                    }
                                    if cap > 0. {
                                        let g = clipped(ui, e.name(), FontId::proportional(12.5), c_dim(), tile);
                                        ui.painter().galley(rect.min + vec2(0., tile + 5.), g, c_dim());
                                        if view == View::Large {
                                            let meta = format!(
                                                "{} · {}",
                                                e.ext().to_uppercase(),
                                                if e.size_known { bytes(e.bytes) } else { "—".into() }
                                            );
                                            let g = clipped(ui, &meta, FontId::monospace(11.), c_mute(), tile);
                                            ui.painter().galley(rect.min + vec2(0., tile + 22.), g, c_mute());
                                        }
                                    }
                                }
                                let resp = resp.on_hover_text(e.path().display().to_string());
                                if act.is_none() && resp.clicked()
                                    && !selected.remove(&i) {
                                        selected.insert(i);
                                    }
                                if resp.double_clicked() {
                                    open_file(e.path());
                                }
                                resp.context_menu(|ui| {
                                    if ui.button("Abrir").clicked() {
                                        open_file(e.path());
                                        ui.close();
                                    }
                                    if ui.button("Mostrar na pasta").clicked() {
                                        reveal(e.path());
                                        ui.close();
                                    }
                                    if ui.button("Copiar caminho").clicked() {
                                        ui.ctx().copy_text(e.path().display().to_string());
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui.button("Pré-visualizar (F3)").clicked() {
                                        *req = Some((i, 10));
                                        ui.close();
                                    }
                                    ui.label(RichText::new("Buscar semelhantes").small().color(c_mute()));
                                    if let Some(mode) = similar_menu(ui, e) {
                                        *req = Some((i, mode));
                                    }
                                });
                                match act {
                                    Some(0) => ui.ctx().copy_text(e.path().display().to_string()),
                                    Some(_) => reveal(e.path()),
                                    None => {}
                                }
                            };
                            if matches!(view, View::List | View::Compact) {
                                draw(ui, row, &mut want, &mut similar_req);
                            } else {
                                ui.horizontal(|ui| {
                                    ui.add_space(18.);
                                    for c in 0..cols {
                                        let slot = row * cols + c;
                                        if slot < n {
                                            draw(ui, slot, &mut want, &mut similar_req);
                                        }
                                    }
                                });
                                ui.add_space(gap);
                            }
                        }
                    });
                self.scroll_y = scrolled.state.offset.y;
                self.view_h = scrolled.inner_rect.height();
            });
        self.thumbs.want(&want);
        if let Some((i, mode)) = similar_req {
            if mode == 10 {
                self.open_viewer(ctx, i);
            } else {
                self.start_similar(ctx, i, mode);
            }
        }
        // Pré-visualização em tela cheia (F3 ou Espaço): imagem grande ou o começo do texto.
        if let Some(index) = self.viewer {
            if let Some(data) = self.viewer_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
                self.viewer_rx = None;
                match data {
                    PreviewData::Image(img) => self.viewer_tex = Some(ctx.load_texture("previa", img, egui::TextureOptions::LINEAR)),
                    PreviewData::Text(t) => self.viewer_text = Some(t),
                    PreviewData::Nothing => {}
                }
            }
            if let Some(e) = self.entries.get(index) {
                let screen = ctx.content_rect();
                let mut close = false;
                egui::Area::new(egui::Id::new("previa"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(screen.min)
                    .show(ctx, |ui| {
                        let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                        ui.painter().rect_filled(rect, 0, Color32::from_black_alpha(228));
                        let area = Rect::from_min_max(rect.min + vec2(40., 40.), rect.max - vec2(40., 110.));
                        if let Some(tex) = &self.viewer_tex {
                            let [w, h] = tex.size();
                            let scale = (area.width() / w as f32).min(area.height() / h as f32).min(3.);
                            let shown = Rect::from_center_size(area.center(), vec2(w as f32 * scale, h as f32 * scale));
                            ui.painter().image(tex.id(), shown, Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)), Color32::WHITE);
                        } else if let Some(text) = &self.viewer_text {
                            let g = ui.painter().layout(text.clone(), FontId::monospace(13.), Color32::from_rgb(210, 210, 210), area.width().min(900.));
                            ui.painter().with_clip_rect(area).galley(Pos2::new(area.center().x - g.size().x.max(300.) / 2., area.top()), g, Color32::from_rgb(237, 237, 237));
                        } else if self.viewer_rx.is_some() {
                            ui.painter().text(area.center(), Align2::CENTER_CENTER, "Carregando…", FontId::proportional(15.), Color32::from_rgb(154, 154, 154));
                        } else {
                            ui.painter().text(area.center(), Align2::CENTER_CENTER, "Sem prévia para este tipo de arquivo", FontId::proportional(15.), Color32::from_rgb(154, 154, 154));
                        }
                        let name = e.name().to_owned();
                        let info = format!("{}   ·   {}   ·   {}", bytes(e.bytes), fmt_date(e.modified), e.folder());
                        let g = clipped(ui, &name, FontId::proportional(17.), Color32::from_rgb(237, 237, 237), rect.width() - 80.);
                        ui.painter().galley(Pos2::new(rect.left() + 40., rect.bottom() - 92.), g, Color32::from_rgb(237, 237, 237));
                        let g = clipped(ui, &info, FontId::monospace(12.), Color32::from_rgb(154, 154, 154), rect.width() - 80.);
                        ui.painter().galley(Pos2::new(rect.left() + 40., rect.bottom() - 62.), g, Color32::from_rgb(154, 154, 154));
                        ui.painter().text(
                            Pos2::new(rect.left() + 40., rect.bottom() - 36.),
                            Align2::LEFT_CENTER,
                            "Esc fecha  ·  setas mudam de arquivo  ·  Enter abre",
                            FontId::proportional(12.),
                            Color32::from_rgb(102, 102, 102),
                        );
                        if resp.clicked() {
                            close = true;
                        }
                    });
                if close {
                    self.close_viewer();
                }
            } else {
                self.close_viewer();
            }
        }
        if self.disks {
            let mut open = true;
            let mut pick = None;
            egui::Window::new("Escolher disco")
                .open(&mut open)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, vec2(0., 0.))
                .show(ctx, |ui| {
                    ui.set_min_width(380.);
                    if self.drive_list.len() > 1 {
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 4, c_sel());
                        }
                        ui.painter().text(rect.left_center() + vec2(10., 0.), Align2::LEFT_CENTER, "Todos os discos", FontId::proportional(15.), c_text());
                        ui.painter().text(rect.right_center() - vec2(10., 0.), Align2::RIGHT_CENTER, "busca em tudo de uma vez", FontId::proportional(12.), c_mute());
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            pick = Some(PathBuf::from(velo::ALL_DISKS));
                        }
                    }
                    for (root, total, free) in &self.drive_list {
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 4, c_sel());
                        }
                        ui.painter().text(
                            rect.left_center() + vec2(10., 0.),
                            Align2::LEFT_CENTER,
                            root.display().to_string(),
                            FontId::monospace(15.),
                            c_text(),
                        );
                        ui.painter().text(
                            rect.right_center() - vec2(10., 0.),
                            Align2::RIGHT_CENTER,
                            format!("{} · {} livres", bytes(*total), bytes(*free)),
                            FontId::monospace(12.),
                            c_mute(),
                        );
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            pick = Some(root.clone());
                        }
                    }
                    ui.add_space(6.);
                    if ui.add(flat("Escolher só uma pasta…")).clicked() {
                        pick = rfd::FileDialog::new().pick_folder();
                    }
                });
            if let Some(root) = pick {
                self.disks = false;
                self.start_scan(ctx, root, true);
            } else if !open {
                self.disks = false;
            }
        }
        if self.settings {
            let mut open = true;
            egui::Window::new("Configurações")
                .open(&mut open)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Atalho para abrir o Seek de qualquer lugar");
                    ui.horizontal(|ui| {
                        let shown = self
                            .hotkeys
                            .as_ref()
                            .map_or("indisponível".to_string(), |h| hotkey::pretty(&h.spec));
                        ui.label(RichText::new(shown).monospace().color(c_text()));
                        if self.capturing {
                            ui.label(RichText::new("Pressione o novo atalho… (Esc cancela)").color(c_dim()));
                        } else if ui.add(flat("Alterar")).clicked() {
                            self.capturing = true;
                            self.hotkey_error.clear();
                        }
                    });
                    if !self.hotkey_error.is_empty() {
                        ui.label(RichText::new(&self.hotkey_error).color(c_dim()));
                    }
                    ui.add_space(10.);
                    ui.horizontal(|ui| {
                        ui.label("Tema");
                        let light = is_light();
                        if ui.selectable_label(!light, "Escuro").clicked() && light {
                            set_light(ctx, false);
                        }
                        if ui.selectable_label(light, "Claro").clicked() && !light {
                            set_light(ctx, true);
                        }
                    });
                    ui.add_space(10.);
                    let mut on = self.autostart;
                    if ui.checkbox(&mut on, "Abrir junto com o Windows (fica na bandeja)").changed() {
                        match crate::startup::set_enabled(on) {
                            Ok(()) => {
                                self.autostart = on;
                                self.hotkey_error.clear();
                            }
                            Err(e) => self.hotkey_error = e,
                        }
                    }
                    ui.add_space(6.);
                    ui.label(RichText::new("Fechar a janela mantém o Seek rodando na bandeja do Windows.").small().color(c_mute()));
                });
            if !open {
                self.settings = false;
                self.capturing = false;
            }
        }
        if self.help {
            egui::Window::new("Como buscar")
                .open(&mut self.help)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("foto / vídeo / música / documento → busca por formato");
                    ui.label("foto férias → imagens com férias no caminho ou nome");
                    ui.label(".png / .exe / ext:pdf → somente a extensão escolhida");
                    ui.label("foto -ext:png → imagens, exceto PNG");
                    ui.label("vídeo maior:100MB → vídeos maiores que 100 MB");
                    ui.label("ext:jpg | ext:pdf → JPG ou PDF");
                    ui.label("app / código / design / dados → categorias; use nome:app para a palavra");
                    ui.label("Qualquer extensão funciona: .xyz mostra todos os arquivos .xyz");
                    ui.label("modificado:hoje | ontem | semana | mes | ano | 2023 | 2023-05 | 2023-05-14 | 2023-01..2023-06 | >2023-01-01");
                    ui.label("criado: funciona do mesmo jeito (exemplo: foto criado:2022)");
                    ui.label("F3 (ou Espaço fora do campo de busca) mostra a prévia; setas trocam de arquivo; Esc fecha");
                    ui.label("Achar duplicados compara o conteúdo dos resultados atuais; só move, nunca apaga");
                    ui.label("A lista EXTENSÕES, à esquerda, mostra o que existe no seu disco.");
                    ui.label("\"viagem praia\" → frase exata (ignora acentos)");
                    ui.label("nome:foto → procura a palavra foto, sem interpretar categoria");
                    ui.separator();
                    ui.label("O índice é salvo na pasta indices, ao lado do aplicativo.");
                    ui.label("Ao abrir novamente, ele carrega a última pasta automaticamente.");
                    ui.label("Use Atualizar após adicionar, mover ou apagar arquivos no HD.");
                    ui.label("A busca não analisa o conteúdo de documentos ou imagens.");
                    ui.separator();
                    ui.label("Ícones: escolha uma imagem PNG, JPG, ICO, BMP ou WebP.");
                    ui.label("Também pode arrastar a imagem para a janela depois de indexar o HD.");
                    ui.label("A comparação usa o ícone do Windows, não o conteúdo de fotos.");
                    ui.label("Texto e categoria continuam filtrando os resultados de ícone.");
                    ui.label("Use uma imagem recortada do ícone e ajuste a tolerância.");
                    ui.separator();
                    ui.label("Ctrl + scroll muda o tamanho das miniaturas. Clique seleciona; duplo clique abre.");
                    ui.label("Botão direito num arquivo → Buscar semelhantes: nome, foto ou ícone parecido.");
                });
        }
        if self.preview {
            let mut open = true;
            egui::Window::new(if self.dup.is_some() { "Revisar cópias" } else { "Revisar organização" })
                .open(&mut open)
                .default_width(620.)
                .show(ctx, |ui| {
                    if self.dup.is_some() {
                        ui.label(format!(
                            "Mover {} cópias para Seek - Duplicados, uma pasta por grupo, dentro do disco de cada arquivo. Nada é apagado.",
                            self.selected.len()
                        ));
                        ui.label("Escolha antes quais cópias mover (use \"Selecionar cópias\"). Você pode desfazer enquanto o aplicativo estiver aberto.");
                    } else {
                        ui.label(format!(
                            "Mover {} arquivos para subpastas de Seek - Organizados, dentro da pasta escolhida.",
                            self.selected.len()
                        ));
                        ui.label("Nomes repetidos serão ignorados. Você pode desfazer enquanto o aplicativo estiver aberto.");
                    }
                    let mut indices: Vec<_> = self.selected.iter().copied().collect();
                    indices.sort_unstable();
                    egui::ScrollArea::vertical().max_height(280.).show_rows(ui, 24., indices.len(), |ui, range| {
                        for row in range {
                            let e = &self.entries[indices[row]];
                            let i = indices[row];
                            let destino = match self.dup.as_ref().and_then(|d| d.group_of.get(&i)) {
                                Some(g) => format!("Seek - Duplicados / grupo-{:04} / {}", g + 1, e.name()),
                                None => format!("{} / {}", e.kind.label(), e.name()),
                            };
                            ui.label(RichText::new(format!("{} → {}", e.path().display(), destino)).monospace().color(c_dim()));
                        }
                    });
                    ui.add_space(12.);
                    if ui.add(primary("Confirmar e mover arquivos")).clicked() {
                        self.organize(ctx);
                    }
                });
            if !open {
                self.preview = false;
            }
        }
    }
}
