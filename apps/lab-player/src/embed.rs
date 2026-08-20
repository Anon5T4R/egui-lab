//! Embed do vídeo na janela do player (Windows-only).
//!
//! Técnica clássica do mpv: `--wid=<hwnd>` — o mpv renderiza dentro de um
//! child window que criamos sobre o painel central do egui. O DWM compõe o
//! child acima da superfície OpenGL do eframe; reposicionar por frame é
//! barato (SetWindowPos). No Linux não há child Win32: o player abre o mpv
//! em janela própria (política do lab: AppImage enxuto).

#[cfg(windows)]
mod imp {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, FindWindowW, SetWindowPos, ShowWindow, HWND_TOP, SW_HIDE,
        SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, WS_CHILD,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub struct VideoEmbed {
        child: isize,
    }

    impl VideoEmbed {
        /// Acha a janela do player (título exato) e cria o child que o mpv
        /// vai usar como superfície de vídeo.
        pub fn new(window_title: &str) -> Option<Self> {
            unsafe {
                // A janela existe no primeiro update() — se ainda não, retorna
                // None e o caller tenta de novo no próximo frame.
                let parent =
                    FindWindowW(None, PCWSTR::from_raw(wide(window_title).as_ptr())).ok()?;

                let hinstance = GetModuleHandleW(None).ok()?;
                let child = CreateWindowExW(
                    Default::default(),           // dwExStyle
                    w!("STATIC"),                 // classe de sistema
                    PCWSTR::null(),               // sem título
                    WS_CHILD,                     // nasce oculto
                    0, 0, 16, 16,                 // posição/tamanho provisórios
                    parent,
                    None,
                    Some(&windows::Win32::Foundation::HINSTANCE(hinstance.0)),
                    None,
                )
                .ok()?;
                Some(Self { child: child.0 as isize })
            }
        }

        pub fn child_hwnd(&self) -> isize {
            self.child
        }

        /// Reposiciona o child sobre o retângulo do painel de vídeo
        /// (coordenadas em pixels, relativas à client area do pai).
        pub fn place(&self, x: i32, y: i32, w: i32, h: i32) {
            unsafe {
                let _ = SetWindowPos(
                    HWND(self.child as _),
                    HWND_TOP,
                    x,
                    y,
                    w,
                    h,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }

        pub fn set_visible(&self, visible: bool) {
            unsafe {
                let _ = ShowWindow(
                    HWND(self.child as _),
                    if visible { SW_SHOWNOACTIVATE } else { SW_HIDE },
                );
            }
        }
    }
}

#[cfg(windows)]
pub use imp::VideoEmbed;

#[cfg(not(windows))]
mod imp {
    /// Stub: no Linux o vídeo roda na janela própria do mpv.
    pub struct VideoEmbed;

    impl VideoEmbed {
        pub fn new(_window_title: &str) -> Option<Self> {
            None
        }
        pub fn child_hwnd(&self) -> isize {
            0
        }
        pub fn place(&self, _x: i32, _y: i32, _w: i32, _h: i32) {}
        pub fn set_visible(&self, _visible: bool) {}
    }
}

#[cfg(not(windows))]
pub use imp::VideoEmbed;
