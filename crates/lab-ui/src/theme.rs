//! Temas do lab — aproximação das 5 paletas nomeadas do padrao-apps
//! (Nature, DarkBlue, CalmGreen, PastelPink, PunkPrincess), aplicadas sobre
//! `egui::Visuals`. Os tons são mão-alta de referência, não os valores exatos
//! do padrão Tauri (isso só faria sentido num port de verdade).

use egui::Color32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Theme {
    Nature,
    DarkBlue,
    CalmGreen,
    PastelPink,
    PunkPrincess,
}

impl Theme {
    pub const ALL: [Theme; 5] = [
        Theme::Nature,
        Theme::DarkBlue,
        Theme::CalmGreen,
        Theme::PastelPink,
        Theme::PunkPrincess,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Theme::Nature => "Nature",
            Theme::DarkBlue => "DarkBlue",
            Theme::CalmGreen => "CalmGreen",
            Theme::PastelPink => "PastelPink",
            Theme::PunkPrincess => "PunkPrincess",
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Theme::Nature => Palette {
                dark: true,
                base: Color32::from_rgb(0x16, 0x1b, 0x17),
                sunken: Color32::from_rgb(0x0e, 0x12, 0x0f),
                accent: Color32::from_rgb(0x7c, 0xbf, 0x7f),
                text: Color32::from_rgb(0xdf, 0xe7, 0xdf),
            },
            Theme::DarkBlue => Palette {
                dark: true,
                base: Color32::from_rgb(0x12, 0x18, 0x26),
                sunken: Color32::from_rgb(0x0c, 0x11, 0x1c),
                accent: Color32::from_rgb(0x5f, 0x9e, 0xe6),
                text: Color32::from_rgb(0xdf, 0xe4, 0xee),
            },
            Theme::CalmGreen => Palette {
                dark: false,
                base: Color32::from_rgb(0xea, 0xf2, 0xea),
                sunken: Color32::from_rgb(0xdc, 0xe8, 0xdc),
                accent: Color32::from_rgb(0x3f, 0x8f, 0x5f),
                text: Color32::from_rgb(0x22, 0x30, 0x2a),
            },
            Theme::PastelPink => Palette {
                dark: false,
                base: Color32::from_rgb(0xf7, 0xed, 0xf2),
                sunken: Color32::from_rgb(0xed, 0xdd, 0xe6),
                accent: Color32::from_rgb(0xc9, 0x74, 0x9f),
                text: Color32::from_rgb(0x33, 0x22, 0x2b),
            },
            Theme::PunkPrincess => Palette {
                dark: true,
                base: Color32::from_rgb(0x19, 0x10, 0x19),
                sunken: Color32::from_rgb(0x12, 0x0a, 0x12),
                accent: Color32::from_rgb(0xf0, 0x60, 0x9f),
                text: Color32::from_rgb(0xed, 0xdf, 0xe8),
            },
        }
    }

    /// Aplica o tema no contexto (visuais base + overrides da paleta).
    pub fn apply(self, ctx: &egui::Context) {
        let pal = self.palette();
        let mut v = if pal.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.panel_fill = pal.base;
        v.window_fill = pal.base;
        v.extreme_bg_color = pal.sunken;
        v.code_bg_color = pal.sunken;
        v.hyperlink_color = pal.accent;
        v.selection.bg_fill = pal.accent;
        ctx.set_visuals(v);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// Fundo de painel/janela.
    pub base: Color32,
    /// Fundo de campos, gráficos, barras (mais "fundo" que o fundo).
    pub sunken: Color32,
    /// Cor de destaque (linhas, seleção, links).
    pub accent: Color32,
    /// Texto principal sobre `base`/`sunken`.
    pub text: Color32,
}

impl Palette {
    /// Cor de linha de grade sutil sobre `sunken`.
    pub fn grid(self) -> Color32 {
        if self.dark {
            Color32::from_black_alpha(50)
        } else {
            Color32::from_black_alpha(18)
        }
    }
}
