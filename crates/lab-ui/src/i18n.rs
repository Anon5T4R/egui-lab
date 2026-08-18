//! i18n mínimo PT/EN/ES no espírito do padrao-apps: chaves enum (o compilador
//! e o teste de completude cobrem o que o `tsc` cobria na suíte), idiomas com
//! nome em endônimo no seletor.

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Lang {
    Pt,
    En,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Pt, Lang::En, Lang::Es];

    /// Nome no seletor — sempre endônimo (regra da suíte).
    pub fn label(self) -> &'static str {
        match self {
            Lang::Pt => "Português",
            Lang::En => "English",
            Lang::Es => "Español",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Language,
    Theme,
    Cpu,
    Memory,
    Cores,
    Uptime,
    History,
    Clear,
    Error,
    ExprHint,
    NoHistory,
}

const KEYS: &[Key] = &[
    Key::Language,
    Key::Theme,
    Key::Cpu,
    Key::Memory,
    Key::Cores,
    Key::Uptime,
    Key::History,
    Key::Clear,
    Key::Error,
    Key::ExprHint,
    Key::NoHistory,
];

pub fn t(lang: Lang, key: Key) -> &'static str {
    match key {
        Key::Language => match lang {
            Lang::Pt => "Idioma",
            Lang::En => "Language",
            Lang::Es => "Idioma",
        },
        Key::Theme => match lang {
            Lang::Pt => "Tema",
            Lang::En => "Theme",
            Lang::Es => "Tema",
        },
        Key::Cpu => "CPU",
        Key::Memory => match lang {
            Lang::Pt => "Memória",
            Lang::En => "Memory",
            Lang::Es => "Memoria",
        },
        Key::Cores => match lang {
            Lang::Pt => "Núcleos",
            Lang::En => "Cores",
            Lang::Es => "Núcleos",
        },
        Key::Uptime => match lang {
            Lang::Pt => "Ligado há",
            Lang::En => "Uptime",
            Lang::Es => "Tiempo encendido",
        },
        Key::History => match lang {
            Lang::Pt => "Histórico",
            Lang::En => "History",
            Lang::Es => "Historial",
        },
        Key::Clear => match lang {
            Lang::Pt => "Limpar",
            Lang::En => "Clear",
            Lang::Es => "Borrar",
        },
        Key::Error => match lang {
            Lang::Pt => "Erro",
            Lang::En => "Error",
            Lang::Es => "Error",
        },
        Key::ExprHint => match lang {
            Lang::Pt => "expresse, ex.: 2*(3+4)^2",
            Lang::En => "type an expression, e.g. 2*(3+4)^2",
            Lang::Es => "escribe una expresión, p. ej. 2*(3+4)^2",
        },
        Key::NoHistory => match lang {
            Lang::Pt => "sem contas ainda",
            Lang::En => "no calculations yet",
            Lang::Es => "sin cálculos aún",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Equivalente do "tsc força completude": toda chave tem tradução nos
    /// 3 idiomas (match exaustivo + não-vazio).
    #[test]
    fn todas_as_chaves_tem_traducao() {
        for lang in Lang::ALL {
            for key in KEYS {
                let s = t(lang, *key);
                assert!(!s.is_empty(), "chave {key:?} vazia em {lang:?}");
            }
        }
    }
}
