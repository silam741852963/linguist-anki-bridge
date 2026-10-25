//! WP-25 text tools shared by the word harvester and the WP-26 drills: a
//! Japanese tokenizer (Lindera with the embedded IPADIC dictionary, offline)
//! and an English word splitter with a rule lemmatizer. Nothing here reads a
//! store or calls a service.
pub mod en;
pub mod ja;
pub mod jmdict;

/// One content word met in a sentence: its base (dictionary) form, the forms
/// that may name it in a deck, a reading (Japanese, hiragana) and a part of
/// speech as the tokenizer gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub base: String,
    /// Other spellings the known-word check also tries (the surface form, a
    /// second lemma guess), base first.
    pub forms: Vec<String>,
    pub reading: String,
    pub pos: String,
}

/// Katakana to hiragana; other characters unchanged.
pub fn hiragana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// True for a hiragana or katakana character (with the long-vowel mark).
pub fn is_kana(c: char) -> bool {
    matches!(c, 'ぁ'..='ゖ' | 'ァ'..='ヺ' | 'ー' | 'ゝ' | 'ゞ' | 'ヽ' | 'ヾ')
}

/// True for a CJK ideograph (kanji) or the iteration mark 々.
pub fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々' | '〆')
}
