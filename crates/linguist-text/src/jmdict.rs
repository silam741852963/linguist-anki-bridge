//! An optional local JMdict file (EDRDG, CC BY-SA 4.0, installed by the user):
//! written forms, readings and priority tags. The harvester uses it to keep a
//! compound whole, to give a reading, and to rank by frequency.
use std::collections::BTreeMap;

/// One headword of the file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub forms: Vec<String>,
    pub readings: Vec<String>,
    /// The best `nfXX` frequency band (1 is most frequent, 48 least), or 49
    /// with `news1`/`ichi1`/`spec1`/`gai1` alone, or none.
    pub frequency: Option<u8>,
}

/// Forms and readings to entries.
#[derive(Debug, Default)]
pub struct Jmdict {
    entries: Vec<Entry>,
    index: BTreeMap<String, Vec<usize>>,
}

fn contents<'a>(entry: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = entry;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find(&close) else { break };
        out.push(after[..end].trim());
        rest = &after[end + close.len()..];
    }
    out
}

impl Jmdict {
    /// Parse JMdict XML text (the `JMdict_e` or `JMdict` file).
    pub fn parse(xml: &str) -> Self {
        let mut dict = Jmdict::default();
        let mut rest = xml;
        while let Some(start) = rest.find("<entry>") {
            let after = &rest[start..];
            let Some(end) = after.find("</entry>") else {
                break;
            };
            let body = &after[..end];
            let mut frequency = None;
            for pri in contents(body, "ke_pri")
                .into_iter()
                .chain(contents(body, "re_pri"))
            {
                let band = match pri.strip_prefix("nf").and_then(|n| n.parse::<u8>().ok()) {
                    Some(n) => n,
                    None if matches!(pri, "news1" | "ichi1" | "spec1" | "gai1") => 49,
                    None => continue,
                };
                frequency = Some(frequency.map_or(band, |f: u8| f.min(band)));
            }
            let entry = Entry {
                forms: contents(body, "keb")
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                readings: contents(body, "reb")
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                frequency,
            };
            let id = dict.entries.len();
            for key in entry.forms.iter().chain(&entry.readings) {
                dict.index.entry(key.clone()).or_default().push(id);
            }
            dict.entries.push(entry);
            rest = &after[end + "</entry>".len()..];
        }
        dict
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("JMDICT_UNREADABLE: {e}"))?;
        Ok(Self::parse(&text))
    }

    /// Entries that list `word` as a form or a reading.
    pub fn lookup(&self, word: &str) -> Vec<&Entry> {
        self.index
            .get(word)
            .into_iter()
            .flatten()
            .map(|&i| &self.entries[i])
            .collect()
    }

    pub fn contains(&self, word: &str) -> bool {
        self.index.contains_key(word)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_readings_and_the_best_frequency_band_are_read() {
        let xml = "<JMdict><entry><ent_seq>1</ent_seq><k_ele><keb>交通費</keb><ke_pri>news2</ke_pri><ke_pri>nf30</ke_pri></k_ele><r_ele><reb>こうつうひ</reb><re_pri>nf12</re_pri></r_ele><sense><pos>&n;</pos><gloss>travel expenses</gloss></sense></entry><entry><r_ele><reb>ゆっくり</reb><re_pri>ichi1</re_pri></r_ele></entry></JMdict>";
        let dict = Jmdict::parse(xml);
        assert_eq!(dict.len(), 2);
        let entry = dict.lookup("交通費")[0];
        assert_eq!(entry.readings, ["こうつうひ"]);
        assert_eq!(entry.frequency, Some(12));
        assert_eq!(dict.lookup("ゆっくり")[0].frequency, Some(49));
        assert!(!dict.contains("交通"));
    }
}
