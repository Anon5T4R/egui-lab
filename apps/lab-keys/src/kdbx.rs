//! ⚠️ COPIADO VERBATIM de `LocalKeys · src-tauri/src/kdbx.rs` (repo da suíte).
//! Único ajuste: nenhum — a UI da onda 6 consome a API inteira.
//!
//! Import de bancos KeePass (`.kdbx`) — leitura, decifrando com a senha do
//! usuário via o crate auditado `keepass`. Devolve as entradas cruas; o front
//! mapeia para itens de login (atribui ids/timestamps).

use std::fs::File;

use keepass::db::NodeRef;
use keepass::{Database, DatabaseKey};
use serde::Serialize;

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KdbxEntry {
    pub name: String,
    pub username: String,
    pub password: String,
    pub url: String,
    pub totp: String,
    pub notes: String,
}

/// Abre o `.kdbx` com a `password` e extrai todas as entradas (percurso
/// recursivo pelos grupos). Erro genérico se a senha estiver errada.
pub fn import(path: &str, password: &str) -> Result<Vec<KdbxEntry>, String> {
    let mut file = File::open(path).map_err(|e| format!("falha ao abrir '{path}': {e}"))?;
    let key = DatabaseKey::new().with_password(password);
    let db = Database::open(&mut file, key)
        .map_err(|_| "não abriu o .kdbx (senha errada ou formato não suportado)".to_string())?;

    let mut out = Vec::new();
    for node in &db.root {
        if let NodeRef::Entry(e) = node {
            let name = e.get_title().unwrap_or("").to_string();
            let username = e.get_username().unwrap_or("").to_string();
            let password = e.get_password().unwrap_or("").to_string();
            let url = e.get_url().unwrap_or("").to_string();
            let totp = extract_totp(e.get("otp").unwrap_or(""));
            let notes = e.get("Notes").unwrap_or("").to_string();
            if name.is_empty() && username.is_empty() && password.is_empty() {
                continue;
            }
            out.push(KdbxEntry {
                name,
                username,
                password,
                url,
                totp,
                notes,
            });
        }
    }
    Ok(out)
}

/// KeePass guarda o TOTP como segredo base32 puro OU como `otpauth://...?secret=X`
/// (ou `key=X&...` no KeePassXC). Extrai só o segredo.
fn extract_totp(raw: &str) -> String {
    let raw = raw.trim();
    for marker in ["secret=", "key="] {
        if let Some(idx) = raw.find(marker) {
            let rest = &raw[idx + marker.len()..];
            let end = rest.find('&').unwrap_or(rest.len());
            return rest[..end].to_string();
        }
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::extract_totp;

    #[test]
    fn extrai_secret_de_otpauth() {
        assert_eq!(
            extract_totp("otpauth://totp/Acme:joe?secret=JBSWY3DP&issuer=Acme&period=30"),
            "JBSWY3DP"
        );
        assert_eq!(extract_totp("key=ABC123&step=30"), "ABC123");
        assert_eq!(extract_totp("JBSWY3DP"), "JBSWY3DP");
        assert_eq!(extract_totp("  "), "");
    }
}
