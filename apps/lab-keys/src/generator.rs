//! ⚠️ COPIADO VERBATIM de `LocalKeys · src-tauri/src/generator.rs` (repo da
//! suíte). Único ajuste: nenhum — a UI da onda 6 consome a API inteira.
//!
//! Gerador de senhas e passphrases, e medidor de força.
//!
//! Toda aleatoriedade vem do CSPRNG do SO (`OsRng`) — nunca do `rand` default
//! (que é semeado e não serve para segredos). A força usa o `zxcvbn` (mesmo
//! algoritmo do Bitwarden/Dropbox), não uma heurística caseira.

use rand::rngs::OsRng;
use rand::Rng;
use serde::{Deserialize, Serialize};

/// Wordlist EFF "large" (7776 palavras, uma por linha no formato `12345\tword`).
/// Domínio público (EFF). log2(7776) ≈ 12,9 bits de entropia por palavra.
const EFF_LARGE_EN: &str = include_str!("wordlists/eff_large_en.txt");

fn wordlist() -> Vec<&'static str> {
    EFF_LARGE_EN
        .lines()
        .filter_map(|l| l.split_whitespace().last())
        .collect()
}

const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{};:,.?";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordOptions {
    pub length: usize,
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    pub symbols: bool,
}

impl Default for PasswordOptions {
    fn default() -> Self {
        PasswordOptions {
            length: 20,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
        }
    }
}

/// Gera uma senha aleatória respeitando as classes escolhidas, garantindo ao
/// menos um caractere de cada classe ativa (sem enfraquecer a distribuição:
/// os obrigatórios também são sorteados e a ordem final é embaralhada).
pub fn generate_password(opts: &PasswordOptions) -> Result<String, String> {
    let mut classes: Vec<&[u8]> = Vec::new();
    if opts.lowercase {
        classes.push(LOWER);
    }
    if opts.uppercase {
        classes.push(UPPER);
    }
    if opts.digits {
        classes.push(DIGITS);
    }
    if opts.symbols {
        classes.push(SYMBOLS);
    }
    if classes.is_empty() {
        return Err("selecione ao menos uma classe de caractere".into());
    }
    let length = opts.length.clamp(4, 128);
    if length < classes.len() {
        return Err("comprimento menor que o número de classes exigidas".into());
    }

    let pool: Vec<u8> = classes.iter().flat_map(|c| c.iter().copied()).collect();
    let mut rng = OsRng;
    let mut chars: Vec<u8> = Vec::with_capacity(length);

    // Um obrigatório por classe ativa...
    for class in &classes {
        chars.push(class[rng.gen_range(0..class.len())]);
    }
    // ...o resto do pool geral.
    while chars.len() < length {
        chars.push(pool[rng.gen_range(0..pool.len())]);
    }
    // Embaralha (Fisher-Yates) para não fixar os obrigatórios no início.
    for i in (1..chars.len()).rev() {
        let j = rng.gen_range(0..=i);
        chars.swap(i, j);
    }

    Ok(String::from_utf8(chars).expect("pools são ASCII"))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassphraseOptions {
    pub words: usize,
    pub separator: String,
    pub capitalize: bool,
    pub include_number: bool,
}

impl Default for PassphraseOptions {
    fn default() -> Self {
        PassphraseOptions {
            words: 5,
            separator: "-".into(),
            capitalize: false,
            include_number: true,
        }
    }
}

/// Passphrase estilo diceware: N palavras sorteadas da wordlist EFF.
pub fn generate_passphrase(opts: &PassphraseOptions) -> Result<String, String> {
    let list = wordlist();
    if list.is_empty() {
        return Err("wordlist vazia".into());
    }
    let count = opts.words.clamp(3, 12);
    let mut rng = OsRng;

    let mut words: Vec<String> = (0..count)
        .map(|_| {
            let w = list[rng.gen_range(0..list.len())];
            if opts.capitalize {
                let mut c = w.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            } else {
                w.to_string()
            }
        })
        .collect();

    if opts.include_number {
        // Anexa um dígito a uma palavra aleatória (hardening leve contra dicionário).
        let idx = rng.gen_range(0..words.len());
        let digit = rng.gen_range(0..10);
        words[idx].push_str(&digit.to_string());
    }

    let sep = if opts.separator.is_empty() {
        "-"
    } else {
        &opts.separator
    };
    Ok(words.join(sep))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Strength {
    /// 0 (péssima) a 4 (ótima) — escala do zxcvbn.
    pub score: u8,
    /// log10 do número estimado de tentativas para quebrar.
    pub guesses_log10: f64,
}

/// Avalia a força de uma senha com o zxcvbn. `user_inputs` são termos a penalizar
/// (nome do item, URL) que o atacante provavelmente conhece.
pub fn password_strength(password: &str, user_inputs: &[&str]) -> Strength {
    let entropy = zxcvbn::zxcvbn(password, user_inputs);
    Strength {
        score: u8::from(entropy.score()),
        guesses_log10: entropy.guesses_log10(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordlist_tem_7776_palavras() {
        assert_eq!(wordlist().len(), 7776, "EFF large deve ter 7776 palavras");
    }

    #[test]
    fn senha_respeita_comprimento_e_classes() {
        let opts = PasswordOptions {
            length: 24,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
        };
        let pw = generate_password(&opts).unwrap();
        assert_eq!(pw.chars().count(), 24);
        assert!(pw.chars().any(|c| c.is_ascii_lowercase()));
        assert!(pw.chars().any(|c| c.is_ascii_uppercase()));
        assert!(pw.chars().any(|c| c.is_ascii_digit()));
        assert!(pw.chars().any(|c| SYMBOLS.contains(&(c as u8))));
    }

    #[test]
    fn senha_sem_classe_falha() {
        let opts = PasswordOptions {
            length: 10,
            lowercase: false,
            uppercase: false,
            digits: false,
            symbols: false,
        };
        assert!(generate_password(&opts).is_err());
    }

    #[test]
    fn senhas_geradas_nao_repetem() {
        let opts = PasswordOptions::default();
        let a = generate_password(&opts).unwrap();
        let b = generate_password(&opts).unwrap();
        assert_ne!(a, b, "duas senhas de 20 chars não deveriam colidir");
    }

    #[test]
    fn passphrase_tem_o_numero_de_palavras() {
        let opts = PassphraseOptions {
            words: 5,
            separator: "-".into(),
            capitalize: true,
            include_number: false,
        };
        let p = generate_passphrase(&opts).unwrap();
        assert_eq!(p.split('-').count(), 5);
    }

    #[test]
    fn forca_distingue_fraca_de_forte() {
        let fraca = password_strength("123456", &[]);
        let forte = password_strength("correct-horse-battery-staple-7", &[]);
        assert!(fraca.score < forte.score);
    }
}
