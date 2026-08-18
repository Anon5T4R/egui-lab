//! Histórico do lab-clip — em memória (o oficial persiste em SQLite; aqui o
//! alvo do teste é integração OS + pipeline de imagem, não armazenamento).
//! Semânticas copiadas do LocalClip oficial: recopiar o mesmo conteúdo sobe
//! pro topo sem duplicar (preservando o pin), fixados nunca expiram e nunca
//! são apagados pelo "limpar". Imagens: PNG em memória com teto próprio
//! (RAM manda — o oficial guarda PNG comprimido no SQLite, aqui é o mesmo
//! PNG, só sem disco).

/// Texto solto: teto alto (o oficial usa 500 configurável).
pub const MAX_TEXT_ITEMS: usize = 500;
/// Imagem: teto próprio bem menor — PNG de screenshot pesa MBs.
pub const MAX_IMAGE_ITEMS: usize = 20;

#[derive(Clone, PartialEq)]
pub enum Payload {
    Text(String),
    Image(ImageItem),
}

#[derive(Clone, PartialEq)]
pub struct ImageItem {
    pub png: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone)]
pub struct ClipItem {
    pub id: u64,
    pub payload: Payload,
    pub pinned: bool,
}

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Insere no topo; se o payload já existia, a cópia antiga sai — mas o PIN
/// atravessa (o oficial faz `UPDATE created_ms WHERE hash`: a linha é a
/// mesma, só renasce no topo; desafixar é só pelo botão).
pub fn insert(items: &mut Vec<ClipItem>, payload: Payload) {
    let was_pinned = items
        .iter()
        .find(|i| i.payload == payload)
        .map(|i| i.pinned)
        .unwrap_or(false);
    items.retain(|i| i.payload != payload);
    items.insert(
        0,
        ClipItem {
            id: next_id(),
            payload,
            pinned: was_pinned,
        },
    );

    // Tetes por classe: fixado não conta (nunca aperta o espaço dos soltos).
    match items[0].payload {
        Payload::Text(_) => {
            let soltos = items
                .iter()
                .filter(|i| matches!(i.payload, Payload::Text(_)) && !i.pinned)
                .count();
            if soltos > MAX_TEXT_ITEMS {
                evict(items, MAX_TEXT_ITEMS, |i| {
                    matches!(i.payload, Payload::Text(_))
                });
            }
        }
        Payload::Image(_) => {
            let soltos = items
                .iter()
                .filter(|i| matches!(i.payload, Payload::Image(_)) && !i.pinned)
                .count();
            if soltos > MAX_IMAGE_ITEMS {
                evict(items, MAX_IMAGE_ITEMS, |i| {
                    matches!(i.payload, Payload::Image(_))
                });
            }
        }
    }
}

/// Apaga os mais VELHOS não fixados da classe até restar `teto` soltos.
fn evict(items: &mut Vec<ClipItem>, teto: usize, classe: fn(&ClipItem) -> bool) {
    let mut excesso = items
        .iter()
        .filter(|i| classe(i) && !i.pinned)
        .count()
        .saturating_sub(teto);
    let mut idx = items.len();
    while idx > 0 && excesso > 0 {
        idx -= 1;
        if classe(&items[idx]) && !items[idx].pinned {
            items.remove(idx);
            excesso -= 1;
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

    fn text(s: &str) -> Payload {
        Payload::Text(s.into())
    }

    fn img(tag: u8) -> Payload {
        Payload::Image(ImageItem {
            png: vec![tag; 8],
            w: 2,
            h: 1,
        })
    }

    #[test]
    fn novo_item_vai_pro_topo() {
        let mut v = vec![];
        insert(&mut v, text("a"));
        insert(&mut v, text("b"));
        assert_eq!(v[0].payload, text("b"));
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn recopiar_sobe_sem_duplicar() {
        let mut v = vec![];
        insert(&mut v, text("a"));
        insert(&mut v, text("b"));
        insert(&mut v, text("a")); // recopiado
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].payload, text("a"));
    }

    #[test]
    fn recopiar_preserva_o_pin() {
        let mut v = vec![];
        insert(&mut v, text("fixo"));
        insert(&mut v, text("x"));
        v[1].pinned = true; // "fixo" agora está no índice 1
        insert(&mut v, text("fixo"));
        // saiu da posição e voltou pro topo SEM perder o pin — regra:
        // recopiar não desafixa (o oficial só desafia pelo botão).
        assert_eq!(v[0].payload, text("fixo"));
        assert!(v[0].pinned);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn limpar_preserva_fixados() {
        let mut v = vec![];
        insert(&mut v, text("a"));
        insert(&mut v, text("b"));
        insert(&mut v, img(1));
        v[0].pinned = true;
        clear_unpinned(&mut v);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].payload, img(1));
    }

    #[test]
    fn teto_de_texto_apaga_soltos_nunca_fixados() {
        let mut v = vec![];
        for i in 0..MAX_TEXT_ITEMS {
            insert(&mut v, text(&format!("s{i}")));
        }
        v[0].pinned = true; // o mais novo vira fixado
        insert(&mut v, text("novo"));
        // 1 fixado + 500 soltos = 501: a invariante é SOLTOS ≤ MAX.
        assert_eq!(v.len(), MAX_TEXT_ITEMS + 1);
        let soltos = v.iter().filter(|i| !i.pinned).count();
        assert_eq!(soltos, MAX_TEXT_ITEMS);
        assert!(v.iter().any(|i| i.pinned && i.payload == text("s499")));
    }

    #[test]
    fn imagens_tem_teto_proprio_e_nao_pegam_o_das_textos() {
        let mut v = vec![];
        for i in 0..(MAX_IMAGE_ITEMS + 5) {
            insert(&mut v, img(i as u8));
        }
        let imagens = v
            .iter()
            .filter(|i| matches!(i.payload, Payload::Image(_)))
            .count();
        assert_eq!(imagens, MAX_IMAGE_ITEMS);

        // Texto continua cabendo muito mais que MAX_IMAGE_ITEMS.
        for i in 0..30 {
            insert(&mut v, text(&format!("t{i}")));
        }
        let textos = v
            .iter()
            .filter(|i| matches!(i.payload, Payload::Text(_)))
            .count();
        assert_eq!(textos, 30);
    }

    #[test]
    fn imagem_duplicada_sobe_sem_duplicar() {
        let mut v = vec![];
        insert(&mut v, img(7));
        insert(&mut v, text("meio"));
        insert(&mut v, img(7)); // mesma imagem de novo
        assert_eq!(
            v.iter()
                .filter(|i| matches!(i.payload, Payload::Image(_)))
                .count(),
            1
        );
        assert!(matches!(v[0].payload, Payload::Image(_)));
    }

    #[test]
    fn ids_sao_unicos() {
        let mut v = vec![];
        insert(&mut v, text("a"));
        insert(&mut v, text("b"));
        insert(&mut v, text("a"));
        assert_ne!(v[0].id, v[1].id);
    }
}
