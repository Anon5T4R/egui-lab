//! Área útil do monitor e encaixe de janela — extraído dos mains do
//! lab-image/lab-player (era o mesmo ~90 linhas duplicado nos dois).
//!
//! `fit_to_work_area` é o caminho único pra "janela do tamanho do
//! conteúdo": px → points, clamp na área útil (taskbar descontada,
//! auto-hide inclusa) e centralização contando o chrome (outer−inner do
//! viewport) pro OUTER inteiro caber.

/// Área útil do monitor em POINTS (tamanho, origem) — o espaço que a
/// taskbar NÃO come. Windows: `SPI_GETWORKAREA` ∪ strip docked da taskbar
/// auto-hide (escondida, o SPI devolve a tela inteira — mas o espaço que
/// ela ocupa ao aparecer é real). Outros: 90% do monitor.
pub fn work_area(ctx: &egui::Context) -> (egui::Vec2, egui::Pos2) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::UI::Shell::{
            ABM_GETSTATE, ABM_GETTASKBARPOS, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP,
            APPBARDATA, SHAppBarMessage,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowW, SystemParametersInfoW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
            SPI_GETWORKAREA,
        };

        let ppp = ctx.pixels_per_point();
        let monitor = ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(egui::vec2(1920.0, 1080.0))
            * ppp; // px

        let mut wa = RECT::default();
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut wa as *mut RECT as *mut core::ffi::c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .is_ok()
        };
        if !ok {
            return (
                egui::vec2(monitor.x / ppp * 0.9, monitor.y / ppp * 0.9),
                egui::pos2(0.0, 0.0),
            );
        }

        unsafe {
            let tray = FindWindowW(
                windows::core::PCWSTR::from_raw(
                    "Shell_TrayWnd\0".encode_utf16().collect::<Vec<_>>().as_ptr(),
                ),
                windows::core::PCWSTR::null(),
            );
            if let Ok(tray) = tray {
                let mut abd = APPBARDATA {
                    cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                    hWnd: tray,
                    ..Default::default()
                };
                let state = SHAppBarMessage(ABM_GETSTATE, &mut abd);
                if state & 1 != 0 && SHAppBarMessage(ABM_GETTASKBARPOS, &mut abd) != 0 {
                    // ABS_AUTOHIDE: desconta o strip docked da taskbar.
                    let strip = &abd.rc;
                    let (w, h) = (monitor.x as i32, monitor.y as i32);
                    let eff = match abd.uEdge {
                        e if e == ABE_LEFT => RECT { left: strip.right, top: 0, right: w, bottom: h },
                        e if e == ABE_TOP => RECT { left: 0, top: strip.bottom, right: w, bottom: h },
                        e if e == ABE_RIGHT => RECT { left: 0, top: 0, right: strip.left, bottom: h },
                        e if e == ABE_BOTTOM => RECT { left: 0, top: 0, right: w, bottom: strip.top },
                        _ => wa,
                    };
                    return (
                        egui::vec2(
                            (eff.right - eff.left) as f32 / ppp,
                            (eff.bottom - eff.top) as f32 / ppp,
                        ),
                        egui::pos2(eff.left as f32 / ppp, eff.top as f32 / ppp),
                    );
                }
            }
        }

        (
            egui::vec2(
                (wa.right - wa.left) as f32 / ppp,
                (wa.bottom - wa.top) as f32 / ppp,
            ),
            egui::pos2(wa.left as f32 / ppp, wa.top as f32 / ppp),
        )
    }
    #[cfg(not(windows))]
    {
        let m = ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(egui::vec2(1920.0, 1080.0));
        (m * 0.9, egui::pos2(m.x * 0.05, m.y * 0.05))
    }
}

/// Converte o tamanho de conteúdo (px) em (`InnerSize`, `OuterPosition`)
/// de viewport: points com clamp na área útil e posição centrada. O chrome
/// da janela (bordas+title bar, outer−inner do viewport real) entra na
/// conta — o OUTER inteiro cabe, title bar não invade a taskbar.
pub fn fit_to_work_area(ctx: &egui::Context, w_px: f32, h_px: f32) -> (egui::Vec2, egui::Pos2) {
    let ppp = ctx.pixels_per_point();
    let (avail, origin) = work_area(ctx);
    let (cx, cy) = ctx.input(|i| {
        let v = i.viewport();
        match (v.outer_rect, v.inner_rect) {
            (Some(o), Some(n)) => (
                (o.width() - n.width()).max(0.0),
                (o.height() - n.height()).max(0.0),
            ),
            _ => (0.0, 40.0),
        }
    });
    let mut w = w_px / ppp;
    let mut h = h_px / ppp;
    let s = ((avail.x - cx) / w).min((avail.y - cy) / h);
    if s < 1.0 {
        w *= s;
        h *= s;
    }
    let pos = egui::pos2(
        origin.x + (avail.x - (w + cx)) / 2.0,
        origin.y + (avail.y - (h + cy)) / 2.0,
    );
    (egui::vec2(w, h), pos)
}
