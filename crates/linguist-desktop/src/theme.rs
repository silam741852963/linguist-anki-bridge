use std::{collections::BTreeMap, fs, path::PathBuf};

pub struct ThemePalette {
    pub background: String,
    pub surface: String,
    pub foreground: String,
    pub muted: String,
    pub accent: String,
}

impl ThemePalette {
    pub fn load() -> Self {
        let fallback = Self::default();
        let Some(home) = std::env::var_os("HOME") else {
            return fallback;
        };
        let path = PathBuf::from(home).join(".config/omarchy/current/theme/colors.toml");
        let Ok(contents) = fs::read_to_string(path) else {
            return fallback;
        };
        let colors = parse_top_level_strings(&contents);
        Self {
            background: color(&colors, "background", fallback.background),
            surface: color(&colors, "lighter_background", fallback.surface),
            foreground: color(&colors, "foreground", fallback.foreground),
            muted: color(&colors, "muted", fallback.muted),
            accent: color(&colors, "accent", fallback.accent),
        }
    }
}

impl Default for ThemePalette {
    fn default() -> Self {
        Self {
            background: "#121212".into(),
            surface: "#1c1c1c".into(),
            foreground: "#e8e8e8".into(),
            muted: "#737373".into(),
            accent: "#e68e0d".into(),
        }
    }
}

fn color(values: &BTreeMap<String, String>, key: &str, fallback: String) -> String {
    values
        .get(key)
        .filter(|value| valid_hex_color(value))
        .cloned()
        .unwrap_or(fallback)
}

fn valid_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn parse_top_level_strings(contents: &str) -> BTreeMap<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                return None;
            }
            let (key, raw) = line.split_once('=')?;
            let value = raw.trim().trim_matches('"');
            (!key.trim().is_empty() && !value.is_empty())
                .then(|| (key.trim().to_owned(), value.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_simple_top_level_strings() {
        let values = parse_top_level_strings(
            "mode = \"dark\"\naccent = \"#89b4fa\"\n[ignored]\nnumber = 1\n",
        );
        assert_eq!(values["accent"], "#89b4fa");
        assert_eq!(values["mode"], "dark");
        assert_eq!(values["number"], "1");
    }

    #[test]
    fn validates_qml_safe_hex_colors() {
        assert!(valid_hex_color("#89b4fa"));
        assert!(!valid_hex_color("red"));
        assert!(!valid_hex_color("#abcd"));
    }
}
