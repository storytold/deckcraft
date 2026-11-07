use deckcraft_text::map_symbol_bullet;

#[test]
fn monotype_sorts_m_bullet_is_open_circle() {
    assert_eq!(map_symbol_bullet("m", Some("Monotype Sorts")), "\u{25CB}");
}

#[test]
fn wingdings_eth_bullet_is_right_arrow() {
    assert_eq!(map_symbol_bullet("\u{f0}", Some("Wingdings")), "\u{21E8}");
    assert_eq!(map_symbol_bullet("\u{f0f0}", Some("Wingdings")), "\u{21E8}");
}
