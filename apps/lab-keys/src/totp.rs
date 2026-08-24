//! ⚠️ COPIADO VERBATIM de `LocalKeys@0.9.0 · src-tauri/src/totp.rs` (MIT),
//! como o crypto.rs — segunda prova de reuso de back-end puro.
//!
//! TOTP (RFC 6238) — os códigos de 6 dígitos que trocam a cada 30 s.
//!
//! Usamos o crate auditado `totp-rs` (nada de implementar HMAC na mão). A chave
//! secreta é base32 (o que apps como Google Authenticator dão); guardamos ela no
//! item de login e geramos o código sob demanda, sempre no back-end.

// Mantido verbatim de propósito: campos/structs da API original que o LAB usa
// parcialmente não podem gerar warnings — a regra aqui é "idêntico ao original".
#![allow(dead_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use totp_rs::{Algorithm, Secret, TOTP};

const PERIOD: u64 = 30;
const DIGITS: usize = 6;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TotpCode {
    /// Código atual (6 dígitos, como string para preservar zeros à esquerda).
    pub code: String,
    /// Passo em segundos (30).
    pub period: u64,
    /// Segundos restantes até o código trocar.
    pub seconds_remaining: u64,
}

fn build(secret_b32: &str) -> Result<TOTP, String> {
    let cleaned = secret_b32.trim().replace([' ', '-'], "").to_uppercase();
    if cleaned.is_empty() {
        return Err("chave TOTP vazia".into());
    }
    let bytes = Secret::Encoded(cleaned)
        .to_bytes()
        .map_err(|_| "chave TOTP inválida (não é base32)".to_string())?;
    // new_unchecked (não new): o `new` exige segredo >= 128 bits, mas segredos
    // reais (Google Authenticator etc.) costumam ter 80 bits (16 chars base32) —
    // rejeitá-los quebraria o import. skew=1 tolera 1 passo de diferença de relógio.
    Ok(TOTP::new_unchecked(
        Algorithm::SHA1,
        DIGITS,
        1,
        PERIOD,
        bytes,
    ))
}

/// Código atual + quanto falta para virar. Erro se a chave não for base32 válida.
pub fn now(secret_b32: &str) -> Result<TotpCode, String> {
    let totp = build(secret_b32)?;
    let code = totp
        .generate_current()
        .map_err(|e| format!("relógio do sistema: {e}"))?;
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    Ok(TotpCode {
        code,
        period: PERIOD,
        seconds_remaining: PERIOD - (unix % PERIOD),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vetores conhecidos do RFC 6238 (Apêndice B), SHA-1, seed ASCII
    /// "12345678901234567890", 8 dígitos, passo 30 s. Confirma que a base do
    /// nosso TOTP casa com a referência oficial.
    #[test]
    fn rfc6238_vetores_sha1() {
        let secret = Secret::Raw(b"12345678901234567890".to_vec())
            .to_bytes()
            .unwrap();
        let totp = TOTP::new(Algorithm::SHA1, 8, 1, 30, secret).unwrap();
        assert_eq!(totp.generate(59), "94287082");
        assert_eq!(totp.generate(1_111_111_109), "07081804");
        assert_eq!(totp.generate(1_111_111_111), "14050471");
        assert_eq!(totp.generate(1_234_567_890), "89005924");
        assert_eq!(totp.generate(2_000_000_000), "69279037");
    }

    #[test]
    fn aceita_base32_com_espacos_e_gera_6_digitos() {
        // "JBSWY3DPEHPK3PXP" = "Hello!\xDE\xAD\xBE\xEF" (secret de exemplo comum).
        let c = now("JBSW Y3DP EHPK 3PXP").unwrap();
        assert_eq!(c.code.len(), 6);
        assert!(c.code.chars().all(|ch| ch.is_ascii_digit()));
        assert!(c.seconds_remaining >= 1 && c.seconds_remaining <= 30);
    }

    #[test]
    fn base32_invalida_falha() {
        assert!(now("!!! não é base32 !!!").is_err());
        assert!(now("").is_err());
    }
}
