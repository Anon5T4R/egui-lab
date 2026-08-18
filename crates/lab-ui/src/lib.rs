//! lab-ui — esqueleto do "padrão egui" da suíte: tema (5 paletas nomeadas,
//! espelhando o padrao-apps), i18n PT/EN/ES e config JSON por app.
//! Piloto de referência; nada aqui vira padrão oficial sem decisão.

pub mod config;
pub mod i18n;
pub mod theme;

use config::Config;
use i18n::{Lang, Key};
use theme::Theme;

/// Desenha os combos de idioma + tema (canto de configurações comum aos apps).
/// Devolve `true` se algo mudou (aí o app aplica o tema e salva a config).
pub fn settings_ui(ui: &mut egui::Ui, cfg: &mut Config) -> bool {
    let before = *cfg;

    egui::ComboBox::from_label(i18n::t(cfg.lang, Key::Language))
        .selected_text(cfg.lang.label())
        .show_ui(ui, |ui| {
            for l in Lang::ALL {
                ui.selectable_value(&mut cfg.lang, l, l.label());
            }
        });

    egui::ComboBox::from_label(i18n::t(cfg.lang, Key::Theme))
        .selected_text(cfg.theme.label())
        .show_ui(ui, |ui| {
            for th in Theme::ALL {
                ui.selectable_value(&mut cfg.theme, th, th.label());
            }
        });

    *cfg != before
}
