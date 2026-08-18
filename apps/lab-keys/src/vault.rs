//! Schema do vault `.tkeys` (o JSON dentro do blob cifrado), compatível com o
//! LocalKeys oficial — lá o schema vive no front TS (`src/types.ts`); aqui só
//! o subconjunto que o lab usa.
//!
//! ESCRITA CONSERVADORA: o lab só ACRESCENTA itens de login. Pastas, anexos,
//! campos custom, histórico de senha e itens de outros tipos de um cofre REAL
//! atravessam intactos — manipulamos o JSON bruto (`serde_json::Value`), nunca
//! um struct que re-serializaria só os campos conhecidos.

use serde_json::{json, Value};

/// Vault recém-criado — mesma semente do oficial (`lib.rs` do LocalKeys).
pub const EMPTY_VAULT: &str = r#"{"version":1,"folders":[],"items":[]}"#;

/// Gravação atômica — copiada verbatim do LocalKeys (`lib.rs`): tmp + rename
/// por cima (atómico em Windows e Unix), com `.bak` do estado anterior. Uma
/// gravação interrompida não corrompe o vault.
pub fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        let _ = std::fs::copy(path, format!("{}.bak", path.display()));
    }
    let tmp = format!("{}.tmp", path.display());
    std::fs::write(&tmp, bytes).map_err(|e| format!("falha ao gravar '{tmp}': {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("falha ao substituir '{}': {}", path.display(), e)
    })
}

/// Linha da lista (visão só do que a UI mostra).
pub struct ItemView {
    pub id: String,
    pub name: String,
    pub username: Option<String>,
    pub favorite: bool,
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawItem {
    id: String,
    name: String,
    favorite: bool,
    deleted_at: Option<i64>,
    login: Option<RawLogin>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct RawLogin {
    username: String,
}

/// Visão de lista do vault (lixeira — `deletedAt` — fica fora, como no oficial).
pub fn items_view(raw: &Value) -> Vec<ItemView> {
    raw.get("items")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|it| {
                    let ri: RawItem = serde_json::from_value(it.clone()).ok()?;
                    if ri.deleted_at.is_some() {
                        return None;
                    }
                    Some(ItemView {
                        id: ri.id,
                        name: ri.name,
                        username: ri.login.map(|l| l.username),
                        favorite: ri.favorite,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Id opaco único (o oficial usa uuid v4 do TS; aqui 128 bits aleatórios em
/// hex — mesma forma, mesmo tamanho, mesmo significado de "string única").
fn gen_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// ACRESCENTA um item de login no vault bruto (mesma forma que `emptyItem`
/// do oficial gera pra kind=login).
pub fn add_login(raw: &mut Value, name: &str, username: &str, password: &str) {
    let item = json!({
        "id": gen_id(),
        "kind": "login",
        "name": name,
        "favorite": false,
        "folderId": null,
        "notes": "",
        "createdAt": now_ms(),
        "updatedAt": now_ms(),
        "deletedAt": null,
        "login": { "username": username, "password": password, "uris": [""], "totp": "" }
    });
    if !raw["items"].is_array() {
        raw["items"] = json!([]);
    }
    raw["items"]
        .as_array_mut()
        .expect("items é array (garantido acima)")
        .push(item);
}

/// `(username, password)` do login pelo id — só leitura, para o botão copiar.
pub fn login_pair(raw: &Value, id: &str) -> Option<(String, String)> {
    raw.get("items")?
        .as_array()?
        .iter()
        .find_map(|it| {
            let same = it.get("id").and_then(Value::as_str) == Some(id);
            if !same {
                return None;
            }
            let login = it.get("login")?;
            Some((
                login.get("username").and_then(Value::as_str)?.to_string(),
                login.get("password").and_then(Value::as_str)?.to_string(),
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_com_item() -> Value {
        let mut v: Value = serde_json::from_str(EMPTY_VAULT).unwrap();
        add_login(&mut v, "Gmail", "joao", "secreta");
        v
    }

    #[test]
    fn add_login_popula_items() {
        let v = vault_com_item();
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"], "login");
        assert_eq!(items[0]["name"], "Gmail");
        assert_eq!(items[0]["login"]["username"], "joao");
        // campos camelCase exatamente como o oficial espera
        assert!(items[0].get("createdAt").is_some());
        assert!(items[0].get("deletedAt").is_some());
        assert!(items[0].get("folderId").is_some());
    }

    #[test]
    fn view_esconde_lixeira() {
        let mut v = vault_com_item();
        v["items"][0]["deletedAt"] = json!(1700000000000i64);
        assert!(items_view(&v).is_empty());
    }

    #[test]
    fn login_pair_devolve_as_credenciais() {
        let v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        assert_eq!(
            login_pair(&v, &id),
            Some(("joao".into(), "secreta".into()))
        );
        assert_eq!(login_pair(&v, "id-que-nao-existe"), None);
    }

    #[test]
    fn add_preserva_campos_desconhecidos() {
        // Um cofre REAL tem anexos/campos custom que o lab não conhece —
        // acrescentar item não pode derrubar nada disso.
        let mut v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[{"id":"f1","name":"Pessoal"}],
               "items":[{"id":"x","kind":"note","name":"nota",
                          "customFields":[{"id":"c1","name":"k","value":"v","hidden":false},
                                          {"id":"c2","name":"k2","value":"v2","hidden":true}],
                          "attachments":[{"id":"a1","name":"f.png","size":10,"mime":"image/png","dataB64":"aGk="}]}]}"#,
        )
        .unwrap();
        add_login(&mut v, "Novo", "u", "p");
        assert_eq!(v["folders"][0]["name"], "Pessoal");
        assert_eq!(v["items"].as_array().unwrap().len(), 2);
        assert_eq!(v["items"][0]["customFields"].as_array().unwrap().len(), 2);
        assert_eq!(v["items"][0]["attachments"][0]["mime"], "image/png");
        assert_eq!(v["items"][1]["login"]["password"], "p");
    }

    #[test]
    fn atomic_write_escreve_e_reescreve() {
        let dir = std::env::temp_dir().join(format!(
            "lab-keys-aw-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("v.tkeys");
        atomic_write(&p, b"um").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"um");
        atomic_write(&p, b"dois").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"dois");
        assert_eq!(std::fs::read(dir.join("v.tkeys.bak")).unwrap(), b"um");
        std::fs::remove_dir_all(&dir).ok();
    }
}
