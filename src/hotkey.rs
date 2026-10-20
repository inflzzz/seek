//! Atalho global (padrão Ctrl+Espaço): abre a janelinha de busca de qualquer lugar do Windows.
//! O atalho escolhido fica gravado ao lado do executável e pode ser trocado em Configurações.
use eframe::egui;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use std::sync::atomic::{AtomicBool, Ordering};

/// Ligado pelo atalho; a interface consome e abre a janelinha de busca.
pub static SUMMON: AtomicBool = AtomicBool::new(false);
const DEFAULT: &str = "ctrl+space";
/// Id especial: `Seek.exe --quit` posta este WM_HOTKEY na janela interna do atalho global. Ela roda na thread
/// principal mesmo com a janela do Seek escondida, então é por onde o programa recebe o pedido de se fechar.
pub const QUIT_ID: u32 = 0x5EE4;

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    current: Option<HotKey>,
    pub spec: String,
    pub error: String,
}

fn file() -> Option<std::path::PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("atalho.txt"))
}
/// "ctrl+space" -> "Ctrl+Space", para mostrar ao usuário.
pub fn pretty(spec: &str) -> String {
    spec.split('+')
        .map(|p| {
            let mut c = p.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
        })
        .collect::<Vec<_>>()
        .join("+")
}
/// Nome de tecla aceito pelo atalho (letras, números, Espaço e F1-F12).
pub fn key_name(key: egui::Key) -> Option<String> {
    let n = key.name();
    match n {
        "Space" => Some("space".into()),
        _ if n.len() == 1 && n.chars().all(|c| c.is_ascii_alphabetic()) => Some(n.to_lowercase()),
        _ if n.starts_with("Num") && n.len() == 4 => Some(n[3..].to_string()),
        _ if n.starts_with('F') && n[1..].parse::<u8>().is_ok_and(|v| (1..=12).contains(&v)) => Some(n.to_lowercase()),
        _ => None,
    }
}

impl Hotkeys {
    pub fn install(ctx: &egui::Context) -> Option<Self> {
        let manager = GlobalHotKeyManager::new().ok()?;
        let repaint = ctx.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.id == QUIT_ID {
                crate::tray::quit();
            }
            if event.state == HotKeyState::Pressed {
                SUMMON.store(true, Ordering::SeqCst);
                crate::tray::show_window();
                repaint.request_repaint();
            }
        }));
        let mut me = Self { manager, current: None, spec: String::new(), error: String::new() };
        let saved = file().and_then(|f| std::fs::read_to_string(f).ok()).map(|s| s.trim().to_lowercase());
        if me.set(saved.as_deref().unwrap_or(DEFAULT)).is_err() {
            let _ = me.set(DEFAULT);
        }
        Some(me)
    }
    /// Troca o atalho. Em caso de erro (por exemplo, já usado por outro programa) mantém o anterior.
    pub fn set(&mut self, spec: &str) -> Result<(), String> {
        let hotkey: HotKey = spec.parse().map_err(|_| "Atalho inválido.".to_string())?;
        if let Some(old) = self.current.take() {
            let _ = self.manager.unregister(old);
        }
        if self.manager.register(hotkey).is_err() {
            let msg = format!("{} já está em uso por outro programa.", pretty(spec));
            if let Ok(old) = self.spec.parse::<HotKey>()
                && self.manager.register(old).is_ok() {
                    self.current = Some(old);
                }
            self.error = msg.clone();
            return Err(msg);
        }
        self.current = Some(hotkey);
        self.spec = spec.to_owned();
        self.error.clear();
        if let Some(f) = file() {
            let _ = std::fs::write(f, spec);
        }
        Ok(())
    }
}
