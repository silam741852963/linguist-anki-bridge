//! Japanese: Lindera with the embedded IPADIC dictionary. A sentence becomes
//! content words in their dictionary form: particles, auxiliaries, symbols,
//! numbers, proper nouns, dependent words and single kana are dropped, a サ変
//! noun followed by する is the noun, and adjacent nouns are joined when the
//! caller knows the joined form (交通 + 費 is 交通費 when 交通費 is a word).
use crate::{Word, hiragana, is_kana, is_kanji};
use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use std::borrow::Cow;
use std::sync::OnceLock;

/// One IPADIC token: surface, part of speech (three levels), conjugation
/// type, base form and reading (hiragana; empty when IPADIC has none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub surface: String,
    pub pos: [String; 3],
    pub conjugation: String,
    pub base: String,
    pub reading: String,
}

fn segmenter() -> Result<&'static Segmenter, String> {
    static SEGMENTER: OnceLock<Result<Segmenter, String>> = OnceLock::new();
    SEGMENTER
        .get_or_init(|| {
            let dictionary = load_dictionary("embedded://ipadic")
                .map_err(|e| format!("TEXT_TOKENIZER_UNAVAILABLE: {e}"))?;
            Ok(Segmenter::new(Mode::Normal, dictionary, None))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// IPADIC tokens of `text`.
pub fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let mut tokens = segmenter()?
        .segment(Cow::Borrowed(text))
        .map_err(|e| format!("TEXT_TOKENIZER_FAILED: {e}"))?;
    Ok(tokens
        .iter_mut()
        .map(|token| {
            let surface = token.surface.to_string();
            let details: Vec<String> = token.details().iter().map(|d| d.to_string()).collect();
            let field = |i: usize| {
                details
                    .get(i)
                    .filter(|d| d.as_str() != "*")
                    .cloned()
                    .unwrap_or_default()
            };
            Token {
                pos: [field(0), field(1), field(2)],
                conjugation: field(4),
                base: Some(field(6))
                    .filter(|b| !b.is_empty())
                    .unwrap_or_else(|| surface.clone()),
                reading: hiragana(&field(7)),
                surface,
            }
        })
        .collect())
}

/// The reading of a token's base form: the reading of the surface with its
/// kana ending swapped for the base form's (食べさせ たべさせ, 食べる たべる).
/// Empty when the two do not share a stem.
fn base_reading(token: &Token) -> String {
    if token.reading.is_empty() {
        return String::new();
    }
    if token.surface == token.base {
        return token.reading.clone();
    }
    let surface: Vec<char> = token.surface.chars().collect();
    let base: Vec<char> = token.base.chars().collect();
    let shared = surface
        .iter()
        .zip(&base)
        .take_while(|(a, b)| a == b)
        .count();
    let surface_rest: String = hiragana(&surface[shared..].iter().collect::<String>());
    let base_rest: String = hiragana(&base[shared..].iter().collect::<String>());
    if !surface_rest.chars().all(is_kana) || !base_rest.chars().all(is_kana) {
        return String::new();
    }
    match token.reading.strip_suffix(&surface_rest) {
        Some(stem) => format!("{stem}{base_rest}"),
        None => String::new(),
    }
}

/// Words IPADIC tags as content words that act as grammar: negation (休みなく),
/// かもしれない, and polite endings met outside a Japanese sentence.
const FUNCTION_WORDS: [&str; 4] = ["ない", "しれる", "ます", "です"];

fn dropped(token: &Token) -> bool {
    let [pos, sub, sub2] = &token.pos;
    if FUNCTION_WORDS.contains(&token.base.as_str())
        || !token.base.chars().any(|c| is_kana(c) || is_kanji(c))
    {
        return true;
    }
    let single_kana = token.base.chars().count() == 1 && token.base.chars().all(is_kana);
    matches!(
        pos.as_str(),
        "助詞" | "助動詞" | "記号" | "フィラー" | "その他" | ""
    ) || (pos == "名詞" && matches!(sub.as_str(), "数" | "固有名詞" | "非自立" | "代名詞"))
        || (pos == "名詞" && sub == "接尾" && sub2 == "人名")
        || (matches!(pos.as_str(), "動詞" | "形容詞") && matches!(sub.as_str(), "非自立" | "接尾"))
        || single_kana
        || token.base.chars().all(|c| c.is_ascii() || c.is_numeric())
}

fn noun(token: &Token) -> bool {
    token.pos[0] == "名詞" && !dropped(token)
        || (token.pos[0] == "名詞" && token.pos[1] == "接尾" && token.pos[2] != "人名")
        || token.pos[0] == "接頭詞"
}

/// The content words of one sentence. `known` says whether a joined noun
/// (交通費) is a word worth keeping whole.
pub fn words(text: &str, known: &dyn Fn(&str) -> bool) -> Result<Vec<Word>, String> {
    // An English explanation that quotes a Japanese form ("The ます form is
    // polite.") is not a Japanese sentence.
    let japanese = text.chars().filter(|&c| is_kana(c) || is_kanji(c)).count();
    if text.chars().filter(char::is_ascii_alphabetic).count() > japanese {
        return Ok(Vec::new());
    }
    let tokens = tokenize(text)?;
    let mut out: Vec<Word> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        // Adjacent nouns (and a prefix or suffix) joined when the joined form
        // is known: the longest run first.
        if noun(token) {
            let mut end = i + 1;
            while end < tokens.len() && noun(&tokens[end]) {
                end += 1;
            }
            let mut joined = None;
            for stop in (i + 2..=end).rev() {
                let surface: String = tokens[i..stop].iter().map(|t| t.surface.as_str()).collect();
                if known(&surface) {
                    let reading: String =
                        tokens[i..stop].iter().map(|t| t.reading.as_str()).collect();
                    joined = Some((stop, surface, reading));
                    break;
                }
            }
            if let Some((stop, surface, reading)) = joined {
                out.push(Word {
                    forms: vec![surface.clone()],
                    base: surface,
                    reading,
                    pos: "名詞".into(),
                });
                i = stop;
                continue;
            }
        }
        if !dropped(token)
            && !(token.pos[0] == "接頭詞")
            && !(token.pos[0] == "名詞" && token.pos[1] == "接尾")
        {
            // する after a サ変 noun belongs to the noun.
            let after_sahen = token.base == "する"
                && i > 0
                && tokens[i - 1].pos[0] == "名詞"
                && tokens[i - 1].pos[1] == "サ変接続";
            if !after_sahen && token.base != "する" {
                let mut forms = vec![token.base.clone()];
                if token.surface != token.base {
                    forms.push(token.surface.clone());
                }
                out.push(Word {
                    base: token.base.clone(),
                    forms,
                    reading: base_reading(token),
                    pos: token.pos[0].clone(),
                });
            }
        }
        i += 1;
    }
    Ok(out)
}

/// The WP-27 class of a word with no dictionary label, from its IPADIC part
/// of speech: noun, verb, adjective, adverb, pronoun, function, number,
/// affix or interjection.
pub fn class(text: &str) -> Option<&'static str> {
    let tokens = tokenize(text).ok()?;
    let content = tokens
        .iter()
        .find(|t| !matches!(t.pos[0].as_str(), "記号" | ""))?;
    Some(match (content.pos[0].as_str(), content.pos[1].as_str()) {
        ("名詞", "代名詞") => "pronoun",
        ("名詞", "数") => "number",
        ("名詞", "形容動詞語幹") => "adjective",
        ("名詞", _) => "noun",
        ("動詞", _) => "verb",
        ("形容詞", _) => "adjective",
        ("副詞", _) => "adverb",
        ("連体詞", _) => "adjective",
        ("接続詞" | "助詞" | "助動詞", _) => "function",
        ("感動詞", _) => "interjection",
        ("接頭詞", _) => "affix",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bases(text: &str, known: &[&str]) -> Vec<String> {
        words(text, &|w| known.contains(&w))
            .unwrap()
            .into_iter()
            .map(|w| w.base)
            .collect()
    }

    #[test]
    fn a_known_compound_stays_whole_and_others_split() {
        assert_eq!(bases("交通費を払った。", &["交通費"]), ["交通費", "払う"]);
        assert_eq!(bases("交通費を払った。", &[]), ["交通", "払う"]);
    }

    #[test]
    fn a_sahen_noun_takes_its_suru() {
        assert_eq!(bases("毎日勉強しています。", &[]), ["毎日", "勉強"]);
    }

    #[test]
    fn a_conjugated_verb_is_its_dictionary_form_with_reading() {
        let w = words("食べさせられる", &|_| false).unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].base, "食べる");
        assert_eq!(w[0].reading, "たべる");
        assert!(w[0].forms.contains(&"食べ".to_owned()));
    }

    #[test]
    fn names_particles_and_single_kana_are_dropped() {
        assert_eq!(bases("田中さんはゆっくり歩く。", &[]), ["ゆっくり", "歩く"]);
        assert_eq!(bases("りんごを3つ買う", &[]), ["りんご", "買う"]);
    }

    #[test]
    fn grammar_words_symbols_and_english_text_are_dropped() {
        assert_eq!(bases("休みなく働く", &[]), ["休み", "働く"]);
        assert_eq!(bases("雨かもしれません", &[]), ["雨"]);
        assert_eq!(bases("～の試み", &[]), ["試み"]);
        assert!(bases("The ます form is polite.", &[]).is_empty());
        assert_eq!(bases("PDAを使う", &[]), ["使う"]);
    }

    #[test]
    fn a_word_class_comes_from_its_part_of_speech() {
        assert_eq!(class("食べる"), Some("verb"));
        assert_eq!(class("静か"), Some("adjective"));
        assert_eq!(class("ゆっくり"), Some("adverb"));
        assert_eq!(class("しかし"), Some("function"));
    }
}
