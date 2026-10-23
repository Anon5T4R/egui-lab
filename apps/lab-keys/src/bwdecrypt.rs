//! ⚠️ COPIADO VERBATIM de `LocalKeys · src-tauri/src/bwdecrypt.rs` (repo da
//! suíte). Único ajuste: nenhum — a UI da onda 6 consome a API inteira.
//!
//! Import do export **cifrado** (password-protected) do Bitwarden.
//!
//! **Validado com export real em 2026-07-18 (teste do João).** É decifragem apenas (import),
//! então pior caso ele falha — nunca corrompe o seu cofre. O algoritmo segue o
//! `bitwarden/clients` (GPL): deriva a chave da senha (PBKDF2 ou Argon2id), estica
//! via HKDF em (encKey, macKey), e a `data` é uma enc-string `2.iv|ct|mac`
//! (AES-256-CBC + HMAC-SHA256). Só primitivas auditadas do RustCrypto.

use aes::Aes256;
use base64::Engine;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;
type Aes256CbcDec = cbc::Decryptor<Aes256>;

#[derive(Deserialize)]
struct Export {
    encrypted: Option<bool>,
    #[serde(rename = "passwordProtected")]
    password_protected: Option<bool>,
    salt: Option<String>,
    #[serde(rename = "kdfType")]
    kdf_type: Option<u32>,
    #[serde(rename = "kdfIterations")]
    kdf_iterations: Option<u32>,
    #[serde(rename = "kdfMemory")]
    kdf_memory: Option<u32>,
    #[serde(rename = "kdfParallelism")]
    kdf_parallelism: Option<u32>,
    data: Option<String>,
}

fn b64(s: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|_| "base64 inválido".to_string())
}

/// Deriva a chave-mestra (32 bytes) do password + salt conforme o KDF do export.
fn derive_master(
    password: &str,
    salt: &str,
    kdf_type: u32,
    iters: u32,
    mem_mib: u32,
    parallelism: u32,
) -> Result<Zeroizing<[u8; 32]>, String> {
    let mut key = Zeroizing::new([0u8; 32]);
    match kdf_type {
        0 => {
            // PBKDF2-HMAC-SHA256; salt = bytes da string.
            pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt.as_bytes(), iters, key.as_mut_slice());
        }
        1 => {
            // Argon2id; o Bitwarden usa salt = SHA256(salt).
            let salt_hash = Sha256::digest(salt.as_bytes());
            let params = argon2::Params::new(mem_mib * 1024, iters, parallelism.max(1), Some(32))
                .map_err(|_| "parâmetros Argon2 inválidos".to_string())?;
            argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
                .hash_password_into(password.as_bytes(), &salt_hash, key.as_mut_slice())
                .map_err(|_| "Argon2 falhou".to_string())?;
        }
        other => return Err(format!("KDF do Bitwarden não suportado: {other}")),
    }
    Ok(key)
}

/// Estica a chave-mestra em (encKey 32, macKey 32) via HKDF-Expand-SHA256.
fn stretch(master: &[u8; 32]) -> Result<([u8; 32], [u8; 32]), String> {
    let hk = Hkdf::<Sha256>::from_prk(master).map_err(|_| "HKDF prk".to_string())?;
    let mut enc = [0u8; 32];
    let mut mac = [0u8; 32];
    hk.expand(b"enc", &mut enc).map_err(|_| "HKDF enc".to_string())?;
    hk.expand(b"mac", &mut mac).map_err(|_| "HKDF mac".to_string())?;
    Ok((enc, mac))
}

/// Decifra uma enc-string `2.iv|ct|mac` (AesCbc256_HmacSha256_B64).
fn decrypt_enc_string(s: &str, enc_key: &[u8; 32], mac_key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let rest = s
        .strip_prefix("2.")
        .ok_or("formato de cifra não suportado (esperado tipo 2)")?;
    let parts: Vec<&str> = rest.split('|').collect();
    if parts.len() != 3 {
        return Err("enc-string malformada".into());
    }
    let iv = b64(parts[0])?;
    let ct = b64(parts[1])?;
    let tag = b64(parts[2])?;
    if iv.len() != 16 {
        return Err("IV inválido".into());
    }
    // Autentica (iv || ct) ANTES de decifrar.
    let mut h = <HmacSha256 as Mac>::new_from_slice(mac_key).map_err(|_| "chave MAC".to_string())?;
    h.update(&iv);
    h.update(&ct);
    h.verify_slice(&tag)
        .map_err(|_| "senha incorreta ou arquivo corrompido (MAC)".to_string())?;

    Aes256CbcDec::new_from_slices(enc_key, &iv)
        .map_err(|_| "chave/IV inválidos".to_string())?
        .decrypt_padded_vec_mut::<Pkcs7>(&ct)
        .map_err(|_| "decifragem falhou".to_string())
}

/// Recebe o JSON do export cifrado + a senha; devolve o JSON claro (mesmo formato
/// do export não-cifrado do Bitwarden), pronto para o parser comum.
pub fn decrypt_export(json: &str, password: &str) -> Result<String, String> {
    let export: Export = serde_json::from_str(json).map_err(|e| format!("JSON inválido: {e}"))?;
    if export.password_protected != Some(true) && export.encrypted != Some(true) {
        return Err("este arquivo não parece um export cifrado do Bitwarden".into());
    }
    let salt = export.salt.ok_or("export sem `salt`")?;
    let data = export.data.ok_or("export sem `data`")?;
    let kdf_type = export.kdf_type.unwrap_or(0);
    let iters = export.kdf_iterations.ok_or("export sem `kdfIterations`")?;
    let mem = export.kdf_memory.unwrap_or(64);
    let par = export.kdf_parallelism.unwrap_or(4);

    let master = derive_master(password, &salt, kdf_type, iters, mem, par)?;
    let (enc_key, mac_key) = stretch(&master)?;
    let plaintext = decrypt_enc_string(&data, &enc_key, &mac_key)?;
    String::from_utf8(plaintext).map_err(|_| "conteúdo decifrado não é UTF-8".to_string())
}
