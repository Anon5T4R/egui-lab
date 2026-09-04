//! Controle Win32 da janela, FORA do ciclo de pintura do egui.
//!
//! Por que existe (mesma razão do lab-clip): no Windows, janela oculta não
//! recebe `WM_PAINT`, o winit não entrega `RedrawRequested` e o eframe para
//! de rodar o `update()`. Num app de bandeja, mostrar/esconder têm de falar
//! com o SO direto — os handlers do tray-icon chamam `show()`/`hide()` daqui.
//!
//! Encerrar de verdade continua sendo o caminho limpo (`ViewportCommand::Close`
//! no update com a janela visível): o winit 0.30 ignora WM_QUIT externo.

#![cfg(windows)]

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    ShowWindow, SW_HIDE, SW_SHOW,
};

/// HWND da janela egui ("Lab Keys"), descoberto no 1º frame e cacheado.
static HWND_CACHE: AtomicIsize = AtomicIsize::new(0);

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
        if title == "Lab Keys" {
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
