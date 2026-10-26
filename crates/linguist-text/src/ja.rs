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

/// Plural and honorific suffixes: a noun with one of these (子どもたち,
/// 学生ら, 田中様) is the noun, never a new word, even when JMdict lists the
/// joined form.
const INFLECTING_SUFFIXES: [&str; 11] = [
    "たち",
    "達",
    "ら",
    "等",
    "がた",
    "様",
    "さま",
    "さん",
    "君",
    "くん",
    "ちゃん",
];

fn inflecting_suffix(token: &Token) -> bool {
    token.pos[0] == "名詞"
        && token.pos[1] == "接尾"
        && INFLECTING_SUFFIXES.contains(&token.surface.as_str())
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
            while end < tokens.len() && noun(&tokens[end]) && !inflecting_suffix(&tokens[end]) {
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

/// Verbs that make a compound particle with the に or と before them
/// (について, に関して, として, という): they stay in that chunk.
const COMPOUND_PARTICLE_VERBS: [&str; 13] = [
    "つく",
    "関する",
    "対する",
    "よる",
    "当たる",
    "際する",
    "沿う",
    "おく",
    "する",
    "いう",
    "言う",
    "比べる",
    "伴う",
];

/// Most chunks a sentence is cut into; shorter neighbours are joined first.
pub const MAX_CHUNKS: usize = 9;

/// A content word that starts a chunk (bunsetsu): a noun, an independent
/// verb or adjective, an adverb, adnominal, conjunction, interjection or
/// prefix. Suffixes and dependent words belong to the chunk before them.
fn starts_chunk(token: &Token) -> bool {
    let [pos, sub, _] = &token.pos;
    match pos.as_str() {
        "名詞" => !matches!(sub.as_str(), "接尾" | "非自立"),
        "動詞" | "形容詞" => sub == "自立",
        "副詞" | "連体詞" | "接続詞" | "感動詞" | "接頭詞" => true,
        _ => false,
    }
}

/// Whether `token` stays in the chunk that `previous` ended.
fn continues_chunk(previous: &Token, token: &Token) -> bool {
    let [pos, sub, _] = &token.pos;
    let [prev_pos, prev_sub, _] = &previous.pos;
    // A prefix belongs to its word; a noun run (交通費, 3点) is one word,
    // unless the first is adverbial (先程 ちょうだい).
    prev_pos == "接頭詞"
        || (pos == "名詞" && prev_pos == "名詞" && prev_sub != "副詞可能")
        // する after a サ変 noun (確認 させ).
        || (pos == "動詞" && token.base == "する" && prev_pos == "名詞" && prev_sub == "サ変接続")
        // A compound particle (に つき まして, と して).
        || (pos == "動詞"
            && prev_pos == "助詞"
            && matches!(previous.surface.as_str(), "に" | "と")
            && COMPOUND_PARTICLE_VERBS.contains(&token.base.as_str()))
        // A word after an opening bracket.
        || (prev_pos == "記号" && prev_sub == "括弧開")
        || sub == "接尾"
}

/// Cut a sentence into bunsetsu chunks for a tap-build drill (WP-28): each
/// chunk is a content word with the particles, auxiliaries, suffixes and
/// punctuation that follow it. The chunks join to `text` exactly. A chunk
/// of one character is joined to its neighbour, and a sentence gives at
/// most `MAX_CHUNKS`, the shortest neighbours joined first.
pub fn chunks(text: &str) -> Result<Vec<String>, String> {
    let tokens = tokenize(text)?;
    let mut out: Vec<String> = Vec::new();
    let mut content = false;
    let mut cursor = 0;
    for (i, token) in tokens.iter().enumerate() {
        // Characters the tokenizer skipped (spaces) stay where they were.
        let at = text[cursor..]
            .find(token.surface.as_str())
            .map(|offset| cursor + offset)
            .ok_or("TEXT_CHUNK_ALIGNMENT")?;
        let gap = &text[cursor..at];
        let open = token.pos[0] == "記号" && token.pos[1] == "括弧開";
        let new_chunk = (starts_chunk(token) || open)
            && content
            && !(i > 0 && continues_chunk(&tokens[i - 1], token));
        if new_chunk || out.is_empty() {
            if let Some(last) = out.last_mut() {
                last.push_str(gap);
                out.push(String::new());
            } else {
                out.push(gap.to_owned());
            }
            content = false;
        } else {
            out.last_mut().unwrap().push_str(gap);
        }
        out.last_mut().unwrap().push_str(&token.surface);
        content |= starts_chunk(token);
        cursor = at + token.surface.len();
    }
    match out.last_mut() {
        Some(last) => last.push_str(&text[cursor..]),
        None if !text.is_empty() => out.push(text.to_owned()),
        None => {}
    }
    // One-character chunks join their neighbour.
    let mut i = 0;
    while out.len() > 1 && i < out.len() {
        if out[i].trim().chars().count() <= 1 {
            if i == 0 {
                let first = out.remove(0);
                out[0].insert_str(0, &first);
            } else {
                let chunk = out.remove(i);
                out[i - 1].push_str(&chunk);
            }
            continue;
        }
        i += 1;
    }
    while out.len() > MAX_CHUNKS {
        let shortest = (0..out.len() - 1)
            .min_by_key(|&i| out[i].chars().count() + out[i + 1].chars().count())
            .unwrap();
        let next = out.remove(shortest + 1);
        out[shortest].push_str(&next);
    }
    Ok(out)
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
    fn a_plural_or_honorific_suffix_is_never_joined() {
        let known = ["子ども", "子どもたち", "子供達", "学生ら"];
        assert_eq!(bases("子どもたちが遊ぶ", &known), ["子ども", "遊ぶ"]);
        assert_eq!(bases("子供達が遊ぶ", &known), ["子供", "遊ぶ"]);
        assert_eq!(bases("学生らが来た", &known), ["学生", "来る"]);
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
    fn a_keigo_sentence_is_cut_into_bunsetsu_that_join_back() {
        let text = "先程ちょうだいした見積書につきまして、3点ほど確認させていただきたいことがあるのですが。";
        let cut = chunks(text).unwrap();
        assert_eq!(
            cut,
            [
                "先程",
                "ちょうだいした",
                "見積書につきまして、",
                "3点ほど",
                "確認させていただきたいことが",
                "あるのですが。"
            ]
        );
        assert_eq!(cut.concat(), text);
    }

    #[test]
    fn short_and_spaced_sentences_still_join_back() {
        for text in [
            "はい、ただ今、お持ちいたします。",
            "そちら資料の2ページの表3をご覧ください。",
            "お忙しいとは存じますが、山田様のご都合のよい日時をお知らせいただければ幸いでございます。",
            "これ は 「テスト」 です。",
            "はい。",
        ] {
            let cut = chunks(text).unwrap();
            assert_eq!(cut.concat(), text, "{cut:?}");
            assert!(cut.len() <= MAX_CHUNKS);
            assert!(
                cut.len() == 1 || cut.iter().all(|c| c.trim().chars().count() > 1),
                "{cut:?}"
            );
        }
        assert_eq!(
            chunks("はい、ただ今、お持ちいたします。").unwrap(),
            ["はい、", "ただ今、", "お持ちいたします。"]
        );
    }

    #[test]
    fn a_word_class_comes_from_its_part_of_speech() {
        assert_eq!(class("食べる"), Some("verb"));
        assert_eq!(class("静か"), Some("adjective"));
        assert_eq!(class("ゆっくり"), Some("adverb"));
        assert_eq!(class("しかし"), Some("function"));
    }
}
