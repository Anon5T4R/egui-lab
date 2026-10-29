//! Trabalho pesado FORA da thread da UI (onda 17 — mesmo desenho do lab-image/
//! lab-player): a thread faz o trabalho caro (Argon2id, PBKDF2 600k, keepass,
//! `fs::read` de arquivo em rede/OneDrive) e devolve **dados prontos** pelo
//! canal; no fim acorda a UI com `ctx.request_repaint()`.
//!
//! Regra do módulo: a thread nunca toca o estado do app (`&mut KeysApp`) — o
//! `JobResult` carrega só o que o `update` precisa pra adotar (sessão, JSON do
//! cofre, itens do import…). Sem lock, sem corrida com o frame.
//!
//! O que NÃO vem pra cá (e por quê): `atomic_write` (gravar o cofre já cifrado
//! são poucos KB e o retorno precisa decidir o estado na hora — falhou a
//! gravação, o app NÃO muda) e o desbloqueio rápido do boot
//! (`crypto::open_with_key` não roda KDF: é AEAD de poucos KB).

use std::sync::mpsc::{Receiver, Sender};

use zeroize::Zeroizing;

use crate::{bitwarden_encrypted, bwdecrypt, crypto, import, kdbx, mime_by_ext, vault, ImportKind};

/// Cap de anexo do lab: 10 MiB. O cofre INTEIRO é regravado (e recifrado) a
/// cada save e o anexo viaja em base64 dentro do JSON — um anexo grande infla
/// toda gravação. O LocalKeys oficial não tem esse limite; aqui é decisão do
/// lab (o limite é checado por `metadata()` ANTES de ler o arquivo).
pub const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;

/// O que rodar na thread. Tudo que é segredo viaja dentro de `Zeroizing`.
pub enum Job {
    Unlock {
        path: String,
        password: Zeroizing<String>,
        /// Estado do checkbox "desbloqueio rápido" no momento do clique.
        quick: bool,
    },
    Create {
        path: String,
        password: Zeroizing<String>,
        quick: bool,
    },
    ChangeMaster {
        path: String,
        old: Zeroizing<String>,
        new: Zeroizing<String>,
    },
    Import {
        kind: ImportKind,
        path: String,
        password: Zeroizing<String>,
    },
    ReadAttachment {
        item_id: String,
        source: String,
    },
    SaveAttachment {
        target: String,
        data_b64: String,
    },
    /// Export em claro (JSON/CSV): o texto já está pronto, mas o destino pode
    /// ser rede/OneDrive — gravar aqui evita travar a janela.
    WriteExport {
        target: String,
        text: String,
    },
}

/// Cofre destrancado pronto pra `adopt` — nada de estado do app aqui.
pub struct Unlocked {
    pub path: String,
    pub raw: serde_json::Value,
    pub session: crypto::SessionKey,
    pub quick: bool,
}

/// Cofre novo: o `.tkeys` já cifrado (a gravação fica na UI).
pub struct Created {
    pub path: String,
    pub file: Vec<u8>,
    pub raw: serde_json::Value,
    pub session: crypto::SessionKey,
    pub quick: bool,
}

/// Cofre recifrado com a senha nova (a gravação fica na UI).
pub struct Renewed {
    pub path: String,
    pub file: Vec<u8>,
}

/// Itens já convertidos pro schema do vault — `vault::push_items` aceita.
pub struct Imported {
    pub items: Vec<serde_json::Value>,
    pub skipped: usize,
}

/// Anexo lido e codificado, pronto pro `vault::add_attachment`.
pub struct AttachmentRead {
    pub item_id: String,
    pub name: String,
    pub mime: &'static str,
    pub size: usize,
    pub data_b64: String,
}

/// Erro de job. A thread não conhece o idioma: o que é traduzível volta como
/// variante e a UI resolve com a chave i18n.
pub enum JobError {
    /// Mensagem pronta em PT (leitura/parse/gravação), como no resto do app.
    Msg(String),
    /// Senha errada / arquivo adulterado → `Key::WrongPassword`.
    WrongPassword,
    /// Anexo acima do cap → `Key::AttachmentTooBig`.
    AttachmentTooBig { name: String },
}

impl From<String> for JobError {
    fn from(msg: String) -> Self {
        JobError::Msg(msg)
    }
}

/// Resultado de um job — exatamente um por job.
pub enum JobResult {
    Unlocked(Result<Unlocked, JobError>),
    Created(Result<Created, JobError>),
    MasterChanged(Result<Renewed, JobError>),
    Imported(Result<Imported, JobError>),
    AttachmentRead(Result<AttachmentRead, JobError>),
    /// `Ok(destino)` quando o anexo foi gravado.
    AttachmentSaved(Result<String, JobError>),
    /// `Ok(destino)` quando o export foi gravado.
    Exported(Result<String, JobError>),
}

/// Sobe a thread do job. `ctx` é um clone do `egui::Context` (Send+Sync): o
/// `request_repaint` no fim do trabalho é o que faz a UI acordar sem input.
pub fn spawn(job: Job, tx: Sender<JobResult>, ctx: egui::Context) {
    let spawned = std::thread::Builder::new()
        .name("keys-job".into())
        .spawn(move || {
            let result = run(job);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    if let Err(e) = spawned {
        // Sem thread o `tx` morre junto: o `drain` vê o canal fechado e limpa
        // o spinner (o app não fica girando pra sempre).
        eprintln!("job: não deu para subir a thread: {e}");
    }
}

/// Drena o canal sem bloquear. Devolve os resultados prontos e se o canal já
/// fechou (thread terminou — normalmente depois de mandar o resultado).
pub fn drain(rx: &Receiver<JobResult>) -> (Vec<JobResult>, bool) {
    let mut out = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(result) => out.push(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => return (out, false),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return (out, true),
        }
    }
}

fn run(job: Job) -> JobResult {
    match job {
        Job::Unlock {
            path,
            password,
            quick,
        } => JobResult::Unlocked(unlock(&path, &password, quick)),
        Job::Create {
            path,
            password,
            quick,
        } => JobResult::Created(create(&path, &password, quick)),
        Job::ChangeMaster { path, old, new } => {
            JobResult::MasterChanged(change_master(&path, &old, &new))
        }
        Job::Import {
            kind,
            path,
            password,
        } => JobResult::Imported(import(&kind, &path, &password)),
        Job::ReadAttachment { item_id, source } => {
            JobResult::AttachmentRead(read_attachment(item_id, &source))
        }
        Job::SaveAttachment { target, data_b64 } => {
            JobResult::AttachmentSaved(save_attachment(target, &data_b64))
        }
        Job::WriteExport { target, text } => JobResult::Exported(write_export(target, &text)),
    }
}

fn crypto_err(e: crypto::CryptoError) -> JobError {
    match e {
        crypto::CryptoError::Decrypt => JobError::WrongPassword,
        other => JobError::Msg(other.to_string()),
    }
}

/// Lê o arquivo AQUI (OneDrive/pasta de rede trava igual ao Explorer) e roda o
/// Argon2id — os ~300 ms que antes congelavam a janela.
fn unlock(path: &str, password: &str, quick: bool) -> Result<Unlocked, JobError> {
    let file = std::fs::read(path).map_err(|e| JobError::Msg(format!("{path}: {e}")))?;
    let (plaintext, session) = crypto::open_vault(password, &file).map_err(crypto_err)?;
    let raw = serde_json::from_slice::<serde_json::Value>(&plaintext)
        .map_err(|_| JobError::Msg("vault não é JSON válido".into()))?;
    Ok(Unlocked {
        path: path.to_string(),
        raw,
        session,
        quick,
    })
}

/// Deriva a chave e devolve o `.tkeys` cifrado pronto; a gravação atômica fica
/// no retorno (poucos KB — e é ela que decide se o app adota o cofre novo).
fn create(path: &str, password: &str, quick: bool) -> Result<Created, JobError> {
    let (file, session) =
        crypto::create_vault(password, vault::EMPTY_VAULT.as_bytes()).map_err(crypto_err)?;
    let raw = serde_json::from_str::<serde_json::Value>(vault::EMPTY_VAULT)
        .map_err(|e| JobError::Msg(format!("semente do cofre inválida: {e}")))?;
    Ok(Created {
        path: path.to_string(),
        file,
        raw,
        session,
        quick,
    })
}

/// Argon2id 2× (decifra com a antiga, recifra com a nova): ~600 ms fora da UI.
fn change_master(path: &str, old: &str, new: &str) -> Result<Renewed, JobError> {
    if new.is_empty() {
        return Err(JobError::Msg(
            "a nova master password não pode ser vazia".into(),
        ));
    }
    let file =
        std::fs::read(path).map_err(|e| JobError::Msg(format!("falha ao ler '{path}': {e}")))?;
    let renewed = crypto::change_password(old, new, &file).map_err(crypto_err)?;
    Ok(Renewed {
        path: path.to_string(),
        file: renewed,
    })
}

/// Lê e decifra/parseia o arquivo escolhido. É o job mais caro: PBKDF2 com
/// 600k iterações (Bitwarden) e o Argon2id interno do keepass levam segundos.
fn import(kind: &ImportKind, path: &str, password: &str) -> Result<Imported, JobError> {
    let (items, skipped) = match kind {
        ImportKind::Kdbx => {
            kdbx::import(path, password).map(|entries| (import::kdbx_to_items(&entries), 0))?
        }
        ImportKind::Bitwarden => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("falha ao ler '{path}': {e}"))?;
            // Export cifrado: decifra ANTES de mapear (o parse espera o mesmo
            // JSON claro do export não-cifrado).
            let clear = if bitwarden_encrypted(&text) {
                bwdecrypt::decrypt_export(&text, password)?
            } else {
                text
            };
            let report = import::parse_bitwarden_json(&clear)?;
            (report.items, report.skipped_card_identity)
        }
        ImportKind::Csv => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("falha ao ler '{path}': {e}"))?;
            (import::parse_csv_items(&text), 0)
        }
    };
    Ok(Imported { items, skipped })
}

/// `metadata()` antes de ler: um anexo de 2 GB é recusado sem tocar o conteúdo.
fn read_attachment(item_id: String, source: &str) -> Result<AttachmentRead, JobError> {
    use base64::Engine as _;

    let name = file_name_of(source);
    let meta = std::fs::metadata(source)
        .map_err(|e| JobError::Msg(format!("falha ao ler '{source}': {e}")))?;
    if too_big(meta.len()) {
        return Err(JobError::AttachmentTooBig { name });
    }
    let bytes = std::fs::read(source)
        .map_err(|e| JobError::Msg(format!("falha ao ler '{source}': {e}")))?;
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(AttachmentRead {
        item_id,
        name,
        mime: mime_by_ext(source),
        size: bytes.len(),
        data_b64,
    })
}

/// Decodifica o base64 e grava — `fs::write` em pasta de rede também trava.
fn save_attachment(target: String, data_b64: &str) -> Result<String, JobError> {
    use base64::Engine as _;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|_| JobError::Msg("anexo com base64 inválido".into()))?;
    std::fs::write(&target, &bytes)
        .map_err(|e| JobError::Msg(format!("falha ao gravar '{target}': {e}")))?;
    Ok(target)
}

/// Grava o texto do export (já pronto) no destino escolhido.
fn write_export(target: String, text: &str) -> Result<String, JobError> {
    std::fs::write(&target, text)
        .map_err(|e| JobError::Msg(format!("falha ao gravar '{target}': {e}")))?;
    Ok(target)
}

/// Nome do arquivo pro anexo (o caminho inteiro não vai pro cofre).
fn file_name_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| "anexo".to_string())
}

/// Regra do cap, isolada pra ter teste sem tocar em disco.
fn too_big(len: u64) -> bool {
    len > MAX_ATTACHMENT_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A adoção vive no `KeysApp` (precisa de janela/eframe), então o que se
    /// testa aqui é o mecanismo puro: o canal entrega o resultado, sinaliza o
    /// fim e a thread do job realmente devolve dados prontos.
    #[test]
    fn drain_entrega_o_resultado_e_sinaliza_o_fim() {
        let (tx, rx) = std::sync::mpsc::channel();
        let (done, closed) = drain(&rx);
        assert!(done.is_empty());
        assert!(!closed);

        let sent = tx.send(JobResult::MasterChanged(Ok(Renewed {
            path: "cofre.tkeys".into(),
            file: vec![1, 2, 3],
        })));
        assert!(sent.is_ok());

        let (done, closed) = drain(&rx);
        assert_eq!(done.len(), 1);
        assert!(!closed);

        drop(tx);
        let (done, closed) = drain(&rx);
        assert!(done.is_empty());
        assert!(
            closed,
            "canal sem remetente tem de ser reportado como fechado"
        );
    }

    #[test]
    fn spawn_devolve_o_resultado_pelo_canal() {
        let dir = std::env::temp_dir().join(format!("lab-keys-job-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let target = dir.join("anexo.bin");
        let target_str = target.display().to_string();

        let (tx, rx) = std::sync::mpsc::channel();
        spawn(
            Job::SaveAttachment {
                target: target_str.clone(),
                data_b64: "aGk=".into(), // "hi"
            },
            tx,
            egui::Context::default(),
        );
        let result = rx.recv_timeout(std::time::Duration::from_secs(10));
        match result {
            Ok(JobResult::AttachmentSaved(Ok(written))) => assert_eq!(written, target_str),
            Ok(_) => panic!("job devolveu a variante errada"),
            Err(e) => panic!("job não respondeu: {e}"),
        }
        assert_eq!(
            std::fs::read(&target).ok().as_deref(),
            Some(b"hi".as_slice())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cap_de_anexo_aceita_o_limite_e_recusa_acima() {
        assert!(!too_big(0));
        assert!(!too_big(MAX_ATTACHMENT_BYTES));
        assert!(too_big(MAX_ATTACHMENT_BYTES + 1));
    }

    #[test]
    fn anexo_pequeno_e_lido_e_codificado() {
        let dir = std::env::temp_dir().join(format!("lab-keys-anexo-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("nota.txt");
        assert!(std::fs::write(&src, b"oi").is_ok());

        match read_attachment("item-1".into(), &src.display().to_string()) {
            Ok(read) => {
                assert_eq!(read.item_id, "item-1");
                assert_eq!(read.name, "nota.txt");
                assert_eq!(read.mime, "text/plain");
                assert_eq!(read.size, 2);
                assert_eq!(read.data_b64, "b2k=");
            }
            Err(_) => panic!("anexo pequeno deveria ser lido"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
