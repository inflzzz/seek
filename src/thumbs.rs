// Miniaturas de fotos: decodifica em segundo plano apenas o que está visível na tela.
use eframe::egui;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
};

pub enum State {
    Loading,
    Ready(egui::TextureHandle),
    Icon(egui::TextureHandle),
    Fit(egui::TextureHandle), // miniatura de documento/vídeo: mostrada inteira, sem cortar
    Failed,
}
enum Msg {
    Skipped(usize),
    Failed(usize),
    Ready(usize, egui::ColorImage),
    Icon(usize, egui::ColorImage),
    Fit(usize, egui::ColorImage),
}
pub struct Thumbs {
    tx: mpsc::Sender<(usize, PathBuf)>,
    rx: mpsc::Receiver<Msg>,
    visible: Arc<Mutex<HashSet<usize>>>,
    pub map: HashMap<usize, State>,
}

// Formatos que a crate `image` do projeto decodifica. HEIC e RAW ficam com bloco cinza.
pub fn supported(ext: &str) -> bool {
    matches!(ext, "jpg" | "jpeg" | "png" | "bmp" | "webp" | "ico")
}
fn decode(path: &Path) -> Option<egui::ColorImage> {
    // shortcut: ignora a rotação EXIF e não grava cache em disco; reavaliar se o HD for lento.
    if std::fs::metadata(path).ok()?.len() > 64 << 20 {
        return None;
    }
    let img = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let t = img.thumbnail(256, 256).to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [t.width() as usize, t.height() as usize],
        t.as_raw(),
    ))
}
fn to_color(t: &image::RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw())
}
// Ícone do Windows: o próprio do arquivo nos executáveis, o padrão do tipo nos demais.
fn shell_icon(
    shell: &velo::icon_search::Extractor,
    path: &Path,
    ext: &str,
    cache: &mut HashMap<String, Option<egui::ColorImage>>,
) -> Option<egui::ColorImage> {
    if matches!(ext, "exe" | "lnk" | "scr" | "msi" | "cpl") {
        return shell.image_transparent(path, false).map(|i| to_color(&i));
    }
    if ext.is_empty() {
        return None;
    }
    cache
        .entry(ext.to_owned())
        .or_insert_with(|| {
            shell
                .image_transparent(Path::new(&format!("x.{ext}")), true)
                .map(|i| to_color(&i))
        })
        .clone()
}
fn icon_msg(i: usize, image: Option<egui::ColorImage>) -> Msg {
    match image {
        Some(img) => Msg::Icon(i, img),
        None => Msg::Failed(i),
    }
}
impl Thumbs {
    // Cada nova leitura de pasta cria um `Thumbs` novo: os índices antigos deixam de valer.
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, jobs) = mpsc::channel::<(usize, PathBuf)>();
        let (out, rx) = mpsc::channel();
        let jobs = Arc::new(Mutex::new(jobs));
        let visible: Arc<Mutex<HashSet<usize>>> = Default::default();
        for _ in 0..3 {
            let (jobs, out, visible, ctx) = (jobs.clone(), out.clone(), visible.clone(), ctx.clone());
            thread::spawn(move || {
                let shell = velo::icon_search::Extractor::new();
                let mut icons = HashMap::new();
                loop {
                    let Ok((i, path)) = jobs.lock().unwrap().recv() else {
                        break;
                    };
                    let msg = if !visible.lock().unwrap().contains(&i) {
                        Msg::Skipped(i)
                    } else {
                        let ext = path
                            .extension()
                            .map(|e| e.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                        let kind = velo::Kind::for_ext(&ext);
                        let own_icon = matches!(ext.as_str(), "exe" | "lnk" | "scr" | "msi" | "cpl");
                        let shell_thumb = || {
                            // o Windows pode devolver maior que o pedido (vídeo em 1280x720): reduz para economizar memória
                            velo::shellthumb::thumbnail(&path, 256, true).map(|i| {
                                let small = image::DynamicImage::ImageRgba8(i).thumbnail(256, 256).to_rgba8();
                                to_color(&small)
                            })
                        };
                        if own_icon {
                            icon_msg(i, shell_icon(&shell, &path, &ext, &mut icons))
                        } else if kind == velo::Kind::Photo {
                            // 1) miniatura do Windows (HEIC, RAW, rotação certa)  2) decodificar aqui  3) ícone do tipo
                            if let Some(img) = shell_thumb() {
                                Msg::Ready(i, img)
                            } else if supported(&ext) {
                                match decode(&path) {
                                    Some(img) => Msg::Ready(i, img),
                                    None => Msg::Failed(i),
                                }
                            } else {
                                icon_msg(i, shell_icon(&shell, &path, &ext, &mut icons))
                            }
                        } else if matches!(kind, velo::Kind::Video | velo::Kind::Document | velo::Kind::Design | velo::Kind::Audio) {
                            match shell_thumb() {
                                Some(img) => Msg::Fit(i, img),
                                None => icon_msg(i, shell_icon(&shell, &path, &ext, &mut icons)),
                            }
                        } else {
                            icon_msg(i, shell_icon(&shell, &path, &ext, &mut icons))
                        }
                    };
                    if out.send(msg).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }
        Self {
            tx,
            rx,
            visible,
            map: HashMap::new(),
        }
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Skipped(i) => {
                    self.map.remove(&i);
                }
                Msg::Failed(i) => {
                    self.map.insert(i, State::Failed);
                }
                Msg::Ready(i, img) => {
                    let tex = ctx.load_texture(format!("t{i}"), img, egui::TextureOptions::LINEAR);
                    self.map.insert(i, State::Ready(tex));
                }
                Msg::Icon(i, img) => {
                    let tex = ctx.load_texture(format!("i{i}"), img, egui::TextureOptions::LINEAR);
                    self.map.insert(i, State::Icon(tex));
                }
                Msg::Fit(i, img) => {
                    let tex = ctx.load_texture(format!("f{i}"), img, egui::TextureOptions::LINEAR);
                    self.map.insert(i, State::Fit(tex));
                }
            }
        }
        if self.map.len() > 1500 {
            let visible = self.visible.lock().unwrap();
            self.map.retain(|k, _| visible.contains(k));
        }
    }
    // Informa o que está na tela agora e pede o que ainda não foi pedido.
    pub fn want(&mut self, wanted: &[(usize, PathBuf)]) {
        *self.visible.lock().unwrap() = wanted.iter().map(|(i, _)| *i).collect();
        for (i, path) in wanted {
            if !self.map.contains_key(i) {
                self.map.insert(*i, State::Loading);
                let _ = self.tx.send((*i, path.clone()));
            }
        }
    }
}
