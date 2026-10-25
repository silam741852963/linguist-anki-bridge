//! English: words split on letters (an apostrophe inside a word kept),
//! function words dropped by a fixed stop list, a capitalized word inside a
//! sentence taken for a name and dropped, and each word reduced to a lemma by
//! an irregular-forms table and suffix rules.
use crate::Word;
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

const STOP: &str = "a about above across after again against all almost alone along already also although always am among an and another any anybody anyone anything anyway anywhere are aren't around as at back be became because become becomes been before behind being below beside besides between beyond both but by can can't cannot could couldn't did didn't do does doesn't doing don't done down during each either else enough even ever every everybody everyone everything everywhere few for from further get gets getting go goes going gone got had hadn't has hasn't have haven't having he he'd he'll he's her here here's hers herself him himself his how how's however i i'd i'll i'm i've if in inside instead into is isn't it it's its itself just least less let let's like many may me might mine more most mostly much must mustn't my myself near neither never next no nobody none nor not nothing now nowhere of off often on once one ones only onto or other others otherwise ought our ours ourselves out outside over own per perhaps quite rather really same shall shan't she she'd she'll she's should shouldn't since so some somebody someone something sometimes somewhere soon still such than that that's the their theirs them themselves then there there's therefore these they they'd they'll they're they've this those though through throughout thus till to together too toward towards under unless until up upon us very via was wasn't we we'd we'll we're we've well were weren't what what's whatever when when's whenever where where's whereas wherever whether which while who who's whoever whole whom whose why why's will with within without won't would wouldn't yet you you'd you'll you're you've your yours yourself yourselves yes oh ok okay mr mrs ms dr etc";

/// Built-in irregular forms (form, lemma): common verbs and plurals.
const IRREGULAR: &str = "arose arise|arisen arise|awoke awake|awoken awake|was be|were be|been be|bore bear|borne bear|beat beat|beaten beat|began begin|begun begin|bent bend|bet bet|bit bite|bitten bite|bled bleed|blew blow|blown blow|broke break|broken break|bred breed|brought bring|built build|burnt burn|burst burst|bought buy|caught catch|chose choose|chosen choose|clung cling|came come|cost cost|crept creep|cut cut|dealt deal|dug dig|did do|done do|drew draw|drawn draw|dreamt dream|drank drink|drunk drink|drove drive|driven drive|ate eat|eaten eat|fell fall|fallen fall|fed feed|felt feel|fought fight|found find|fled flee|flew fly|flown fly|forbade forbid|forbidden forbid|forgot forget|forgotten forget|forgave forgive|forgiven forgive|froze freeze|frozen freeze|got get|gotten get|gave give|given give|went go|gone go|ground grind|grew grow|grown grow|hung hang|had have|has have|heard hear|hid hide|hidden hide|hit hit|held hold|hurt hurt|kept keep|knelt kneel|knew know|known know|laid lay|led lead|leant lean|leapt leap|learnt learn|left leave|lent lend|let let|lay lie|lain lie|lit light|lost lose|made make|meant mean|met meet|paid pay|put put|quit quit|read read|rode ride|ridden ride|rang ring|rung ring|rose rise|risen rise|ran run|said say|saw see|seen see|sought seek|sold sell|sent send|set set|shook shake|shaken shake|shone shine|shot shoot|showed show|shown show|shrank shrink|shrunk shrink|shut shut|sang sing|sung sing|sank sink|sunk sink|sat sit|slept sleep|slid slide|spoke speak|spoken speak|spent spend|spun spin|spat spit|split split|spread spread|sprang spring|sprung spring|stood stand|stole steal|stolen steal|stuck stick|stung sting|stank stink|struck strike|strove strive|swore swear|sworn swear|swept sweep|swam swim|swum swim|swung swing|took take|taken take|taught teach|tore tear|torn tear|told tell|thought think|threw throw|thrown throw|understood understand|woke wake|woken wake|wore wear|worn wear|wept weep|won win|wound wind|withdrew withdraw|withdrawn withdraw|wrote write|written write|children child|men man|women woman|people person|mice mouse|feet foot|teeth tooth|geese goose|lives life|knives knife|wives wife|leaves leaf|halves half|selves self|shelves shelf|wolves wolf|thieves thief|loaves loaf|better good|best good|worse bad|worst bad|further far|farther far|data data|news news|series series|species species|criteria criterion|phenomena phenomenon|analyses analysis|crises crisis|theses thesis";

fn stop() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| STOP.split_whitespace().collect())
}

fn irregular() -> &'static BTreeMap<&'static str, &'static str> {
    static MAP: OnceLock<BTreeMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        IRREGULAR
            .split('|')
            .filter_map(|pair| pair.split_once(' '))
            .collect()
    })
}

/// True for a function word of the stop list.
pub fn is_stop(word: &str) -> bool {
    stop().contains(word.to_lowercase().as_str())
}

fn vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// Lemma guesses for a lowercase word, the likeliest first: the irregular
/// tables (`extra` holds forms parsed from dictionary pages), then the suffix
/// rules. The word itself is always the last guess.
pub fn lemmas(word: &str, extra: &BTreeMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if s.len() >= 2 && !out.contains(&s) {
            out.push(s);
        }
    };
    if let Some(lemma) = extra.get(word) {
        push(lemma.clone());
    }
    if let Some(lemma) = irregular().get(word) {
        push((*lemma).to_owned());
    }
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    let stem = |cut: usize| chars[..n - cut].iter().collect::<String>();
    let undouble = |s: &str| {
        let c: Vec<char> = s.chars().collect();
        let k = c.len();
        (k >= 3
            && c[k - 1] == c[k - 2]
            && !vowel(c[k - 1])
            && !matches!(c[k - 1], 'l' | 's' | 'z' | 'f'))
        .then(|| c[..k - 1].iter().collect::<String>())
    };
    // A short consonant-vowel-consonant stem often lost an `e` (making, hoped).
    let with_e = |s: &str| {
        let c: Vec<char> = s.chars().collect();
        let k = c.len();
        (k >= 2
            && (matches!(c[k - 1], 'v' | 'z' | 'c' | 'u')
                || ((3..=4).contains(&k)
                    && !vowel(c[k - 1])
                    && vowel(c[k - 2])
                    && !vowel(c[k - 3])
                    && !matches!(c[k - 1], 'w' | 'x' | 'y'))))
        .then(|| format!("{s}e"))
    };
    if n > 4 && word.ends_with("ies") {
        push(format!("{}y", stem(3)));
    } else if n > 4
        && (word.ends_with("sses")
            || word.ends_with("xes")
            || word.ends_with("zes")
            || word.ends_with("ches")
            || word.ends_with("shes"))
    {
        push(stem(2));
    } else if n > 3
        && word.ends_with('s')
        && !word.ends_with("ss")
        && !word.ends_with("us")
        && !word.ends_with("is")
    {
        push(stem(1));
    }
    if n > 5 && word.ends_with("ing") {
        let s = stem(3);
        if let Some(u) = undouble(&s) {
            push(u);
        }
        if let Some(e) = with_e(&s) {
            push(e);
        }
        push(s);
    }
    if n > 4 && word.ends_with("ied") {
        push(format!("{}y", stem(3)));
    } else if n > 3 && word.ends_with("ed") {
        let s = stem(2);
        if let Some(u) = undouble(&s) {
            push(u);
        }
        if word.ends_with("eed") {
            push(stem(1));
        }
        if let Some(e) = with_e(&s) {
            push(e);
        }
        push(s);
        push(stem(1));
    }
    if n > 4 && word.ends_with("ier") {
        push(format!("{}y", stem(3)));
    } else if n > 5 && word.ends_with("iest") {
        push(format!("{}y", stem(4)));
    }
    push(word.to_owned());
    out
}

/// The content words of one English sentence; `extra` as for [`lemmas`].
pub fn words(text: &str, extra: &BTreeMap<String, String>) -> Vec<Word> {
    let mut out = Vec::new();
    let mut sentence_start = true;
    let mut current = String::new();
    let flush = |current: &mut String, sentence_start: &mut bool, out: &mut Vec<Word>| {
        let raw = current.trim_matches('\'').to_owned();
        current.clear();
        if raw.is_empty() {
            return;
        }
        let capital = raw.chars().next().is_some_and(char::is_uppercase);
        let start = std::mem::replace(sentence_start, false);
        let lower = raw.to_lowercase();
        let lower = lower.strip_suffix("'s").unwrap_or(&lower).to_owned();
        if (capital && !start) || is_stop(&lower) || lower.chars().count() < 2 {
            return;
        }
        let forms = lemmas(&lower, extra);
        if forms.iter().any(|f| is_stop(f)) {
            return;
        }
        out.push(Word {
            base: forms[0].clone(),
            forms,
            reading: String::new(),
            pos: String::new(),
        });
    };
    for c in text.chars() {
        if c.is_alphabetic() || ((c == '\'' || c == '’') && !current.is_empty()) {
            current.push(if c == '’' { '\'' } else { c });
        } else {
            let digit = c.is_ascii_digit();
            if digit {
                // A word with a digit is not a word (3D, covid19).
                current.clear();
            }
            flush(&mut current, &mut sentence_start, &mut out);
            if matches!(c, '.' | '!' | '?' | '"' | '“' | ':' | '\n') {
                sentence_start = true;
            }
        }
    }
    flush(&mut current, &mut sentence_start, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(word: &str) -> String {
        lemmas(word, &BTreeMap::new())[0].clone()
    }

    #[test]
    fn lemmas_follow_the_rules_and_the_irregular_table() {
        assert_eq!(first("running"), "run");
        assert_eq!(first("studies"), "study");
        assert_eq!(first("went"), "go");
        assert_eq!(first("data"), "data");
        assert_eq!(first("boxes"), "box");
        assert_eq!(first("stopped"), "stop");
        assert!(lemmas("making", &BTreeMap::new()).contains(&"make".to_owned()));
        assert!(lemmas("visited", &BTreeMap::new()).contains(&"visit".to_owned()));
        let extra = BTreeMap::from([("supernovae".to_owned(), "supernova".to_owned())]);
        assert_eq!(lemmas("supernovae", &extra)[0], "supernova");
    }

    #[test]
    fn names_stop_words_and_numbers_are_dropped() {
        let bases: Vec<String> = words(
            "Yesterday Tom bought 3 apples and a 3D printer.",
            &BTreeMap::new(),
        )
        .into_iter()
        .map(|w| w.base)
        .collect();
        assert_eq!(bases, ["yesterday", "buy", "apple", "printer"]);
    }
}
