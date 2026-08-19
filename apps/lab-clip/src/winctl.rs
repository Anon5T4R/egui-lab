//! Controle Win32 da janela, FORA do ciclo de pintura do egui.
//!
//! Por que existe: no Windows, janela oculta não recebe `WM_PAINT` (fonte do
//! winit: "The window technically has to be visible to receive WM_PAINT"),
//! então o winit não entrega `RedrawRequested` e o eframe **para de rodar o
//! app** — todo `update()` morre junto. Um app de bandeja não pode depender
//! do paint: mostrar/esconder/fechar têm de falar com o SO direto.

#![cfg(windows)]

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    PostThreadMessageW, SetForegroundWindow, ShowWindow, SW_HIDE, SW_SHOW, WM_CLOSE, WM_QUIT,
};

/// HWND da janela egui ("Lab Clip"), descoberto no 1º frame e cacheado.
static HWND_CACHE: AtomicIsize = AtomicIsize::new(0);
/// Thread da UI (message loop) — pra postar WM_QUIT de outra thread.
static MAIN_TID: AtomicU32 = AtomicU32::new(0);

pub fn remember_main_thread() {
    unsafe { MAIN_TID.store(GetCurrentThreadId(), Ordering::Relaxed) };
}

struct EnumCtx {
    pid: u32,
    found: isize,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumCtx);
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == ctx.pid && IsWindowVisible(hwnd).as_bool() {
        let mut buf = [0u16; 64];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        // A janela do egui tem o título exato; o console do debug tem o
        // caminho do exe — não bate.
        if title == "Lab Clip" {
            ctx.found = hwnd.0 as isize;
            return BOOL(0); // para de enumerar
        }
    }
    BOOL(1)
}

/// Descobre e cacheia o HWND (chamado no 1º `update`, já com janela viva).
pub fn discover() {
    if HWND_CACHE.load(Ordering::Relaxed) != 0 {
        return;
    }
    let mut ctx = EnumCtx {
        pid: std::process::id(),
        found: 0,
    };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    if ctx.found != 0 {
        HWND_CACHE.store(ctx.found, Ordering::Relaxed);
    }
}

fn hwnd() -> Option<HWND> {
    let v = HWND_CACHE.load(Ordering::Relaxed);
    if v != 0 {
        Some(HWND(v as *mut core::ffi::c_void))
    } else {
        None
    }
}

pub fn is_visible() -> bool {
    hwnd()
        .map(|h| unsafe { IsWindowVisible(h).as_bool() })
        .unwrap_or(false)
}

/// Mostra e traz pra frente. Acordar janela oculta por aqui é o ponto: não
/// passa pelo pipeline de repaint (que está congelado quando oculta).
pub fn show() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_SHOW);
            let _ = SetForegroundWindow(h);
        }
    }
}

pub fn hide() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
}

/// Encerra o app de verdade. Com a janela VISÍVEL o caminho limpo é o
/// `ViewportCommand::Close` (via update); OCULTA o loop não roda — WM_QUIT
/// direto na thread da UI encerra o winit (eframe salva e sai).
pub fn force_quit() {
    let tid = MAIN_TID.load(Ordering::Relaxed);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Dispara o fechamento pelo caminho normal (WM_CLOSE = clique no X).
/// (Reservado: o fluxo atual usa ViewportCommand::Close pela UI viva e
/// force_quit quando oculta — mantido pra futuras rotas.)
#[allow(dead_code)]
pub fn request_close() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}
