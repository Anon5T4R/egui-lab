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
    Clear,
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
    // lab-keys onda 6 — importação, gerador, anexos, senha mestra e auto-type
    Import,
    ImportKdbx,
    ImportBitwarden,
    ImportCsv,
    ImportFile,
    ImportPassword,
    Imported,
    SkippedCardIdentity,
    ImportReportTitle,
    Generator,
    Generate,
    Use,
    Passphrase,
    Words,
    Separator,
    Capitalize,
    IncludeNumber,
    Length,
    Lowercase,
    Uppercase,
    Digits,
    Symbols,
    Strength0,
    Strength1,
    Strength2,
    Strength3,
    Strength4,
    ChangeMaster,
    OldPassword,
    NewPassword,
    NewPasswordConfirm,
    MasterChanged,
    Attachments,
    AddAttachment,
    SaveAttachment,
    NoAttachments,
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
    // lab-keys onda 17 — spinner dos jobs pesados
    Working,
    Unlocking,
    Importing,
    ReadingFile,
    ChangingMaster,
    AttachmentTooBig,
    // lab-keys onda 23 — URL/notas no form, export, favorito, lixeira, reveal
    Url,
    Notes,
    Favorite,
    Reveal,
    Export,
    ExportJson,
    ExportCsv,
    ExportWarning,
    Exported,
    Trash,
    TrashEmpty,
    Restore,
    Restored,
    DeleteForever,
    Deleted,
    Close,
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
    Key::Clear,
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
    Key::Import,
    Key::ImportKdbx,
    Key::ImportBitwarden,
    Key::ImportCsv,
    Key::ImportFile,
    Key::ImportPassword,
    Key::Imported,
    Key::SkippedCardIdentity,
    Key::ImportReportTitle,
    Key::Generator,
    Key::Generate,
    Key::Use,
    Key::Passphrase,
    Key::Words,
    Key::Separator,
    Key::Capitalize,
    Key::IncludeNumber,
    Key::Length,
    Key::Lowercase,
    Key::Uppercase,
    Key::Digits,
    Key::Symbols,
    Key::Strength0,
    Key::Strength1,
    Key::Strength2,
    Key::Strength3,
    Key::Strength4,
    Key::ChangeMaster,
    Key::OldPassword,
    Key::NewPassword,
    Key::NewPasswordConfirm,
    Key::MasterChanged,
    Key::Attachments,
    Key::AddAttachment,
    Key::SaveAttachment,
    Key::NoAttachments,
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
    Key::Working,
    Key::Unlocking,
    Key::Importing,
    Key::ReadingFile,
    Key::ChangingMaster,
    Key::AttachmentTooBig,
    Key::Url,
    Key::Notes,
    Key::Favorite,
    Key::Reveal,
    Key::Export,
    Key::ExportJson,
    Key::ExportCsv,
    Key::ExportWarning,
    Key::Exported,
    Key::Trash,
    Key::TrashEmpty,
    Key::Restore,
    Key::Restored,
    Key::DeleteForever,
    Key::Deleted,
    Key::Close,
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
        Key::Clear => match lang {
            Lang::Pt => "Limpar",
            Lang::En => "Clear",
            Lang::Es => "Borrar",
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
        // ── lab-keys onda 6 ───────────────────────────────────────────
        Key::Import => match lang {
            Lang::Pt => "Importar",
            Lang::En => "Import",
            Lang::Es => "Importar",
        },
        Key::ImportKdbx => "KeePass (.kdbx)",
        Key::ImportBitwarden => match lang {
            Lang::Pt => "Bitwarden (export cifrado)",
            Lang::En => "Bitwarden (encrypted export)",
            Lang::Es => "Bitwarden (exportación cifrada)",
        },
        Key::ImportCsv => "CSV (Chrome/Edge/LastPass/1Password)",
        Key::ImportFile => match lang {
            Lang::Pt => "Arquivo",
            Lang::En => "File",
            Lang::Es => "Archivo",
        },
        Key::ImportPassword => match lang {
            Lang::Pt => "Senha do arquivo",
            Lang::En => "File password",
            Lang::Es => "Contraseña del archivo",
        },
        // Só o sufixo — o número entra via format! no chamador.
        Key::Imported => match lang {
            Lang::Pt => "itens importados",
            Lang::En => "items imported",
            Lang::Es => "elementos importados",
        },
        Key::SkippedCardIdentity => match lang {
            Lang::Pt => "de cartão/identidade pulados — o lab só gerencia login/nota",
            Lang::En => "card/identity skipped — the lab only manages login/note",
            Lang::Es => "de tarjeta/identidad omitidos — el lab solo gestiona login/nota",
        },
        Key::ImportReportTitle => match lang {
            Lang::Pt => "Importação",
            Lang::En => "Import",
            Lang::Es => "Importación",
        },
        Key::Generator => match lang {
            Lang::Pt => "Gerador",
            Lang::En => "Generator",
            Lang::Es => "Generador",
        },
        Key::Generate => match lang {
            Lang::Pt => "Gerar",
            Lang::En => "Generate",
            Lang::Es => "Generar",
        },
        Key::Use => match lang {
            Lang::Pt => "Usar",
            Lang::En => "Use",
            Lang::Es => "Usar",
        },
        Key::Passphrase => match lang {
            Lang::Pt => "Passphrase",
            Lang::En => "Passphrase",
            Lang::Es => "Frase de contraseña",
        },
        Key::Words => match lang {
            Lang::Pt => "Palavras",
            Lang::En => "Words",
            Lang::Es => "Palabras",
        },
        Key::Separator => match lang {
            Lang::Pt => "Separador",
            Lang::En => "Separator",
            Lang::Es => "Separador",
        },
        Key::Capitalize => match lang {
            Lang::Pt => "Capitalizar",
            Lang::En => "Capitalize",
            Lang::Es => "Capitalizar",
        },
        Key::IncludeNumber => match lang {
            Lang::Pt => "Incluir número",
            Lang::En => "Include number",
            Lang::Es => "Incluir número",
        },
        Key::Length => match lang {
            Lang::Pt => "Comprimento",
            Lang::En => "Length",
            Lang::Es => "Longitud",
        },
        Key::Lowercase => match lang {
            Lang::Pt => "Minúsculas",
            Lang::En => "Lowercase",
            Lang::Es => "Minúsculas",
        },
        Key::Uppercase => match lang {
            Lang::Pt => "Maiúsculas",
            Lang::En => "Uppercase",
            Lang::Es => "Mayúsculas",
        },
        Key::Digits => match lang {
            Lang::Pt => "Dígitos",
            Lang::En => "Digits",
            Lang::Es => "Dígitos",
        },
        Key::Symbols => match lang {
            Lang::Pt => "Símbolos",
            Lang::En => "Symbols",
            Lang::Es => "Símbolos",
        },
        Key::Strength0 => match lang {
            Lang::Pt => "péssima",
            Lang::En => "terrible",
            Lang::Es => "pésima",
        },
        Key::Strength1 => match lang {
            Lang::Pt => "ruim",
            Lang::En => "weak",
            Lang::Es => "débil",
        },
        Key::Strength2 => match lang {
            Lang::Pt => "razoável",
            Lang::En => "fair",
            Lang::Es => "aceptable",
        },
        Key::Strength3 => match lang {
            Lang::Pt => "forte",
            Lang::En => "strong",
            Lang::Es => "fuerte",
        },
        Key::Strength4 => match lang {
            Lang::Pt => "ótima",
            Lang::En => "excellent",
            Lang::Es => "excelente",
        },
        Key::ChangeMaster => match lang {
            Lang::Pt => "Trocar senha mestra",
            Lang::En => "Change master password",
            Lang::Es => "Cambiar contraseña maestra",
        },
        Key::OldPassword => match lang {
            Lang::Pt => "Senha atual",
            Lang::En => "Current password",
            Lang::Es => "Contraseña actual",
        },
        Key::NewPassword => match lang {
            Lang::Pt => "Nova senha",
            Lang::En => "New password",
            Lang::Es => "Nueva contraseña",
        },
        Key::NewPasswordConfirm => match lang {
            Lang::Pt => "Confirmar nova senha",
            Lang::En => "Confirm new password",
            Lang::Es => "Confirmar nueva contraseña",
        },
        Key::MasterChanged => match lang {
            Lang::Pt => "senha mestra trocada",
            Lang::En => "master password changed",
            Lang::Es => "contraseña maestra cambiada",
        },
        Key::Attachments => match lang {
            Lang::Pt => "Anexos",
            Lang::En => "Attachments",
            Lang::Es => "Adjuntos",
        },
        Key::AddAttachment => match lang {
            Lang::Pt => "Adicionar anexo",
            Lang::En => "Add attachment",
            Lang::Es => "Añadir adjunto",
        },
        Key::SaveAttachment => match lang {
            Lang::Pt => "Salvar",
            Lang::En => "Save",
            Lang::Es => "Guardar",
        },
        Key::NoAttachments => match lang {
            Lang::Pt => "sem anexos",
            Lang::En => "no attachments",
            Lang::Es => "sin adjuntos",
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
        // ── lab-keys onda 17: jobs pesados (spinner) ──────────────────
        Key::Working => match lang {
            Lang::Pt => "trabalhando…",
            Lang::En => "working…",
            Lang::Es => "trabajando…",
        },
        Key::Unlocking => match lang {
            Lang::Pt => "destrancando…",
            Lang::En => "unlocking…",
            Lang::Es => "desbloqueando…",
        },
        Key::Importing => match lang {
            Lang::Pt => "importando…",
            Lang::En => "importing…",
            Lang::Es => "importando…",
        },
        Key::ReadingFile => match lang {
            Lang::Pt => "lendo arquivo…",
            Lang::En => "reading file…",
            Lang::Es => "leyendo archivo…",
        },
        Key::ChangingMaster => match lang {
            Lang::Pt => "trocando a senha mestra…",
            Lang::En => "changing the master password…",
            Lang::Es => "cambiando la contraseña maestra…",
        },
        // O limite é o do lab (o cofre inteiro é regravado a cada save).
        Key::AttachmentTooBig => match lang {
            Lang::Pt => "anexo grande demais (máx 10 MiB — o cofre inteiro é regravado a cada save)",
            Lang::En => "attachment too large (max 10 MiB — the whole vault is rewritten on every save)",
            Lang::Es => "adjunto demasiado grande (máx 10 MiB — la bóveda entera se regraba en cada guardado)",
        },
        // ── lab-keys onda 23 ─────────────────────────────────────────
        Key::Url => match lang {
            Lang::Pt => "URL",
            Lang::En => "URL",
            Lang::Es => "URL",
        },
        Key::Notes => match lang {
            Lang::Pt => "Notas",
            Lang::En => "Notes",
            Lang::Es => "Notas",
        },
        Key::Favorite => match lang {
            Lang::Pt => "Favorito",
            Lang::En => "Favorite",
            Lang::Es => "Favorito",
        },
        Key::Reveal => match lang {
            Lang::Pt => "Mostrar/ocultar",
            Lang::En => "Show/hide",
            Lang::Es => "Mostrar/ocultar",
        },
        Key::Export => match lang {
            Lang::Pt => "Exportar",
            Lang::En => "Export",
            Lang::Es => "Exportar",
        },
        Key::ExportJson => match lang {
            Lang::Pt => "JSON (cofre em claro)",
            Lang::En => "JSON (vault in the clear)",
            Lang::Es => "JSON (bóveda en claro)",
        },
        Key::ExportCsv => match lang {
            Lang::Pt => "CSV (planilha)",
            Lang::En => "CSV (spreadsheet)",
            Lang::Es => "CSV (hoja de cálculo)",
        },
        // Aviso FORTE: o export não tem cifra nenhuma.
        Key::ExportWarning => match lang {
            Lang::Pt => "ATENÇÃO: o export sai EM CLARO, sem cifra — qualquer app lê. Use só para migrar e apague o arquivo depois.",
            Lang::En => "WARNING: the export is UNENCRYPTED — any app can read it. Use it only to migrate, then delete the file.",
            Lang::Es => "ATENCIÓN: la exportación sale SIN CIFRAR — cualquier app puede leerla. Úsala solo para migrar y borra el archivo después.",
        },
        // Só o sufixo — o caminho entra via format! no chamador.
        Key::Exported => match lang {
            Lang::Pt => "exportado",
            Lang::En => "exported",
            Lang::Es => "exportado",
        },
        Key::Trash => match lang {
            Lang::Pt => "Lixeira",
            Lang::En => "Trash",
            Lang::Es => "Papelera",
        },
        Key::TrashEmpty => match lang {
            Lang::Pt => "lixeira vazia",
            Lang::En => "trash is empty",
            Lang::Es => "papelera vacía",
        },
        Key::Restore => match lang {
            Lang::Pt => "Restaurar",
            Lang::En => "Restore",
            Lang::Es => "Restaurar",
        },
        Key::Restored => match lang {
            Lang::Pt => "restaurado",
            Lang::En => "restored",
            Lang::Es => "restaurado",
        },
        Key::DeleteForever => match lang {
            Lang::Pt => "Excluir de vez",
            Lang::En => "Delete forever",
            Lang::Es => "Eliminar para siempre",
        },
        Key::Deleted => match lang {
            Lang::Pt => "excluído de vez",
            Lang::En => "deleted forever",
            Lang::Es => "eliminado para siempre",
        },
        Key::Close => match lang {
            Lang::Pt => "Fechar",
            Lang::En => "Close",
            Lang::Es => "Cerrar",
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
