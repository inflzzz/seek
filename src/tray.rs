//! Bandeja do sistema: fechar a janela só a esconde; o Seek segue rodando (índice na memória e
//! acompanhando o disco) e abre na hora ao clicar no ícone, como o Everything.
use eframe::egui;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};
use std::sync::{Arc, Mutex, Weak, atomic::{AtomicBool, AtomicIsize, Ordering}};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, HWND_BOTTOM, IsWindow, IsWindowVisible, LWA_ALPHA, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_LAYERED,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

pub const TITLE: &str = "Seek";

/// Usado por `Seek.exe --quit`: pede à cópia que está rodando que se feche (e tire o ícone da bandeja).
pub fn signal_quit() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_HOTKEY};
    let class: Vec<u16> = "global_hotkey_app".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let hwnd = FindWindowW(class.as_ptr(), std::ptr::null());
        if !hwnd.is_null() {
            PostMessageW(hwnd, WM_HOTKEY, crate::hotkey::QUIT_ID as usize, 0);
        }
    }
}

thread_local! {
    // O ícone precisa morrer na mesma thread que o criou (a principal). Se o processo terminar sem isso,
    // o Windows deixa um ícone "fantasma" na bandeja até alguém passar o mouse em cima.
    static TRAY: std::cell::RefCell<Option<TrayIcon>> = const { std::cell::RefCell::new(None) };
}
/// Remove o ícone da bandeja. Chame antes de encerrar o programa (na thread principal).
pub fn remove() {
    TRAY.with(|t| t.borrow_mut().take());
}
/// O índice mais recente (só uma referência fraca: não impede a interface de ser a dona dele) e se ele tem
/// mudanças ao vivo que ainda não foram gravadas em disco.
static SNAPSHOT: Mutex<Option<(std::path::PathBuf, Weak<Vec<velo::Entry>>)>> = Mutex::new(None);
pub static DIRTY: AtomicBool = AtomicBool::new(false);
pub fn remember(root: &std::path::Path, entries: &Arc<Vec<velo::Entry>>) {
    *SNAPSHOT.lock().unwrap() = Some((root.to_path_buf(), Arc::downgrade(entries)));
}
fn save_if_dirty() {
    if !DIRTY.swap(false, Ordering::SeqCst) {
        return;
    }
    let snapshot = SNAPSHOT.lock().unwrap().take();
    if let Some((root, weak)) = snapshot
        && let (Some(entries), Some(file)) = (weak.upgrade(), velo::cache::location(&root)) {
            let saved = velo::cache::save(&file, &root, &entries, 0).is_ok();
            velo::log::line(&format!("índice gravado ao fechar: {}", if saved { "ok" } else { "falhou" }));
        }
}
/// Remove o ícone, grava o índice (se mudou) e encerra de vez.
pub fn quit() -> ! {
    remove();
    save_if_dirty();
    std::process::exit(0)
}

/// Ícone do app (assets/icon.png, embutido no programa).
pub fn icon_rgba() -> (Vec<u8>, u32, u32) {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).expect("assets/icon.png").to_rgba8();
    let (w, h) = img.dimensions();
    (img.into_raw(), w, h)
}

/// Verdadeiro enquanto a janela está "estacionada" na bandeja.
///
/// Esconder de verdade (`SW_HIDE`) faz o Windows parar de desenhar a janela, e sem desenho a interface nunca
/// roda: o índice não carrega, o vigia não começa e as mudanças não entram. Por isso a janela é só
/// estacionada: continua existindo, mas transparente, sem clique, sem botão na barra de tarefas e fora do Alt+Tab.
pub static PARKED: AtomicBool = AtomicBool::new(false);
/// Janela que estava na frente quando o Seek foi chamado: o foco volta para ela ao estacionar.
static PREVIOUS: AtomicIsize = AtomicIsize::new(0);
const PARK_STYLE: u32 = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT;

fn window() -> *mut std::ffi::c_void {
    let title: Vec<u16> = TITLE.encode_utf16().chain(Some(0)).collect();
    unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) }
}
unsafe fn restyle(hwnd: *mut std::ffi::c_void, set: u32, clear: u32) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ((style | set) & !clear) as isize);
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

/// Mostra a janela do Seek (de qualquer thread): estacionada na bandeja ou minimizada.
pub fn show_window() {
    let hwnd = window();
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let front = GetForegroundWindow();
        if front != hwnd && !front.is_null() {
            PREVIOUS.store(front as isize, Ordering::SeqCst);
        }
        if PARKED.swap(false, Ordering::SeqCst) {
            // O Windows só refaz o botão da barra de tarefas quando a janela é escondida e mostrada de novo.
            ShowWindow(hwnd, SW_HIDE);
            SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
            restyle(hwnd, WS_EX_APPWINDOW, PARK_STYLE);
        }
        ShowWindow(hwnd, SW_SHOW);
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
    }
}

/// A janela do Seek é a que está na frente agora.
pub fn has_focus() -> bool {
    let hwnd = window();
    !hwnd.is_null() && unsafe { GetForegroundWindow() == hwnd }
}

/// Chamada a cada quadro: se algo (o winit, o Windows) devolveu o estilo de janela de aplicativo à janela
/// estacionada, o botão volta à barra de tarefas; aqui ele é tirado de novo.
pub fn keep_parked() {
    if !PARKED.load(Ordering::SeqCst) {
        return;
    }
    let hwnd = window();
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if style & WS_EX_APPWINDOW != 0 || style & WS_EX_TOOLWINDOW == 0 {
            ShowWindow(hwnd, SW_HIDE);
            restyle(hwnd, PARK_STYLE, WS_EX_APPWINDOW);
            SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

/// "Esconde" a janela na bandeja (na verdade a estaciona; veja `PARKED`). Usa o Windows direto, no mesmo
/// caminho de `show_window`.
pub fn hide_window() {
    let hwnd = window();
    if hwnd.is_null() {
        return;
    }
    unsafe {
        if PARKED.load(Ordering::SeqCst) && IsWindowVisible(hwnd) != 0 {
            return; // já estacionada
        }
        // Esconder, trocar o estilo e mostrar: é o que faz o botão sumir da barra de tarefas.
        ShowWindow(hwnd, SW_HIDE);
        restyle(hwnd, PARK_STYLE, WS_EX_APPWINDOW);
        SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE); // a janela segue existindo (mas invisível) para a interface continuar rodando
        SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        // Sem isto o Seek invisível continuaria sendo a janela ativa e engoliria o que você digita.
        if GetForegroundWindow() == hwnd {
            let previous = PREVIOUS.swap(0, Ordering::SeqCst) as *mut std::ffi::c_void;
            if !previous.is_null() && IsWindow(previous) != 0 {
                SetForegroundWindow(previous);
            } else {
                let taskbar: Vec<u16> = "Shell_TrayWnd".encode_utf16().chain(Some(0)).collect();
                SetForegroundWindow(FindWindowW(taskbar.as_ptr(), std::ptr::null()));
            }
        }
    }
    PARKED.store(true, Ordering::SeqCst);
}

/// Cria o ícone da bandeja. Precisa ser chamado na thread principal e o resultado mantido vivo.
pub fn install(ctx: &egui::Context) -> bool {
    let menu = Menu::new();
    if menu
        .append_items(&[
            &MenuItem::with_id("open", "Abrir Seek", true, None),
            &MenuItem::with_id("quit", "Fechar", true, None),
        ])
        .is_err()
    {
        return false;
    }
    let (rgba, w, h) = icon_rgba();
    let Ok(icon) = Icon::from_rgba(rgba, w, h) else { return false };
    let Ok(tray) = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(TITLE)
        .with_icon(icon)
        .build()
    else {
        return false;
    };
    let repaint = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        if matches!(
            event,
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }
        ) {
            show_window();
            repaint.request_repaint();
        }
    }));
    let repaint = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id.0.as_str() {
        "open" => {
            show_window();
            repaint.request_repaint();
        }
        "quit" => quit(),
        _ => {}
    }));
    TRAY.with(|t| *t.borrow_mut() = Some(tray));
    true
}
