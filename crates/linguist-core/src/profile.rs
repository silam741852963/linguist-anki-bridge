//! WP-27 derived card profile: the part-of-speech class and the other facts
//! the dictionary labels state, read through the label table
//! `resources/taxonomy/labels-v1.tsv`. No LLM; computed at render time.
use crate::document::{DictionaryEntry, Grammar, Vocabulary};
use crate::review::same_spelling;
use std::collections::BTreeSet;
use std::sync::OnceLock;

const LABELS: &str = include_str!("../../../resources/taxonomy/labels-v1.tsv");

/// The learner classes, grouped from Universal Dependencies v2 UPOS. Each has
/// one card colour.
pub const CLASSES: [&str; 10] = [
    "noun",
    "verb",
    "adjective",
    "adverb",
    "pronoun",
    "function",
    "number",
    "expression",
    "affix",
    "interjection",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Pos,
    Register,
    Region,
    Field,
    Script,
    Origin,
    Note,
    Name,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LabelRow {
    pub language: &'static str,
    pub label: &'static str,
    pub role: Role,
    pub value: &'static str,
}

fn parse(line: &'static str) -> Result<LabelRow, String> {
    let cells: Vec<&'static str> = line.split('\t').collect();
    let [language, label, role, value] = cells[..] else {
        return Err(format!("LABEL_ROW_INVALID: {line}"));
    };
    let role = match role {
        "pos" => Role::Pos,
        "register" => Role::Register,
        "region" => Role::Region,
        "field" => Role::Field,
        "script" => Role::Script,
        "origin" => Role::Origin,
        "note" => Role::Note,
        "name" => Role::Name,
        other => return Err(format!("LABEL_ROLE_UNKNOWN: {other}")),
    };
    if role == Role::Pos && !CLASSES.contains(&value.split("::").next().unwrap_or_default()) {
        return Err(format!("LABEL_CLASS_UNKNOWN: {label}"));
    }
    Ok(LabelRow {
        language,
        label,
        role,
        value,
    })
}

/// Every row of the shipped table. A malformed row is a build defect caught
/// by the unit tests, so it panics here.
pub fn table() -> &'static [LabelRow] {
    static TABLE: OnceLock<Vec<LabelRow>> = OnceLock::new();
    TABLE.get_or_init(|| {
        LABELS
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| parse(line).expect("labels-v1.tsv"))
            .collect()
    })
}

/// The row for a provider label; `language` may carry a region ("ja-JP").
pub fn label(language: &str, label: &str) -> Option<&'static LabelRow> {
    let language = language.split('-').next().unwrap_or_default();
    table()
        .iter()
        .find(|row| row.language == language && row.label == label.trim())
}

/// The derived profile of one card. `class` is `None` when no label, entry or
/// pattern slot names it; the card is then tagged `lab::pos::unknown`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Derived {
    pub class: Option<&'static str>,
    pub subclasses: BTreeSet<&'static str>,
    pub also: BTreeSet<&'static str>,
    pub register: BTreeSet<&'static str>,
    pub region: BTreeSet<&'static str>,
    pub field: BTreeSet<&'static str>,
    pub script: BTreeSet<&'static str>,
    pub origin: BTreeSet<&'static str>,
    pub note: BTreeSet<&'static str>,
    /// Grammar: the forms the pattern requires of its slots (`te`, `nai`).
    pub forms: BTreeSet<&'static str>,
}

impl Derived {
    pub fn tags(&self) -> Vec<String> {
        let mut tags = vec![format!("lab::pos::{}", self.class.unwrap_or("unknown"))];
        if let Some(class) = self.class {
            for sub in &self.subclasses {
                tags.push(format!("lab::pos::{class}::{sub}"));
            }
        }
        for (prefix, values) in [
            ("pos-also", &self.also),
            ("register", &self.register),
            ("region", &self.region),
            ("field", &self.field),
            ("script", &self.script),
            ("origin", &self.origin),
            ("note", &self.note),
            ("form", &self.forms),
        ] {
            for value in values {
                tags.push(format!("lab::{prefix}::{value}"));
            }
        }
        tags
    }

    /// Record one label's role. The first part-of-speech label names the
    /// main class; a later label of another class is an other use.
    fn add(&mut self, row: &LabelRow) {
        let set = match row.role {
            Role::Pos => {
                let (class, sub) = match row.value.split_once("::") {
                    Some((class, sub)) => (class, Some(sub)),
                    None => (row.value, None),
                };
                let class = CLASSES.iter().copied().find(|c| *c == class);
                match (self.class, class) {
                    (_, None) => {}
                    (None, Some(class)) => self.class = Some(class),
                    (Some(main), Some(class)) if main != class => {
                        self.also.insert(class);
                        return;
                    }
                    _ => {}
                }
                if let Some(sub) = sub {
                    self.subclasses.insert(sub);
                }
                return;
            }
            Role::Register => &mut self.register,
            Role::Region => &mut self.region,
            Role::Field => &mut self.field,
            Role::Script => &mut self.script,
            Role::Origin => &mut self.origin,
            Role::Note => &mut self.note,
            Role::Name => return,
        };
        if !row.value.is_empty() {
            set.insert(row.value);
        }
    }
}

fn pos_rows<'a>(
    language: &'a str,
    labels: &'a [String],
) -> impl Iterator<Item = &'static LabelRow> + 'a {
    labels
        .iter()
        .filter_map(move |l| label(language, l))
        .filter(|row| row.role == Role::Pos)
}

/// The part-of-speech labels an entry states for one sense: Cambridge's entry
/// part of speech first, then the sense's own labels. A Jisho sense with none
/// takes those of the nearest sense before it, as Jisho lists them once.
fn sense_pos(language: &str, entry: &DictionaryEntry, index: usize) -> Vec<&'static LabelRow> {
    let mut rows: Vec<_> = entry
        .metadata
        .get("part_of_speech")
        .into_iter()
        .flatten()
        .filter_map(|l| label(language, l))
        .filter(|row| row.role == Role::Pos)
        .collect();
    if let Some(sense) = (0..=index)
        .rev()
        .filter_map(|i| entry.senses.get(i))
        .find(|sense| pos_rows(language, &sense.labels).next().is_some())
    {
        rows.extend(pos_rows(language, &sense.labels));
    }
    rows
}

fn class_of(row: &LabelRow) -> &str {
    row.value.split("::").next().unwrap_or_default()
}

/// The classes of one sense of `entry`, first label first, for its colour.
pub fn sense_classes(language: &str, entry: &DictionaryEntry, index: usize) -> Vec<&'static str> {
    let mut classes = Vec::new();
    for row in sense_pos(language, entry, index) {
        let class = class_of(row);
        if !classes.contains(&class) {
            classes.push(class);
        }
    }
    classes
}

/// The class a part-of-speech label names, for its chip colour.
pub fn label_class(language: &str, text: &str) -> Option<&'static str> {
    label(language, text)
        .filter(|row| row.role == Role::Pos)
        .map(class_of)
}

/// Vocabulary: the selected sense's labels; fallback, another entry that lists
/// the same form. Other senses of the entry, and other Cambridge entries of
/// the word, give the other uses.
pub fn vocabulary(language: &str, v: &Vocabulary) -> Derived {
    let mut derived = Derived::default();
    let expression = crate::review::dictionary_form(v);
    let lists = |entry: &&DictionaryEntry| {
        entry
            .forms
            .iter()
            .chain(&entry.readings)
            .any(|form| same_spelling(form, &expression))
    };
    let selected = v
        .dictionary
        .iter()
        .filter(lists)
        .find_map(|entry| {
            let index = entry.senses.iter().position(|s| s.key == v.sense_key)?;
            Some((entry, index))
        })
        .or_else(|| {
            v.dictionary.iter().find_map(|entry| {
                let index = entry.senses.iter().position(|s| s.key == v.sense_key)?;
                Some((entry, index))
            })
        });
    if let Some((entry, index)) = selected {
        for row in sense_pos(language, entry, index) {
            derived.add(row);
        }
        for l in &entry.senses[index].labels {
            if let Some(row) = label(language, l).filter(|row| row.role != Role::Pos) {
                derived.add(row);
            }
        }
        let mut others: Vec<&'static str> = (0..entry.senses.len())
            .filter(|i| *i != index)
            .flat_map(|i| pos_rows(language, &entry.senses[i].labels).map(class_of))
            .collect();
        for other in v.dictionary.iter().filter(lists) {
            if other.provider == entry.provider
                && other.provider == "cambridge-html-v1"
                && !std::ptr::eq(other, entry)
            {
                others.extend(sense_pos(language, other, 0).into_iter().map(class_of));
            }
        }
        derived.also.extend(
            others
                .into_iter()
                .filter_map(|class| CLASSES.iter().copied().find(|c| *c == class)),
        );
    }
    if derived.class.is_none() {
        let fallback = v.dictionary.iter().filter(lists).find_map(|entry| {
            let rows = (0..entry.senses.len())
                .map(|i| sense_pos(language, entry, i))
                .find(|rows| !rows.is_empty())?;
            Some(rows)
        });
        for row in fallback.into_iter().flatten() {
            derived.add(row);
        }
    }
    if language.starts_with("ja")
        && !expression.is_empty()
        && expression
            .chars()
            .all(|c| matches!(c, '\u{30a0}'..='\u{30ff}' | '\u{31f0}'..='\u{31ff}'))
        && !derived.origin.contains("wasei")
    {
        derived.origin.insert("gairaigo");
    }
    if let Some(class) = derived.class {
        derived.also.remove(class);
    }
    derived
}

/// One slot a grammar pattern attaches to, read from a token such as `V`,
/// `V2`, `Nの`, `い-Adj` or `な-adjective`.
fn slot(token: &str) -> Option<(&'static str, Option<&'static str>)> {
    let token = token.trim();
    let rest = |prefix: char| {
        let tail = token.strip_prefix(prefix)?;
        let tail = tail.trim_start_matches(|c: char| c.is_ascii_digit());
        // `Verb`, `Note`, `NOT`: an ASCII word, not a slot.
        (!tail.starts_with(|c: char| c.is_ascii_alphabetic())).then_some(tail)
    };
    let lower = token.to_lowercase();
    match lower.as_str() {
        "verb" | "verbs" => return Some(("verb", None)),
        "noun" | "nouns" => return Some(("noun", None)),
        "passive" | "causative" | "potential" | "volitional" | "imperative" => {
            return Some(("verb", verb_form(&lower)));
        }
        _ => {}
    }
    if lower.contains("adj") {
        let sub = if token.starts_with('い') {
            Some("i")
        } else if token.starts_with('な') {
            Some("na")
        } else {
            None
        };
        return Some(("adjective", sub));
    }
    if let Some(tail) = rest('V').or_else(|| rest('Ｖ')) {
        return Some(("verb", verb_form(tail.trim_start_matches(['-', ' ']))));
    }
    if let Some(tail) = rest('N').or_else(|| rest('Ｎ')) {
        return Some(("noun", tail.starts_with('の').then_some("noun-no")));
    }
    None
}

/// The verb form a slot names, from the text right after `V`.
fn verb_form(text: &str) -> Option<&'static str> {
    const FORMS: [(&str, &str); 18] = [
        ("て", "te"),
        ("で", "te"),
        ("ない", "nai"),
        ("なく", "nai"),
        ("ず", "nai"),
        ("た", "ta"),
        ("ます", "masu"),
        ("dictionary", "dictionary"),
        ("る", "dictionary"),
        ("volitional", "volitional"),
        ("passive", "passive"),
        ("causative", "causative"),
        ("potential", "potential"),
        ("conditional", "conditional"),
        ("ば", "conditional"),
        ("imperative", "imperative"),
        ("plain", "plain"),
        ("たら", "ta"),
    ];
    let text = text.trim();
    FORMS
        .iter()
        .filter(|(key, _)| text.starts_with(key))
        .max_by_key(|(key, _)| key.len())
        .map(|(_, form)| *form)
}

fn tokens(text: &str) -> Vec<&str> {
    text.split(|c: char| c.is_whitespace() || "／/()（）+＋、,，・…〜~;；「」[]［］".contains(c))
        .filter(|t| !t.is_empty())
        .collect()
}

/// Grammar: the class of the slot the pattern attaches to, read from the
/// pattern, else from the formation. The first slot is the main class.
pub fn grammar(g: &Grammar) -> Derived {
    let mut derived = Derived::default();
    let mut clause = false;
    for text in [&g.pattern, &g.formation] {
        let tokens = tokens(text);
        for (i, token) in tokens.iter().enumerate() {
            if let Some((class, sub)) = slot(token) {
                match derived.class {
                    None => derived.class = Some(class),
                    Some(main) if main != class => {
                        derived.also.insert(class);
                    }
                    _ => {}
                }
                let next = tokens.get(i + 1).copied().unwrap_or_default();
                let form = match (class, sub) {
                    ("verb", None) => verb_form(next),
                    ("noun", None) => next.starts_with('の').then_some("noun-no"),
                    (_, sub) => sub,
                };
                match form {
                    Some(form @ ("i" | "na")) => {
                        derived.subclasses.insert(form);
                    }
                    Some(form) => {
                        derived.forms.insert(form);
                    }
                    None => {}
                }
            } else if *token == "plain" && tokens.get(i + 1) == Some(&"form") {
                derived.forms.insert("plain");
                clause = true;
            } else if matches!(*token, "S" | "S1" | "S2" | "sentence") {
                clause = true;
            }
        }
        if derived.class.is_some() {
            break;
        }
    }
    // A pattern that joins or ends a clause, with no word slot, is a
    // function word (UPOS SCONJ, CCONJ, PART).
    if derived.class.is_none() && clause {
        derived.class = Some("function");
    }
    derived
}

const DOMAINS: &str = include_str!("../../../resources/taxonomy/domains-v1.json");
/// The taxonomy version a judged profile is checked against.
pub const TAXONOMY: &str = "domains-v1";
/// At most this many secondary domains, and grammar functions.
pub const SECONDARY_MAX: usize = 2;
pub const FUNCTIONS_MAX: usize = 2;

/// The shipped domain taxonomy: three-level leaf paths, grammar communicative
/// functions (`group::function`) and the JMdict field hints, keyed by the
/// `field` value of the label table.
#[derive(Debug)]
pub struct Taxonomy {
    pub leaves: Vec<String>,
    pub functions: Vec<String>,
    pub hints: std::collections::BTreeMap<String, String>,
}

pub fn taxonomy() -> &'static Taxonomy {
    static TAXONOMY_CELL: OnceLock<Taxonomy> = OnceLock::new();
    TAXONOMY_CELL.get_or_init(|| {
        let value: serde_json::Value = serde_json::from_str(DOMAINS).expect("domains-v1.json");
        let ids = |node: &serde_json::Value| node["id"].as_str().unwrap_or_default().to_owned();
        let children =
            |node: &serde_json::Value| node["children"].as_array().cloned().unwrap_or_default();
        let mut leaves = Vec::new();
        for tree in value["trees"].as_array().into_iter().flatten() {
            for l1 in tree["nodes"].as_array().into_iter().flatten() {
                for l2 in children(l1) {
                    for l3 in children(&l2) {
                        leaves.push(format!("{}::{}::{}", ids(l1), ids(&l2), ids(&l3)));
                    }
                }
            }
        }
        let functions = value["functions"]
            .as_object()
            .into_iter()
            .flatten()
            .flat_map(|(group, list)| {
                list.as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f.as_str())
                    .map(move |f| format!("{group}::{f}"))
            })
            .collect();
        let hints = value["field_hints"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
            .collect();
        Taxonomy {
            leaves,
            functions,
            hints,
        }
    })
}

/// Why a judged profile is not valid for the shipped taxonomy, if it is not.
pub fn judged_problem(profile: &crate::document::JudgedProfile, grammar: bool) -> Option<String> {
    let t = taxonomy();
    let leaf = |path: &String| t.leaves.contains(path);
    if profile.taxonomy != TAXONOMY {
        return Some(format!("taxonomy {} is not {TAXONOMY}", profile.taxonomy));
    }
    if !leaf(&profile.domain) {
        return Some(format!(
            "domain {} is not a {TAXONOMY} leaf",
            profile.domain
        ));
    }
    if profile.secondary.len() > SECONDARY_MAX {
        return Some("more than two secondary domains".into());
    }
    let mut seen = BTreeSet::from([&profile.domain]);
    if let Some(path) = profile
        .secondary
        .iter()
        .find(|p| !leaf(p) || !seen.insert(p))
    {
        return Some(format!("secondary domain {path} is unknown or repeated"));
    }
    if !grammar && !profile.functions.is_empty() {
        return Some("functions are for grammar only".into());
    }
    if profile.functions.len() > FUNCTIONS_MAX
        || profile.functions.iter().any(|f| !t.functions.contains(f))
    {
        return Some("unknown or too many functions".into());
    }
    None
}

/// Tags of a judged profile: `lab::domain::<path>`, `lab::domain2::<path>`
/// and `lab::function::<group>::<function>`. An invalid profile gives none.
pub fn judged_tags(profile: Option<&crate::document::JudgedProfile>, grammar: bool) -> Vec<String> {
    let Some(profile) = profile.filter(|p| judged_problem(p, grammar).is_none()) else {
        return Vec::new();
    };
    std::iter::once(format!("lab::domain::{}", profile.domain))
        .chain(
            profile
                .secondary
                .iter()
                .map(|p| format!("lab::domain2::{p}")),
        )
        .chain(
            profile
                .functions
                .iter()
                .map(|f| format!("lab::function::{f}")),
        )
        .collect()
}

/// The `domains-v1` list as the enrichment prompt gives it: by tree, one
/// line per level-2 node (`l1::l2: l3, l3, …`), each level-1 node with its
/// scope note; grammar also gets the closed function list.
pub fn prompt_list(grammar: bool) -> String {
    let value: serde_json::Value = serde_json::from_str(DOMAINS).expect("domains-v1.json");
    let text =
        |node: &serde_json::Value, key: &str| node[key].as_str().unwrap_or_default().to_owned();
    let list = |node: &serde_json::Value| node["children"].as_array().cloned().unwrap_or_default();
    let mut out = String::new();
    for tree in value["trees"].as_array().into_iter().flatten() {
        let name = match tree["tree"].as_str() {
            Some("A") => "A, everyday life",
            Some("B") => "B, specialist fields",
            _ => "C, general notions",
        };
        out.push_str(&format!("Tree {name}:\n"));
        for l1 in tree["nodes"].as_array().into_iter().flatten() {
            let scope = text(l1, "scope");
            if !scope.is_empty() {
                out.push_str(&format!("# {}: {scope}\n", text(l1, "id")));
            }
            for l2 in list(l1) {
                let leaves: Vec<String> = list(&l2).iter().map(|l3| text(l3, "id")).collect();
                out.push_str(&format!(
                    "{}::{}: {}\n",
                    text(l1, "id"),
                    text(&l2, "id"),
                    leaves.join(", ")
                ));
            }
        }
    }
    if grammar {
        out.push_str("Functions:\n");
        for (group, functions) in value["functions"].as_object().into_iter().flatten() {
            let names: Vec<&str> = functions
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str())
                .collect();
            out.push_str(&format!("{group}: {}\n", names.join(", ")));
        }
    }
    out
}

/// Domain hints for one vocabulary card: the taxonomy leaf of each field label
/// of the selected sense, and Cambridge's SMART Vocabulary topic as written.
pub fn domain_hints(language: &str, v: &Vocabulary) -> Vec<String> {
    let mut hints = Vec::new();
    for field in vocabulary(language, v).field {
        if let Some(leaf) = taxonomy().hints.get(field)
            && !hints.contains(leaf)
        {
            hints.push(leaf.clone());
        }
    }
    let selected = v
        .dictionary
        .iter()
        .find(|entry| entry.senses.iter().any(|s| s.key == v.sense_key));
    for topic in selected
        .and_then(|entry| entry.metadata.get("smart_vocabulary_topic"))
        .into_iter()
        .flatten()
    {
        let topic = format!("topic: {}", topic.trim());
        if !hints.contains(&topic) {
            hints.push(topic);
        }
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn label_table_rows_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for line in LABELS
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let row = parse(line).unwrap();
            assert!(
                seen.insert((row.language, row.label)),
                "duplicate {}",
                row.label
            );
            assert!(matches!(row.language, "ja" | "en"), "{line}");
            if row.role == Role::Pos {
                assert!(row.value.split("::").count() <= 2, "{line}");
            }
            assert!(
                row.value
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == ':'),
                "tag value {line}"
            );
        }
        assert!(!table().is_empty());
    }

    #[test]
    fn taxonomy_has_the_documented_size_and_every_hint_is_a_leaf() {
        let t = taxonomy();
        assert_eq!(t.leaves.len(), 293 + 77 + 89);
        let unique: BTreeSet<_> = t.leaves.iter().collect();
        assert_eq!(unique.len(), t.leaves.len());
        assert!(t.leaves.iter().all(|p| p.split("::").count() == 3));
        let value: serde_json::Value = serde_json::from_str(DOMAINS).unwrap();
        let sizes: Vec<(usize, usize)> = value["trees"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tree| {
                let nodes = tree["nodes"].as_array().unwrap();
                let l2 = nodes
                    .iter()
                    .map(|n| n["children"].as_array().unwrap().len())
                    .sum();
                (nodes.len(), l2)
            })
            .collect();
        assert_eq!(sizes, [(15, 72), (10, 26), (7, 21)]);
        for leaf in t.hints.values() {
            assert!(t.leaves.contains(leaf), "hint {leaf}");
        }
        for row in table().iter().filter(|r| r.role == Role::Field) {
            assert!(
                t.hints.contains_key(row.value),
                "no hint for field {}",
                row.value
            );
        }
        assert!(t.functions.contains(&"suasion::requesting".to_owned()));
    }

    #[test]
    fn judged_profiles_are_checked_against_the_taxonomy() {
        let profile =
            |domain: &str, secondary: &[&str], functions: &[&str]| crate::document::JudgedProfile {
                taxonomy: TAXONOMY.into(),
                domain: domain.into(),
                secondary: secondary.iter().map(|s| s.to_string()).collect(),
                functions: functions.iter().map(|s| s.to_string()).collect(),
            };
        let good = profile(
            "food::meals::breakfast",
            &["health::wellbeing::diet-exercise"],
            &[],
        );
        assert_eq!(judged_problem(&good, false), None);
        assert_eq!(
            judged_tags(Some(&good), false),
            [
                "lab::domain::food::meals::breakfast",
                "lab::domain2::health::wellbeing::diet-exercise"
            ]
        );
        assert!(judged_problem(&profile("food::meals", &[], &[]), false).is_some());
        assert!(
            judged_problem(
                &profile("food::meals::breakfast", &["food::meals::breakfast"], &[]),
                false
            )
            .is_some()
        );
        assert!(
            judged_problem(
                &profile(
                    "relational::logic::condition",
                    &[],
                    &["suasion::requesting"]
                ),
                false
            )
            .is_some()
        );
        let grammar = profile(
            "relational::logic::condition",
            &[],
            &["suasion::requesting"],
        );
        assert_eq!(judged_problem(&grammar, true), None);
        assert!(judged_tags(Some(&profile("x::y::z", &[], &[])), false).is_empty());
    }

    #[test]
    fn non_part_of_speech_labels_never_name_a_class() {
        for name in [
            "Wikipedia definition",
            "Computing",
            "Usually written using kana alone",
        ] {
            assert_ne!(label("ja", name).unwrap().role, Role::Pos);
        }
        assert_eq!(label("ja-JP", "Noun").unwrap().value, "noun");
        assert!(label("en", "money").is_none());
    }

    fn entry(
        provider: &str,
        forms: &[&str],
        senses: serde_json::Value,
        pos: &[&str],
    ) -> serde_json::Value {
        serde_json::json!({
            "provider": provider, "source_url": "https://example.org", "language": "ja",
            "forms": forms, "readings": [], "senses": senses,
            "metadata": if pos.is_empty() { serde_json::json!({}) } else { serde_json::json!({"part_of_speech": pos}) },
        })
    }

    fn vocab(expression: &str, sense_key: &str, entries: Vec<serde_json::Value>) -> Vocabulary {
        serde_json::from_value(serde_json::json!({
            "expression": expression, "meaning": "m", "sense_key": sense_key,
            "dictionary": entries,
        }))
        .unwrap()
    }

    #[test]
    fn jisho_sense_gives_class_subclasses_other_uses_and_facts() {
        let v = vocab(
            "対応",
            "s1",
            vec![entry(
                "jisho-api-v1",
                &["対応"],
                serde_json::json!([
                    {"key":"s1","definitions":["correspondence"],"labels":["Noun","Suru verb","Intransitive verb","Computing"]},
                    {"key":"s2","definitions":["support"],"labels":[]},
                    {"key":"s3","definitions":["x"],"labels":["Wikipedia definition"]}
                ]),
                &[],
            )],
        );
        let d = vocabulary("ja", &v);
        assert_eq!(d.class, Some("noun"));
        assert_eq!(d.also, BTreeSet::from(["verb"]));
        assert_eq!(d.field, BTreeSet::from(["computing"]));
        let tags = d.tags();
        assert!(tags.contains(&"lab::pos::noun".to_owned()));
        assert!(tags.contains(&"lab::pos-also::verb".to_owned()));
        assert!(
            !tags
                .iter()
                .any(|t| t.contains("wikipedia") || t == "lab::pos::computing")
        );
    }

    #[test]
    fn a_jisho_sense_without_labels_takes_the_sense_before_it() {
        let v = vocab(
            "祈る",
            "s2",
            vec![entry(
                "jisho-api-v1",
                &["祈る"],
                serde_json::json!([
                    {"key":"s1","definitions":["pray"],"labels":["Godan verb with 'ru' ending","Transitive verb"]},
                    {"key":"s2","definitions":["wish"],"labels":[]}
                ]),
                &[],
            )],
        );
        let d = vocabulary("ja", &v);
        assert_eq!(d.class, Some("verb"));
        assert_eq!(d.subclasses, BTreeSet::from(["godan-ru", "transitive"]));
    }

    #[test]
    fn cambridge_entry_part_of_speech_and_other_entries_of_the_word() {
        let v = vocab(
            "issue",
            "n1",
            vec![
                entry(
                    "cambridge-html-v1",
                    &["issue"],
                    serde_json::json!([{"key":"n1","definitions":["a subject"],"labels":["noun","subject"]}]),
                    &["noun"],
                ),
                entry(
                    "cambridge-html-v1",
                    &["issue"],
                    serde_json::json!([{"key":"v1","definitions":["to produce"],"labels":["verb","produce"]}]),
                    &["verb"],
                ),
            ],
        );
        let d = vocabulary("en", &v);
        assert_eq!(d.class, Some("noun"));
        assert_eq!(d.also, BTreeSet::from(["verb"]));
    }

    #[test]
    fn no_label_anywhere_is_unknown_and_katakana_is_gairaigo() {
        let v = vocab("ホテル", "s1", vec![]);
        let d = vocabulary("ja", &v);
        assert_eq!(d.class, None);
        assert!(d.tags().contains(&"lab::pos::unknown".to_owned()));
        assert_eq!(d.origin, BTreeSet::from(["gairaigo"]));
    }

    fn grammar_of(pattern: &str, formation: &str) -> Derived {
        let g: Grammar = serde_json::from_value(serde_json::json!({
            "pattern": pattern, "use_key": "k", "meaning": "m", "formation": formation,
            "recognition_prompt": "", "examples": [],
        }))
        .unwrap();
        grammar(&g)
    }

    #[test]
    fn grammar_class_comes_from_the_slot_it_attaches_to() {
        let d = grammar_of("V て ください", "");
        assert_eq!(
            (d.class, d.forms.clone()),
            (Some("verb"), BTreeSet::from(["te"]))
        );
        assert_eq!(grammar_of("N1 は N2 です", "").class, Some("noun"));
        let d = grammar_of("い-Adj くて", "");
        assert_eq!(
            (d.class, d.subclasses.clone()),
            (Some("adjective"), BTreeSet::from(["i"]))
        );
        let d = grammar_of("V1 た／N の あとで、V2", "");
        assert_eq!(d.class, Some("verb"));
        assert_eq!(d.also, BTreeSet::from(["noun"]));
        assert_eq!(
            grammar_of("V-ます っぱなし", "").forms,
            BTreeSet::from(["masu"])
        );
        assert_eq!(grammar_of("Vてごらん", "").forms, BTreeSet::from(["te"]));
        assert_eq!(
            grammar_of("Nの ように", "").forms,
            BTreeSet::from(["noun-no"])
        );
    }

    #[test]
    fn grammar_without_a_slot_reads_the_formation_then_is_unknown() {
        assert_eq!(grammar_of("〜ても", "V て-form + も").class, Some("verb"));
        assert_eq!(
            grammar_of("つまり", "sentence + つまり + sentence").class,
            Some("function")
        );
        assert_eq!(grammar_of("S1 が、S2", "").class, Some("function"));
        assert_eq!(
            grammar_of("causative て いただけませんか", "").forms,
            BTreeSet::from(["causative"])
        );
        assert_eq!(grammar_of("これ／それ／あれ", "").class, None);
        assert_eq!(grammar_of("NG Vox Note", "").class, None);
    }
}
