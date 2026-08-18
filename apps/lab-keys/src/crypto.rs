//! ⚠️ ARQUIVO COPIADO VERBATIM de `LocalKeys@0.9.0 · src-tauri/src/crypto.rs`
//! (repo da suíte, MIT). É a PROVA de reuso do lab: o núcleo criptográfico do
//! cofre `.tkeys` — zero Tauri, zero mudança — rodando sob UI egui. Qualquer
//! diferença de comportamento entre LocalKeys e lab-keys NÃO pode estar aqui.
//!
//! Núcleo criptográfico do LocalKeys — o formato de arquivo `.tkeys`.
//!
//! Regra absoluta (ver `SECURITY.md`): **nenhuma primitiva feita em casa**. Aqui
//! só compomos crates auditados do RustCrypto, que já trazem os vetores de teste
//! oficiais (IETF/RFC) nas suas próprias suítes:
//!
//! - **KDF:** Argon2id (`argon2`) transforma a master password + salt numa chave
//!   simétrica de 32 bytes. Os parâmetros (memória/iterações/paralelismo) ficam no
//!   header **em claro**, então um vault antigo abre mesmo se mudarmos o default.
//! - **Cifra:** XChaCha20-Poly1305 (`chacha20poly1305`), AEAD com nonce de 192 bits
//!   (colisão de nonce aleatório é improvável). O **header inteiro entra como AAD**,
//!   então adulterar os parâmetros do KDF, o salt ou o nonce invalida a autenticação.
//!
//! Layout do arquivo (tudo little-endian):
//! ```text
//!   MAGIC "TKEYS\0"  (6)  ─┐
//!   format_version   (1)   │
//!   kdf_id           (1)   ├─ HEADER (60 bytes) — em claro, usado como AAD
//!   m_cost u32       (4)   │
//!   t_cost u32       (4)   │
//!   p_cost u32       (4)   │
//!   salt             (16)  │
//!   nonce            (24) ─┘
//!   ciphertext + tag (n)      ← XChaCha20-Poly1305(JSON do vault), tag de 16 no fim
//! ```
//! A chave derivada nunca toca o disco nem o front-end; vive só no back-end e é
//! apagada da memória (`Zeroizing`) assim que sai de escopo.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use zeroize::Zeroizing;

const MAGIC: [u8; 6] = *b"TKEYS\x00";
const FORMAT_VERSION: u8 = 1;
const KDF_ARGON2ID: u8 = 1;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
const HEADER_LEN: usize = 6 + 1 + 1 + 4 + 4 + 4 + SALT_LEN + NONCE_LEN; // = 60

/// Parâmetros default do Argon2id, calibrados para ~250–500 ms na máquina alvo.
/// 64 MiB de memória é bem acima do recomendado pelo OWASP para uso interativo.
pub const DEFAULT_M_COST: u32 = 65_536; // KiB → 64 MiB
pub const DEFAULT_T_COST: u32 = 3; // iterações
pub const DEFAULT_P_COST: u32 = 1; // lanes (1 = determinístico, sem threads)

#[derive(Debug, PartialEq, Eq)]
pub enum CryptoError {
    /// Não é um arquivo `.tkeys` (magic errado ou truncado).
    BadFormat,
    /// Versão de formato que este build não conhece.
    UnsupportedVersion(u8),
    /// Falha ao derivar a chave (parâmetros de Argon2 inválidos).
    Kdf,
    /// Autenticação falhou. **Proposital: não distingue** senha errada de arquivo
    /// adulterado — os dois caem aqui para não vazar informação.
    Decrypt,
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::BadFormat => write!(f, "arquivo não é um vault .tkeys válido"),
            CryptoError::UnsupportedVersion(v) => {
                write!(f, "versão de formato .tkeys não suportada: {v}")
            }
            CryptoError::Kdf => write!(f, "falha ao derivar a chave (KDF)"),
            CryptoError::Decrypt => {
                write!(f, "senha incorreta ou arquivo corrompido/adulterado")
            }
        }
    }
}

impl std::error::Error for CryptoError {}

/// Parâmetros do KDF de um vault. Ficam no header em claro para que um vault
/// criado com params antigos continue abrindo depois de mudarmos o default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        KdfParams {
            m_cost: DEFAULT_M_COST,
            t_cost: DEFAULT_T_COST,
            p_cost: DEFAULT_P_COST,
        }
    }
}

/// Deriva a chave de 32 bytes da master password com Argon2id.
/// O retorno é `Zeroizing`: a chave é apagada da memória ao sair de escopo.
fn derive_key(
    password: &[u8],
    salt: &[u8],
    params: KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>, CryptoError> {
    let p = Params::new(params.m_cost, params.t_cost, params.p_cost, Some(KEY_LEN))
        .map_err(|_| CryptoError::Kdf)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, p);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password, salt, key.as_mut_slice())
        .map_err(|_| CryptoError::Kdf)?;
    Ok(key)
}

fn build_header(params: KdfParams, salt: &[u8; SALT_LEN], nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(&MAGIC);
    h.push(FORMAT_VERSION);
    h.push(KDF_ARGON2ID);
    h.extend_from_slice(&params.m_cost.to_le_bytes());
    h.extend_from_slice(&params.t_cost.to_le_bytes());
    h.extend_from_slice(&params.p_cost.to_le_bytes());
    h.extend_from_slice(salt);
    h.extend_from_slice(nonce);
    debug_assert_eq!(h.len(), HEADER_LEN);
    h
}

/// Chave de sessão: a chave derivada + o salt/params do vault aberto. Vive só no
/// back-end enquanto o vault está destrancado e é apagada da memória ao trancar
/// (a chave está sob `Zeroizing`). Permite **recifrar (salvar) sem re-rodar o
/// Argon2 nem guardar a master password** — só sorteia um nonce novo a cada save
/// (nonce único por chave é o que o XChaCha20-Poly1305 exige).
pub struct SessionKey {
    key: Zeroizing<[u8; KEY_LEN]>,
    salt: [u8; SALT_LEN],
    params: KdfParams,
}

impl SessionKey {
    /// Recifra o `plaintext` com a chave da sessão e um nonce novo.
    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        seal_bytes(&self.key, &self.salt, self.params, plaintext)
    }

    /// Cópia da chave bruta (32 bytes) — **só** para guardar no cofre do SO
    /// (keyring) no desbloqueio rápido opt-in. Trate como segredo.
    pub fn key_bytes(&self) -> [u8; KEY_LEN] {
        *self.key
    }
}

/// Lê os campos do header (magic/versão/kdf/params/salt/nonce) de um `.tkeys`.
fn parse_header(file: &[u8]) -> Result<(KdfParams, [u8; SALT_LEN], [u8; NONCE_LEN]), CryptoError> {
    if file.len() < HEADER_LEN || file[0..6] != MAGIC {
        return Err(CryptoError::BadFormat);
    }
    let version = file[6];
    if version != FORMAT_VERSION {
        return Err(CryptoError::UnsupportedVersion(version));
    }
    if file[7] != KDF_ARGON2ID {
        return Err(CryptoError::BadFormat);
    }
    let params = KdfParams {
        m_cost: u32::from_le_bytes(file[8..12].try_into().unwrap()),
        t_cost: u32::from_le_bytes(file[12..16].try_into().unwrap()),
        p_cost: u32::from_le_bytes(file[16..20].try_into().unwrap()),
    };
    let salt: [u8; SALT_LEN] = file[20..36].try_into().unwrap();
    let nonce: [u8; NONCE_LEN] = file[36..60].try_into().unwrap();
    Ok((params, salt, nonce))
}

/// Decifra um `.tkeys` com a **chave bruta** (sem re-rodar o Argon2) — usado pelo
/// desbloqueio rápido, que guarda a chave no cofre do SO em vez da master password.
pub fn open_with_key(
    raw_key: &[u8; KEY_LEN],
    file: &[u8],
) -> Result<(Zeroizing<Vec<u8>>, SessionKey), CryptoError> {
    let (params, salt, nonce) = parse_header(file)?;
    let header = &file[0..HEADER_LEN];
    let ciphertext = &file[HEADER_LEN..];
    let cipher = XChaCha20Poly1305::new(Key::from_slice(raw_key));
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: ciphertext,
                aad: header,
            },
        )
        .map_err(|_| CryptoError::Decrypt)?;
    Ok((
        Zeroizing::new(plaintext),
        SessionKey {
            key: Zeroizing::new(*raw_key),
            salt,
            params,
        },
    ))
}

/// Passo comum de cifragem: sorteia nonce, monta o header (que vira AAD) e roda
/// o AEAD. Devolve `header || ciphertext+tag`.
fn seal_bytes(
    key: &[u8; KEY_LEN],
    salt: &[u8; SALT_LEN],
    params: KdfParams,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let header = build_header(params, salt, &nonce);

    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &header,
            },
        )
        .map_err(|_| CryptoError::Decrypt)?;

    let mut out = header;
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Cria um vault novo: deriva a chave da `password`, cifra o `plaintext` inicial
/// e devolve o arquivo + a `SessionKey` (o vault já fica "destrancado").
pub fn create_vault(
    password: &str,
    plaintext: &[u8],
) -> Result<(Vec<u8>, SessionKey), CryptoError> {
    create_vault_with(password, plaintext, KdfParams::default())
}

fn create_vault_with(
    password: &str,
    plaintext: &[u8],
    params: KdfParams,
) -> Result<(Vec<u8>, SessionKey), CryptoError> {
    let mut salt = [0u8; SALT_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let key = derive_key(password.as_bytes(), &salt, params)?;
    let session = SessionKey { key, salt, params };
    let file = session.seal(plaintext)?;
    Ok((file, session))
}

/// Cifra avulsa (sem sessão) — usada só nos testes e onde não há sessão viva.
#[cfg(test)]
fn encrypt_vault_with(
    password: &str,
    plaintext: &[u8],
    params: KdfParams,
) -> Result<Vec<u8>, CryptoError> {
    Ok(create_vault_with(password, plaintext, params)?.0)
}

/// Abre um arquivo `.tkeys`: valida o header, deriva a chave e decifra. Devolve o
/// plaintext (`Zeroizing`, apagado ao sair de escopo) e a `SessionKey` para salvar
/// depois. Falha **genérica** em senha errada OU adulteração (não distingue).
pub fn open_vault(
    password: &str,
    file: &[u8],
) -> Result<(Zeroizing<Vec<u8>>, SessionKey), CryptoError> {
    if file.len() < HEADER_LEN || file[0..6] != MAGIC {
        return Err(CryptoError::BadFormat);
    }
    let version = file[6];
    if version != FORMAT_VERSION {
        return Err(CryptoError::UnsupportedVersion(version));
    }
    let kdf_id = file[7];
    if kdf_id != KDF_ARGON2ID {
        return Err(CryptoError::BadFormat);
    }
    let params = KdfParams {
        m_cost: u32::from_le_bytes(file[8..12].try_into().unwrap()),
        t_cost: u32::from_le_bytes(file[12..16].try_into().unwrap()),
        p_cost: u32::from_le_bytes(file[16..20].try_into().unwrap()),
    };
    let salt: [u8; SALT_LEN] = file[20..36].try_into().unwrap();
    let nonce: [u8; NONCE_LEN] = file[36..60].try_into().unwrap();

    // O header como está no arquivo é o AAD — qualquer bit trocado no header
    // (params, salt, nonce) quebra a autenticação abaixo.
    let header = &file[0..HEADER_LEN];
    let ciphertext = &file[HEADER_LEN..];

    let key = derive_key(password.as_bytes(), &salt, params)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key.as_slice()));
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: ciphertext,
                aad: header,
            },
        )
        .map_err(|_| CryptoError::Decrypt)?;

    Ok((Zeroizing::new(plaintext), SessionKey { key, salt, params }))
}

/// Decifra e devolve só o plaintext (descarta a sessão). Conveniência para testes
/// e para a troca de senha.
pub fn decrypt_vault(password: &str, file: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    Ok(open_vault(password, file)?.0)
}

/// Troca a master password: decifra com a antiga e recifra com a nova (salt e
/// nonce novos). Não expõe o plaintext ao chamador.
pub fn change_password(
    old_password: &str,
    new_password: &str,
    file: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let plaintext = decrypt_vault(old_password, file)?;
    Ok(create_vault(new_password, &plaintext)?.0)
}

/// Lê os parâmetros de KDF de um vault sem decifrar (para exibir/telemetria local).
pub fn read_kdf_params(file: &[u8]) -> Result<KdfParams, CryptoError> {
    if file.len() < HEADER_LEN || file[0..6] != MAGIC {
        return Err(CryptoError::BadFormat);
    }
    Ok(KdfParams {
        m_cost: u32::from_le_bytes(file[8..12].try_into().unwrap()),
        t_cost: u32::from_le_bytes(file[12..16].try_into().unwrap()),
        p_cost: u32::from_le_bytes(file[16..20].try_into().unwrap()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Params baratos só para os testes rodarem rápido no CI (o KDF real usa 64 MiB).
    const TEST_PARAMS: KdfParams = KdfParams {
        m_cost: 512,
        t_cost: 1,
        p_cost: 1,
    };

    fn enc(pw: &str, pt: &[u8]) -> Vec<u8> {
        encrypt_vault_with(pw, pt, TEST_PARAMS).unwrap()
    }

    #[test]
    fn roundtrip_recupera_o_plaintext() {
        let pt = br#"{"items":[{"name":"gmail","password":"hunter2"}]}"#;
        let file = enc("senha-mestra-correta", pt);
        let out = decrypt_vault("senha-mestra-correta", &file).unwrap();
        assert_eq!(out.as_slice(), pt);
    }

    #[test]
    fn senha_errada_falha() {
        let file = enc("certa", b"segredo");
        assert_eq!(
            decrypt_vault("errada", &file).unwrap_err(),
            CryptoError::Decrypt
        );
    }

    #[test]
    fn blob_adulterado_falha() {
        let mut file = enc("pw", b"segredo importante");
        let last = file.len() - 1;
        file[last] ^= 0x01; // vira um bit do tag/ciphertext
        assert_eq!(decrypt_vault("pw", &file).unwrap_err(), CryptoError::Decrypt);
    }

    #[test]
    fn header_adulterado_falha() {
        // Trocar um byte dos params do KDF (offset 8 = m_cost) deve quebrar a
        // autenticação: o header é AAD, então o AEAD rejeita.
        let mut file = enc("pw", b"segredo");
        file[8] ^= 0x01;
        assert_eq!(decrypt_vault("pw", &file).unwrap_err(), CryptoError::Decrypt);
    }

    #[test]
    fn salt_adulterado_falha() {
        let mut file = enc("pw", b"segredo");
        file[20] ^= 0x01; // 1º byte do salt
        assert_eq!(decrypt_vault("pw", &file).unwrap_err(), CryptoError::Decrypt);
    }

    #[test]
    fn arquivo_nao_tkeys_falha() {
        assert_eq!(
            decrypt_vault("pw", b"isto nao eh um vault").unwrap_err(),
            CryptoError::BadFormat
        );
    }

    #[test]
    fn kdf_deterministico() {
        let salt = [7u8; SALT_LEN];
        let a = derive_key(b"mesma senha", &salt, TEST_PARAMS).unwrap();
        let b = derive_key(b"mesma senha", &salt, TEST_PARAMS).unwrap();
        assert_eq!(a.as_slice(), b.as_slice());
    }

    #[test]
    fn kdf_sensivel_ao_salt() {
        let a = derive_key(b"pw", &[1u8; SALT_LEN], TEST_PARAMS).unwrap();
        let b = derive_key(b"pw", &[2u8; SALT_LEN], TEST_PARAMS).unwrap();
        assert_ne!(a.as_slice(), b.as_slice());
    }

    #[test]
    fn nonces_sao_unicos_por_cifragem() {
        // Duas cifragens do MESMO plaintext com a MESMA senha têm de gerar saídas
        // diferentes (nonce aleatório) — senão vazaríamos igualdade de conteúdo.
        let a = enc("pw", b"identico");
        let b = enc("pw", b"identico");
        assert_ne!(a[36..60], b[36..60], "nonces deveriam diferir");
        assert_ne!(a, b);
    }

    #[test]
    fn troca_de_senha_reencaminha_o_conteudo() {
        let file = encrypt_vault_with("velha", b"dados", TEST_PARAMS).unwrap();
        let renewed = change_password("velha", "nova", &file).unwrap();
        assert_eq!(
            decrypt_vault("velha", &renewed).unwrap_err(),
            CryptoError::Decrypt
        );
        let out = decrypt_vault("nova", &renewed).unwrap();
        assert_eq!(out.as_slice(), b"dados");
    }

    #[test]
    fn header_tem_60_bytes() {
        let file = enc("pw", b"x");
        assert!(file.len() > HEADER_LEN);
        assert_eq!(&file[0..6], b"TKEYS\x00");
        assert_eq!(read_kdf_params(&file).unwrap(), TEST_PARAMS);
    }
}
