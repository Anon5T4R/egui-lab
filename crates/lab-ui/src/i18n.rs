//! i18n PT/EN/ES no espírito do padrao-apps: chaves enum (o compilador e o
//! teste de completude cobrem o que o `tsc` cobria na suíte), idiomas com nome
//! em endônimo no seletor.

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
    // compartilhadas
    Language,
    Theme,
    Error,
    Search,
    Copy,
    // lab-monitor
    Cpu,
    Memory,
    Cores,
    Uptime,
    Processes,
    Filter,
    Name,
    End,
    KillTitle,
    KillAsk,
    Confirm,
    Cancel,
    // lab-calc
    History,
    Clear,
    ExprHint,
    NoHistory,
    // lab-clip
    ShowHide,
    Quit,
    Pin,
    Unpin,
    Delete,
    Empty,
    // lab-keys
    Vault,
    MasterPassword,
    Unlock,
    NewVault,
    WrongPassword,
    Add,
    Save,
    Items,
    Username,
    Password,
    Lock,
}

const KEYS: &[Key] = &[
    Key::Language,
    Key::Theme,
    Key::Error,
    Key::Search,
    Key::Copy,
    Key::Cpu,
    Key::Memory,
    Key::Cores,
    Key::Uptime,
    Key::Processes,
    Key::Filter,
    Key::Name,
    Key::End,
    Key::KillTitle,
    Key::KillAsk,
    Key::Confirm,
    Key::Cancel,
    Key::History,
    Key::Clear,
    Key::ExprHint,
    Key::NoHistory,
    Key::ShowHide,
    Key::Quit,
    Key::Pin,
    Key::Unpin,
    Key::Delete,
    Key::Empty,
    Key::Vault,
    Key::MasterPassword,
    Key::Unlock,
    Key::NewVault,
    Key::WrongPassword,
    Key::Add,
    Key::Save,
    Key::Items,
    Key::Username,
    Key::Password,
    Key::Lock,
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
        Key::Error => match lang {
            Lang::Pt => "Erro",
            Lang::En => "Error",
            Lang::Es => "Error",
        },
        Key::Search => match lang {
            Lang::Pt => "Buscar…",
            Lang::En => "Search…",
            Lang::Es => "Buscar…",
        },
        Key::Copy => match lang {
            Lang::Pt => "Copiar",
            Lang::En => "Copy",
            Lang::Es => "Copiar",
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
        Key::Processes => match lang {
            Lang::Pt => "Processos",
            Lang::En => "Processes",
            Lang::Es => "Procesos",
        },
        Key::Filter => match lang {
            Lang::Pt => "Filtrar",
            Lang::En => "Filter",
            Lang::Es => "Filtrar",
        },
        Key::Name => match lang {
            Lang::Pt => "Nome",
            Lang::En => "Name",
            Lang::Es => "Nombre",
        },
        Key::End => match lang {
            Lang::Pt => "Encerrar",
            Lang::En => "End task",
            Lang::Es => "Finalizar",
        },
        Key::KillTitle => match lang {
            Lang::Pt => "Encerrar processo?",
            Lang::En => "End process?",
            Lang::Es => "¿Finalizar proceso?",
        },
        Key::KillAsk => match lang {
            Lang::Pt => "O trabalho não salvo deste app se perde.",
            Lang::En => "Unsaved work in that app is lost.",
            Lang::Es => "El trabajo no guardado de esa app se pierde.",
        },
        Key::Confirm => match lang {
            Lang::Pt => "Confirmar",
            Lang::En => "Confirm",
            Lang::Es => "Confirmar",
        },
        Key::Cancel => match lang {
            Lang::Pt => "Cancelar",
            Lang::En => "Cancel",
            Lang::Es => "Cancelar",
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
        Key::ShowHide => match lang {
            Lang::Pt => "Mostrar/Ocultar",
            Lang::En => "Show/Hide",
            Lang::Es => "Mostrar/Ocultar",
        },
        Key::Quit => match lang {
            Lang::Pt => "Sair",
            Lang::En => "Quit",
            Lang::Es => "Salir",
        },
        Key::Pin => match lang {
            Lang::Pt => "Fixar",
            Lang::En => "Pin",
            Lang::Es => "Fijar",
        },
        Key::Unpin => match lang {
            Lang::Pt => "Desafixar",
            Lang::En => "Unpin",
            Lang::Es => "Desfijar",
        },
        Key::Delete => match lang {
            Lang::Pt => "Excluir",
            Lang::En => "Delete",
            Lang::Es => "Eliminar",
        },
        Key::Empty => match lang {
            Lang::Pt => "vazio — copie algo",
            Lang::En => "empty — copy something",
            Lang::Es => "vacío — copia algo",
        },
        Key::Vault => match lang {
            Lang::Pt => "Cofre",
            Lang::En => "Vault",
            Lang::Es => "Bóveda",
        },
        Key::MasterPassword => match lang {
            Lang::Pt => "Senha mestra",
            Lang::En => "Master password",
            Lang::Es => "Contraseña maestra",
        },
        Key::Unlock => match lang {
            Lang::Pt => "Destrancar",
            Lang::En => "Unlock",
            Lang::Es => "Desbloquear",
        },
        Key::NewVault => match lang {
            Lang::Pt => "Novo cofre",
            Lang::En => "New vault",
            Lang::Es => "Nueva bóveda",
        },
        Key::WrongPassword => match lang {
            Lang::Pt => "senha incorreta ou arquivo corrompido/adulterado",
            Lang::En => "wrong password or corrupted/tampered file",
            Lang::Es => "contraseña incorrecta o archivo corrupto/adulterado",
        },
        Key::Add => match lang {
            Lang::Pt => "Adicionar",
            Lang::En => "Add",
            Lang::Es => "Añadir",
        },
        Key::Save => match lang {
            Lang::Pt => "Salvar",
            Lang::En => "Save",
            Lang::Es => "Guardar",
        },
        Key::Items => match lang {
            Lang::Pt => "Itens",
            Lang::En => "Items",
            Lang::Es => "Elementos",
        },
        Key::Username => match lang {
            Lang::Pt => "Usuário",
            Lang::En => "Username",
            Lang::Es => "Usuario",
        },
        Key::Password => match lang {
            Lang::Pt => "Senha",
            Lang::En => "Password",
            Lang::Es => "Contraseña",
        },
        Key::Lock => match lang {
            Lang::Pt => "Trancar",
            Lang::En => "Lock",
            Lang::Es => "Bloquear",
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
