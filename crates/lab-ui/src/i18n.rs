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
    Disks,
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
    // lab-keys onda 4
    Totp,
    QuickUnlock,
    ForgetKey,
    Edit,
    SecondsShort,
    // lab-keys: confirmação de exclusão
    TrashHint,
    // lab-hub
    Install,
    Update,
    Open,
    StartMenu,
    Desktop,
    Installed,
    Available,
    Downloading,
    Refresh,
    UpToDate,
    NotInstalled,
    // lab-hub onda 6
    Uninstall,
    Clean,
    OpenFolder,
    UninstallAsk,
    // lab-hub: card do próprio hub
    Running,
    Restart,
    // lab-clip: preferências (atalho/autostart/bandeja)
    Settings,
    Hotkey,
    Define,
    Autostart,
    PressKeys,
    CloseToTray,
}

/// Todas as chaves — usado pelo teste de completude (fora de teste o match
/// exaustivo do `t()` já cobre; o const fica silenciado pra não warnar na
/// build da lib).
#[cfg_attr(not(test), allow(dead_code))]
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
    Key::Disks,
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
    Key::Totp,
    Key::QuickUnlock,
    Key::ForgetKey,
    Key::Edit,
    Key::SecondsShort,
    Key::TrashHint,
    Key::Install,
    Key::Update,
    Key::Open,
    Key::StartMenu,
    Key::Desktop,
    Key::Installed,
    Key::Available,
    Key::Downloading,
    Key::Refresh,
    Key::UpToDate,
    Key::NotInstalled,
    Key::Uninstall,
    Key::Clean,
    Key::OpenFolder,
    Key::UninstallAsk,
    Key::Running,
    Key::Restart,
    Key::Settings,
    Key::Hotkey,
    Key::Define,
    Key::Autostart,
    Key::PressKeys,
    Key::CloseToTray,
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
        Key::Disks => match lang {
            Lang::Pt => "Discos",
            Lang::En => "Disks",
            Lang::Es => "Discos",
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
        Key::Totp => "TOTP",
        Key::QuickUnlock => match lang {
            Lang::Pt => "Desbloqueio rápido neste PC",
            Lang::En => "Quick unlock on this PC",
            Lang::Es => "Desbloqueo rápido en este PC",
        },
        Key::ForgetKey => match lang {
            Lang::Pt => "Esquecer chave",
            Lang::En => "Forget key",
            Lang::Es => "Olvidar clave",
        },
        Key::Edit => match lang {
            Lang::Pt => "Editar",
            Lang::En => "Edit",
            Lang::Es => "Editar",
        },
        Key::SecondsShort => "s",
        Key::Install => match lang {
            Lang::Pt => "Instalar",
            Lang::En => "Install",
            Lang::Es => "Instalar",
        },
        Key::Update => match lang {
            Lang::Pt => "Atualizar",
            Lang::En => "Update",
            Lang::Es => "Actualizar",
        },
        Key::Open => match lang {
            Lang::Pt => "Abrir",
            Lang::En => "Open",
            Lang::Es => "Abrir",
        },
        Key::StartMenu => match lang {
            Lang::Pt => "Menu Iniciar",
            Lang::En => "Start menu",
            Lang::Es => "Menú Inicio",
        },
        Key::Desktop => match lang {
            Lang::Pt => "Área de trabalho",
            Lang::En => "Desktop",
            Lang::Es => "Escritorio",
        },
        Key::Installed => match lang {
            Lang::Pt => "instalado",
            Lang::En => "installed",
            Lang::Es => "instalado",
        },
        Key::Available => match lang {
            Lang::Pt => "disponível",
            Lang::En => "available",
            Lang::Es => "disponible",
        },
        Key::Downloading => match lang {
            Lang::Pt => "baixando",
            Lang::En => "downloading",
            Lang::Es => "descargando",
        },
        Key::Refresh => match lang {
            Lang::Pt => "Atualizar lista",
            Lang::En => "Refresh",
            Lang::Es => "Actualizar lista",
        },
        Key::UpToDate => match lang {
            Lang::Pt => "em dia",
            Lang::En => "up to date",
            Lang::Es => "al día",
        },
        Key::NotInstalled => match lang {
            Lang::Pt => "não instalado",
            Lang::En => "not installed",
            Lang::Es => "no instalado",
        },
        Key::Uninstall => match lang {
            Lang::Pt => "Desinstalar",
            Lang::En => "Uninstall",
            Lang::Es => "Desinstalar",
        },
        Key::Clean => match lang {
            Lang::Pt => "Limpeza",
            Lang::En => "Cleanup",
            Lang::Es => "Limpieza",
        },
        Key::OpenFolder => match lang {
            Lang::Pt => "Abrir pasta",
            Lang::En => "Open folder",
            Lang::Es => "Abrir carpeta",
        },
        Key::UninstallAsk => match lang {
            Lang::Pt => "Desinstalar e remover atalhos?",
            Lang::En => "Uninstall and remove shortcuts?",
            Lang::Es => "¿Desinstalar y quitar accesos?",
        },
        Key::Running => match lang {
            Lang::Pt => "rodando",
            Lang::En => "running",
            Lang::Es => "ejecutando",
        },
        Key::Restart => match lang {
            Lang::Pt => "reinicie o Lab Hub",
            Lang::En => "restart Lab Hub",
            Lang::Es => "reinicie Lab Hub",
        },
        Key::TrashHint => match lang {
            Lang::Pt => "O item vai pra lixeira — recuperável no LocalKeys oficial.",
            Lang::En => "The item goes to trash — recoverable in the official LocalKeys.",
            Lang::Es => "El elemento va a la papelera — recuperable en LocalKeys oficial.",
        },
        Key::Settings => match lang {
            Lang::Pt => "Configurações",
            Lang::En => "Settings",
            Lang::Es => "Ajustes",
        },
        Key::Hotkey => match lang {
            Lang::Pt => "Atalho global",
            Lang::En => "Global hotkey",
            Lang::Es => "Atajo global",
        },
        Key::Define => match lang {
            Lang::Pt => "Definir…",
            Lang::En => "Set…",
            Lang::Es => "Definir…",
        },
        Key::Autostart => match lang {
            Lang::Pt => "Iniciar com o sistema",
            Lang::En => "Start with system",
            Lang::Es => "Iniciar con el sistema",
        },
        Key::PressKeys => match lang {
            Lang::Pt => "pressione as teclas…",
            Lang::En => "press the keys…",
            Lang::Es => "pulsa las teclas…",
        },
        Key::CloseToTray => match lang {
            Lang::Pt => "Fechar minimiza pra bandeja.",
            Lang::En => "Closing minimizes to tray.",
            Lang::Es => "Cerrar minimiza a la bandeja.",
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
