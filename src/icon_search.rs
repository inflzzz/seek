//! Busca visual pelos ícones reais de arquivo do Windows, sem abrir os programas.
use crate::Entry;
use image::{Rgba, RgbaImage, imageops::FilterType};
use std::{
    collections::HashMap,
    fs,
    hash::{Hash, Hasher},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};

#[derive(Clone, Copy, Debug)]
pub struct Signature {
    pub hash: u64,
    pub color: [u8; 48],
}
impl Signature {
    /// 0 = visualmente idêntico; valores maiores aceitam diferenças maiores.
    pub fn distance(self, other: Self) -> f32 {
        let shape = (self.hash ^ other.hash).count_ones() as f32 / 64.;
        let color = self
            .color
            .iter()
            .zip(other.color)
            .map(|(a, b)| (*a as f32 - b as f32).abs())
            .sum::<f32>()
            / (48. * 255.);
        shape * 0.65 + color * 0.35
    }
    pub fn from_image(image: &RgbaImage) -> Self {
        Self::from_canvas(normalize_image(image))
    }
    /// Foto: reduz a imagem inteira (sem recortar fundo como nos ícones).
    pub fn from_photo(image: &image::DynamicImage) -> Self {
        Self::from_canvas(image.resize_exact(32, 32, FilterType::Triangle).into_rgba8())
    }
    fn from_canvas(image: RgbaImage) -> Self {
        let gray = image::DynamicImage::ImageRgba8(image.clone()).into_luma8();
        let gray = image::imageops::resize(&gray, 32, 32, FilterType::Triangle);
        // Low-frequency DCT coefficients produce a compact perceptual hash.
        let mut coeff = [0f64; 64];
        for v in 0..8 {
            for u in 0..8 {
                let mut sum = 0.;
                for y in 0..32 {
                    for x in 0..32 {
                        sum += gray.get_pixel(x, y)[0] as f64
                            * ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 64.).cos()
                            * ((2 * y + 1) as f64 * v as f64 * std::f64::consts::PI / 64.).cos();
                    }
                }
                coeff[v * 8 + u] = sum;
            }
        }
        let mut sorted = coeff[1..].to_vec();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        let mut hash = 0u64;
        for (i, value) in coeff.iter().enumerate().skip(1) {
            if *value > median {
                hash |= 1 << i;
            }
        }
        let color_image = image::imageops::resize(&image, 4, 4, FilterType::Triangle);
        let mut color = [0; 48];
        for (i, p) in color_image.pixels().enumerate() {
            color[i * 3..i * 3 + 3].copy_from_slice(&p.0[..3]);
        }
        Self { hash, color }
    }
}

fn normalize_image(image: &RgbaImage) -> RgbaImage {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return RgbaImage::from_pixel(32, 32, Rgba([255, 255, 255, 255]));
    }
    let bg = *image.get_pixel(0, 0);
    let uniform_corners = [(width - 1, 0), (0, height - 1), (width - 1, height - 1)]
        .into_iter()
        .all(|(x, y)| {
            let p = image.get_pixel(x, y);
            (0..3).all(|c| p[c].abs_diff(bg[c]) < 12)
        });
    let background = |p: &Rgba<u8>| {
        p[3] < 24 || (uniform_corners && bg[3] > 230 && (0..3).all(|c| p[c].abs_diff(bg[c]) < 12))
    };
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for (x, y, p) in image.enumerate_pixels() {
        if !background(p) {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    let mut canvas = RgbaImage::from_pixel(32, 32, Rgba([255, 255, 255, 255]));
    if left > right || top > bottom {
        return canvas;
    }
    let crop =
        image::imageops::crop_imm(image, left, top, right - left + 1, bottom - top + 1).to_image();
    let resized = image::DynamicImage::ImageRgba8(crop)
        .resize(28, 28, FilterType::Lanczos3)
        .into_rgba8();
    let ox = (32 - resized.width()) / 2;
    let oy = (32 - resized.height()) / 2;
    for (x, y, p) in resized.enumerate_pixels() {
        let a = p[3] as u32;
        let mut out = [255u8; 4];
        for c in 0..3 {
            out[c] = ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
        }
        canvas.put_pixel(ox + x, oy + y, Rgba(out));
    }
    canvas
}

pub fn load_reference(path: &Path) -> Result<(RgbaImage, Signature), String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > 8 * 1024 * 1024 {
        return Err("Escolha uma imagem de ícone com até 8 MB.".into());
    }
    let reader = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Err("A imagem deve ter até 4096 × 4096 pixels.".into());
    }
    let image = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let signature = Signature::from_image(&image);
    Ok((image, signature))
}

pub struct Catalog {
    pub signatures: Arc<Vec<Option<Signature>>>,
    pub errors: usize,
    pub elapsed: std::time::Duration,
    pub cached: bool,
    pub cancelled: bool,
    pub saved: bool,
}
fn identity(entries: &[Entry]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for e in entries {
        e.path.hash(&mut h);
        e.bytes.hash(&mut h);
    }
    h.finish()
}
fn catalog_path(root: &Path, ext: &str) -> Option<PathBuf> {
    crate::cache::location(root).map(|p| p.with_extension(ext))
}
pub fn invalidate(root: &Path) {
    for ext in ["icons", "photos"] {
        if let Some(path) = catalog_path(root, ext) {
            let _ = fs::remove_file(path);
        }
    }
}
fn read_catalog(path: &Path, magic_wanted: &[u8; 8], id: u64, count: usize) -> io::Result<Vec<Option<Signature>>> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    let read_u64 = |r: &mut BufReader<fs::File>| -> io::Result<u64> {
        let mut b = [0; 8];
        r.read_exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    };
    if &magic != magic_wanted
        || read_u64(&mut reader)? != id
        || read_u64(&mut reader)? != count as u64
    {
        return Err(io::Error::other("Catálogo incompatível"));
    }
    let mut result = Vec::new();
    for _ in 0..count {
        let mut present = [0];
        reader.read_exact(&mut present)?;
        result.push(if present[0] == 1 {
            let hash = read_u64(&mut reader)?;
            let mut color = [0; 48];
            reader.read_exact(&mut color)?;
            Some(Signature { hash, color })
        } else if present[0] == 0 {
            None
        } else {
            return Err(io::Error::other("Catálogo inválido"));
        });
    }
    Ok(result)
}
fn write_catalog(path: &Path, magic: &[u8; 8], id: u64, signatures: &[Option<Signature>]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = path.with_extension(format!("{nonce}.tmp"));
    let result = (|| {
        let mut writer = BufWriter::new(fs::File::create(&temp)?);
        writer.write_all(magic)?;
        writer.write_all(&id.to_le_bytes())?;
        writer.write_all(&(signatures.len() as u64).to_le_bytes())?;
        for signature in signatures {
            writer.write_all(&[u8::from(signature.is_some())])?;
            if let Some(s) = signature {
                writer.write_all(&s.hash.to_le_bytes())?;
                writer.write_all(&s.color)?;
            }
        }
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
pub fn build(
    entries: &[Entry],
    root: &Path,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Catalog {
    let start = Instant::now();
    let id = identity(entries);
    let cache = catalog_path(root, "icons");
    if let Some(path) = &cache
        && let Ok(signatures) = read_catalog(path, b"VELOICO1", id, entries.len()) {
            let errors = signatures.iter().filter(|s| s.is_none()).count();
            return Catalog {
                signatures: Arc::new(signatures),
                errors,
                elapsed: start.elapsed(),
                cached: true,
                cancelled: false,
                saved: true,
            };
        }
    // Executáveis e atalhos têm ícone próprio e são lidos do arquivo, em paralelo. Os demais usam o ícone
    // do tipo, resolvido uma única vez por extensão (sem tocar no disco).
    use rayon::prelude::*;
    const OWN_ICON: [&str; 6] = ["exe", "lnk", "scr", "msi", "cpl", "ico"];
    let mut kinds: Vec<&str> = entries.iter().map(|e| e.ext()).filter(|x| !x.is_empty() && !OWN_ICON.contains(x)).collect();
    kinds.sort_unstable();
    kinds.dedup();
    let by_ext: std::collections::HashMap<&str, Signature> = {
        let mut extractor = Extractor::new();
        kinds
            .iter()
            .filter_map(|ext| Some((*ext, extractor.signature_for(Path::new(&format!("x.{ext}")), true)?)))
            .collect()
    };
    let blocks: Vec<Vec<Option<Signature>>> = entries
        .par_chunks(4096)
        .map(|chunk| {
            let mut extractor = Extractor::new();
            chunk
                .iter()
                .map(|e| {
                    let ext = e.ext();
                    let sig = if cancel.load(Ordering::Relaxed) {
                        None
                    } else if OWN_ICON.contains(&ext) {
                        extractor.signature_for(e.path(), false)
                    } else {
                        by_ext.get(ext).copied()
                    };
                    progress.fetch_add(1, Ordering::Relaxed);
                    sig
                })
                .collect()
        })
        .collect();
    let signatures: Vec<Option<Signature>> = blocks.into_iter().flatten().collect();
    let errors = signatures.iter().filter(|s| s.is_none()).count();
    let cancelled = cancel.load(Ordering::Relaxed);
    let saved = !cancelled
        && cache
            .as_ref()
            .is_some_and(|path| write_catalog(path, b"VELOICO1", id, &signatures).is_ok());
    Catalog {
        signatures: Arc::new(signatures),
        errors,
        elapsed: start.elapsed(),
        cached: false,
        cancelled,
        saved,
    }
}

// Formatos que o Windows sabe mostrar como miniatura (HEIC e RAW dependem dos codecs instalados).
const PHOTO_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "bmp", "webp", "ico", "gif", "tif", "tiff", "heic", "heif", "avif", "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2",
];
fn photo_signature(path: &Path) -> Option<(image::DynamicImage, Signature)> {
    // Primeiro a miniatura do Windows: rápida (cache do sistema), certa na rotação e serve para HEIC e RAW.
    #[cfg(windows)]
    if let Some(thumb) = crate::shellthumb::thumbnail(path, 96, true) {
        let small = image::DynamicImage::ImageRgba8(thumb);
        let signature = Signature::from_photo(&small);
        return Some((small, signature));
    }
    if fs::metadata(path).ok()?.len() > 48 * 1024 * 1024 {
        return None;
    }
    let img = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let small = img.thumbnail(128, 128);
    let signature = Signature::from_photo(&small);
    Some((small, signature))
}
/// Foto escolhida como referência: devolve uma miniatura para mostrar e a assinatura.
pub fn photo_reference(path: &Path) -> Result<(RgbaImage, Signature), String> {
    let (small, signature) = photo_signature(path).ok_or("Não foi possível ler esta foto.")?;
    Ok((small.into_rgba8(), signature))
}
/// Assinatura de todas as fotos do índice, em paralelo e com cache em disco.
pub fn build_photos(
    entries: &[Entry],
    root: &Path,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Catalog {
    use rayon::prelude::*;
    let start = Instant::now();
    let id = identity(entries);
    let cache = catalog_path(root, "photos");
    if let Some(path) = &cache
        && let Ok(signatures) = read_catalog(path, b"VELOPHO1", id, entries.len()) {
            return Catalog {
                errors: 0,
                signatures: Arc::new(signatures),
                elapsed: start.elapsed(),
                cached: true,
                cancelled: false,
                saved: true,
            };
        }
    let signatures: Vec<Option<Signature>> = entries
        .par_iter()
        .map(|e| {
            let sig = if cancel.load(Ordering::Relaxed) || !PHOTO_EXTS.contains(&e.ext()) {
                None
            } else {
                photo_signature(e.path()).map(|(_, s)| s)
            };
            progress.fetch_add(1, Ordering::Relaxed);
            sig
        })
        .collect();
    let errors = signatures
        .iter()
        .zip(entries)
        .filter(|(s, e)| s.is_none() && PHOTO_EXTS.contains(&e.ext()))
        .count();
    let cancelled = cancel.load(Ordering::Relaxed);
    let saved = !cancelled
        && cache
            .as_ref()
            .is_some_and(|path| write_catalog(path, b"VELOPHO1", id, &signatures).is_ok());
    Catalog {
        signatures: Arc::new(signatures),
        errors,
        elapsed: start.elapsed(),
        cached: false,
        cancelled,
        saved,
    }
}

#[cfg(windows)]
pub struct Extractor {
    cache: HashMap<i32, Signature>,
    com: bool,
}
#[cfg(windows)]
impl Extractor {
    pub fn new() -> Self {
        let com = unsafe {
            windows_sys::Win32::System::Com::CoInitializeEx(
                std::ptr::null(),
                windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED as u32,
            )
        } >= 0;
        Self {
            cache: HashMap::new(),
            com,
        }
    }
    pub fn signature(&mut self, path: &Path) -> Option<Signature> {
        self.signature_for(path, false)
    }
    /// `by_extension`: `path` é só um nome como "x.pdf" e vale o ícone padrão do tipo (sem ler o disco).
    pub fn signature_for(&mut self, path: &Path, by_extension: bool) -> Option<Signature> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::{Shell::*, WindowsAndMessaging::DestroyIcon};
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            SHGetFileInfoW(
                wide.as_ptr(),
                if by_extension { 0x80 } else { 0 }, // FILE_ATTRIBUTE_NORMAL, só vale com USEFILEATTRIBUTES
                &mut info,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON | SHGFI_SYSICONINDEX | if by_extension { SHGFI_USEFILEATTRIBUTES } else { 0 },
            )
        };
        if ok == 0 || info.hIcon.is_null() {
            return None;
        }
        let signature = if let Some(s) = self.cache.get(&info.iIcon) {
            Some(*s)
        } else {
            let image = unsafe { render_icon(info.hIcon) };
            image.map(|image| {
                let s = Signature::from_image(&image);
                self.cache.insert(info.iIcon, s);
                s
            })
        };
        unsafe {
            DestroyIcon(info.hIcon);
        };
        signature
    }
    /// Used by integration tests; the application compares fingerprints, not screenshots.
    pub fn image(&self, path: &Path) -> Option<RgbaImage> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::{Shell::*, WindowsAndMessaging::DestroyIcon};
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
        if unsafe {
            SHGetFileInfoW(
                wide.as_ptr(),
                0,
                &mut info,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            )
        } == 0
            || info.hIcon.is_null()
        {
            return None;
        }
        let image = unsafe { render_icon(info.hIcon) };
        unsafe {
            DestroyIcon(info.hIcon);
        };
        image
    }
}

impl Default for Extractor {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(windows)]
impl Extractor {
    /// Ícone do Windows com fundo transparente (64x64). `by_extension`: usa o ícone padrão do tipo
    /// (`path` vira só um nome como "x.pdf", sem tocar no disco); senão lê o ícone do próprio arquivo.
    pub fn image_transparent(&self, path: &Path, by_extension: bool) -> Option<RgbaImage> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::{Shell::*, WindowsAndMessaging::DestroyIcon};
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
        let flags = SHGFI_ICON
            | SHGFI_LARGEICON
            | if by_extension { SHGFI_USEFILEATTRIBUTES } else { 0 };
        let ok = unsafe {
            SHGetFileInfoW(
                wide.as_ptr(),
                0x80, // FILE_ATTRIBUTE_NORMAL, só vale com USEFILEATTRIBUTES
                &mut info,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                flags,
            )
        };
        if ok == 0 || info.hIcon.is_null() {
            return None;
        }
        // Desenha sobre branco e sobre preto; a diferença entre os dois revela a transparência.
        let (white, black) = unsafe { (render_icon_on(info.hIcon, 255), render_icon_on(info.hIcon, 0)) };
        unsafe {
            DestroyIcon(info.hIcon);
        }
        let (white, black) = (white?, black?);
        let mut out = RgbaImage::new(64, 64);
        for ((o, w), b) in out.pixels_mut().zip(white.pixels()).zip(black.pixels()) {
            let diff = (0..3).map(|c| w[c] as i32 - b[c] as i32).sum::<i32>() / 3;
            let alpha = (255 - diff).clamp(0, 255);
            if alpha > 0 {
                for c in 0..3 {
                    o[c] = (b[c] as i32 * 255 / alpha).clamp(0, 255) as u8;
                }
            }
            o[3] = alpha as u8;
        }
        Some(out)
    }
}
#[cfg(windows)]
impl Drop for Extractor {
    fn drop(&mut self) {
        if self.com {
            unsafe {
                windows_sys::Win32::System::Com::CoUninitialize();
            }
        }
    }
}
#[cfg(windows)]
unsafe fn render_icon(
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
) -> Option<RgbaImage> {
    unsafe { render_icon_on(icon, 255) }
}
#[cfg(windows)]
unsafe fn render_icon_on(
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
    background: u8,
) -> Option<RgbaImage> {
    use windows_sys::Win32::{
        Graphics::Gdi::*,
        UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx},
    };
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return None;
        }
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: 64,
            biHeight: -64,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(
            dc,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            return None;
        }
        let previous = SelectObject(dc, bitmap);
        std::ptr::write_bytes(bits, background, 64 * 64 * 4);
        let ok = DrawIconEx(dc, 0, 0, icon, 64, 64, 0, std::ptr::null_mut(), DI_NORMAL);
        // Finish any batched GDI drawing before reading the DIB pixels.
        GdiFlush();
        let image = if ok != 0 {
            let bytes = std::slice::from_raw_parts(bits as *const u8, 64 * 64 * 4);
            let mut pixels = bytes.to_vec();
            for p in pixels.as_chunks_mut::<4>().0 {
                p.swap(0, 2);
                p[3] = 255;
            }
            RgbaImage::from_raw(64, 64, pixels)
        } else {
            None
        };
        SelectObject(dc, previous);
        DeleteObject(bitmap);
        DeleteDC(dc);
        image
    }
}
#[cfg(not(windows))]
pub struct Extractor;
#[cfg(not(windows))]
impl Extractor {
    pub fn image_transparent(&self, _: &Path, _: bool) -> Option<RgbaImage> {
        None
    }
}
#[cfg(not(windows))]
impl Extractor {
    pub fn new() -> Self {
        Self
    }
    pub fn signature(&mut self, _: &Path) -> Option<Signature> {
        None
    }
    pub fn signature_for(&mut self, _: &Path, _: bool) -> Option<Signature> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn picture(color: [u8; 4]) -> RgbaImage {
        let mut image = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 0]));
        for y in 12..52 {
            for x in 12..52 {
                image.put_pixel(x, y, Rgba(color));
            }
        }
        image
    }
    #[test]
    fn perceptual_similarity() {
        let a = picture([220, 30, 30, 255]);
        let same = Signature::from_image(&a);
        assert_eq!(same.distance(Signature::from_image(&a)), 0.);
        let resized = image::imageops::resize(&a, 128, 128, FilterType::Lanczos3);
        assert!(same.distance(Signature::from_image(&resized)) < 0.18);
        let different = Signature::from_image(&picture([20, 30, 220, 255]));
        assert!(same.distance(different) > 0.04);
    }
    #[test]
    fn icon_cache_roundtrip() {
        let file = std::env::temp_dir().join(format!("velo-icons-{}.test", std::process::id()));
        let a = Signature::from_image(&picture([200, 50, 10, 255]));
        write_catalog(&file, b"VELOICO1", 42, &[Some(a), None]).unwrap();
        let restored = read_catalog(&file, b"VELOICO1", 42, 2).unwrap();
        assert_eq!(restored[0].unwrap().distance(a), 0.);
        assert!(restored[1].is_none());
        assert!(read_catalog(&file, b"VELOICO1", 43, 2).is_err());
        fs::remove_file(file).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn windows_icon_reference_matches_file() {
        let path = std::env::current_exe().unwrap();
        let mut extractor = Extractor::new();
        let image = extractor
            .image(&path)
            .expect("Ícone do executável de teste");
        assert!(
            image
                .pixels()
                .any(|p| p[0] < 240 || p[1] < 240 || p[2] < 240),
            "O ícone extraído não pode estar em branco"
        );
        let reference = Signature::from_image(&image);
        let actual = extractor.signature(&path).expect("Assinatura do ícone");
        assert!(reference.distance(actual) < 0.01);
        let file = std::env::temp_dir().join(format!("velo-reference-{}.png", std::process::id()));
        image.save(&file).unwrap();
        let (_, loaded) = load_reference(&file).unwrap();
        assert!(actual.distance(loaded) < 0.01);
        fs::remove_file(file).unwrap();
    }
}
