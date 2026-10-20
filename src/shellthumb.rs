//! Miniaturas feitas pelo próprio Windows (as mesmas do Explorador): fotos de celular (HEIC), RAW,
//! vídeos, PDF e documentos, com a rotação certa e usando o cache de miniaturas do sistema.
#![cfg(windows)]
use image::RgbaImage;
use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::{
    Win32::{
        Foundation::SIZE,
        Graphics::Gdi::{
            BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits,
            GetObjectW, HBITMAP,
        },
        System::Com::CoInitializeEx,
        UI::Shell::SHCreateItemFromParsingName,
    },
    core::GUID,
};

// IID_IShellItemImageFactory = {BCC18B79-BA16-442F-80C4-8A59C30C463B}
const IID: GUID = GUID { data1: 0xBCC1_8B79, data2: 0xBA16, data3: 0x442F, data4: [0x80, 0xC4, 0x8A, 0x59, 0xC3, 0x0C, 0x46, 0x3B] };

#[repr(C)]
struct Vtbl {
    _query_interface: usize,
    _add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_image: unsafe extern "system" fn(*mut c_void, SIZE, i32, *mut HBITMAP) -> i32,
}

fn ensure_com() {
    thread_local! { static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    DONE.with(|d| {
        if !d.get() {
            // Se a thread já iniciou o COM de outro modo, o erro é inofensivo: o COM continua valendo.
            unsafe { CoInitializeEx(std::ptr::null(), 0) };
            d.set(true);
        }
    });
}

/// Miniatura de até `size` pixels de lado. `only_thumbnail`: falha em vez de devolver o ícone genérico.
pub fn thumbnail(path: &Path, size: i32, only_thumbnail: bool) -> Option<RgbaImage> {
    ensure_com();
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut object: *mut c_void = std::ptr::null_mut();
    let hr = unsafe { SHCreateItemFromParsingName(wide.as_ptr(), std::ptr::null_mut(), &IID, &mut object) };
    if hr < 0 || object.is_null() {
        return None;
    }
    let vtbl = unsafe { *(object as *const *const Vtbl) };
    let mut bitmap: HBITMAP = std::ptr::null_mut();
    // SIIGBF_BIGGERSIZEOK (1) | SIIGBF_THUMBNAILONLY (8)
    let flags = 1 | if only_thumbnail { 8 } else { 0 };
    let hr = unsafe { ((*vtbl).get_image)(object, SIZE { cx: size, cy: size }, flags, &mut bitmap) };
    unsafe { ((*vtbl).release)(object) };
    if hr < 0 || bitmap.is_null() {
        return None;
    }
    let image = unsafe { bitmap_to_image(bitmap) };
    unsafe { DeleteObject(bitmap) };
    image
}

unsafe fn bitmap_to_image(bitmap: HBITMAP) -> Option<RgbaImage> {
    unsafe {
        let mut info: BITMAP = std::mem::zeroed();
        if GetObjectW(bitmap, std::mem::size_of::<BITMAP>() as i32, (&mut info as *mut BITMAP).cast()) == 0 {
            return None;
        }
        let (w, h) = (info.bmWidth, info.bmHeight);
        if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
            return None;
        }
        let mut header: BITMAPINFO = std::mem::zeroed();
        header.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // de cima para baixo
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        let got = GetDIBits(dc, bitmap, 0, h as u32, px.as_mut_ptr().cast(), &mut header, DIB_RGB_COLORS);
        DeleteDC(dc);
        if got == 0 {
            return None;
        }
        // O Windows entrega BGRA com a cor já multiplicada pela transparência; sem nenhuma transparência, é opaco.
        let any_alpha = px.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
        for p in px.as_chunks_mut::<4>().0 {
            let a = if any_alpha { p[3] as u32 } else { 255 };
            let (b, g, r) = (p[0] as u32, p[1] as u32, p[2] as u32);
            let un = |c: u32| if a == 0 || a == 255 { c } else { (c * 255 / a).min(255) } as u8;
            let (r, g, b) = if a == 0 { (0, 0, 0) } else { (un(r), un(g), un(b)) };
            p.copy_from_slice(&[r, g, b, a as u8]);
        }
        RgbaImage::from_raw(w as u32, h as u32, px)
    }
}
