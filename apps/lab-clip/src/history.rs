//! Histórico do lab-clip — em memória (o oficial persiste em SQLite; aqui o
//! alvo do teste é integração OS, não armazenamento). Semânticas copiadas do
//! LocalClip oficial: recopiar o mesmo conteúdo sobe pro topo sem duplicar,
//! fixados nunca expiram e nunca são apagados pelo "limpar".

pub const MAX_ITEMS: usize = 500;

#[derive(Clone)]
pub struct ClipItem {
    pub text: String,
    pub pinned: bool,
}

/// Insere no topo; se o texto já existia (fixado ou não), a cópia antiga sai —
/// o item "atual" é sempre o mais recente no topo.
pub fn insert(items: &mut Vec<ClipItem>, text: String) {
    items.retain(|i| i.text != text);
    items.insert(
        0,
        ClipItem {
            text,
            pinned: false,
        },
    );
    // Fixados não contam pro teto: nunca apertam o espaço dos soltos.
    let soltos = items.iter().filter(|i| !i.pinned).count();
    if soltos > MAX_ITEMS {
        let mut extras = soltos - MAX_ITEMS;
        let mut idx = items.len();
        while idx > 0 && extras > 0 {
            idx -= 1;
            if !items[idx].pinned {
                items.remove(idx);
                extras -= 1;
            }
        }
    }
}

/// "Limpar tudo" mantém os fixados (regra do oficial).
pub fn clear_unpinned(items: &mut Vec<ClipItem>) {
    items.retain(|i| i.pinned);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: &str, pinned: bool) -> ClipItem {
        ClipItem {
            text: text.into(),
            pinned,
        }
    }

    #[test]
    fn novo_item_vai_pro_topo() {
        let mut v = vec![];
        insert(&mut v, "a".into());
        insert(&mut v, "b".into());
        assert_eq!(v[0].text, "b");
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn recopiar_sobe_sem_duplicar() {
        let mut v = vec![];
        insert(&mut v, "a".into());
        insert(&mut v, "b".into());
        insert(&mut v, "a".into()); // recopiado
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].text, "a");
    }

    #[test]
    fn recopiar_preserva_o_pin() {
        let mut v = vec![item("fixo", true), item("x", false)];
        insert(&mut v, "fixo".into());
        // saiu da posição e voltou pro topo SEMI o pin — regra: recopiar não
        // desafixa (o oficial só desafia pelo botão).
        assert_eq!(v[0].text, "fixo");
        assert!(v[0].pinned);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn limpar_preserva_fixados() {
        let mut v = vec![item("a", false), item("b", true), item("c", false)];
        clear_unpinned(&mut v);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].text, "b");
    }

    #[test]
    fn teto_apaga_soltos_nunca_fixados() {
        let mut v = vec![];
        for i in 0..MAX_ITEMS {
            insert(&mut v, format!("s{i}"));
        }
        v[0].pinned = true; // o mais novo vira fixado
        insert(&mut v, "novo".into());
        assert_eq!(v.len(), MAX_ITEMS); // 1 fixado + (MAX-1) soltos + novo... conferindo por classe:
        let soltos = v.iter().filter(|i| !i.pinned).count();
        assert!(soltos <= MAX_ITEMS);
        assert!(v.iter().any(|i| i.text == "s0")); // antigo solto pode sair
        assert!(v.iter().any(|i| i.pinned && i.text == "s499")); // fixado fica
    }
}
