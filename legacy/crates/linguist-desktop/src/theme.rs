use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

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
        let Some(path) = omarchy_palette_path() else {
            return fallback;
        };
        Self::load_from(&path)
    }
    pub fn load_from(path: &Path) -> Self {
        let fallback = Self::default();
        let Ok(contents) = fs::read_to_string(path) else {
            return fallback;
        };
        Self::from_contents(&contents)
    }
    pub fn from_contents(contents: &str) -> Self {
        let fallback = Self::default();
        let colors = parse_top_level_strings(contents);
        Self {
            background: color(&colors, "background", fallback.background),
            surface: color(&colors, "lighter_background", fallback.surface),
            foreground: color(&colors, "foreground", fallback.foreground),
            muted: color(&colors, "muted", fallback.muted),
            accent: color(&colors, "accent", fallback.accent),
        }
    }
}
pub fn omarchy_palette_path() -> Option<PathBuf> {
    omarchy_palette_path_from(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}
fn omarchy_palette_path_from(
    config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    config_home
        .or_else(|| home.map(|home| home.join(".config")))
        .map(|root| root.join("omarchy/current/theme/colors.toml"))
}
pub struct ThemeWatch {
    path: PathBuf,
    initialized: bool,
    contents: Option<Vec<u8>>,
}
impl ThemeWatch {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            initialized: false,
            contents: None,
        }
    }
    pub fn poll(&mut self) -> Option<ThemePalette> {
        let contents = fs::read(&self.path).ok();
        if self.initialized && contents == self.contents {
            return None;
        }
        self.initialized = true;
        self.contents = contents.clone();
        Some(
            contents
                .and_then(|contents| String::from_utf8(contents).ok())
                .map_or_else(ThemePalette::default, |contents| {
                    ThemePalette::from_contents(&contents)
                }),
        )
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
        .take_while(|line| !line.trim().starts_with('['))
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
        assert!(!values.contains_key("number"));
    }

    #[test]
    fn validates_qml_safe_hex_colors() {
        assert!(valid_hex_color("#89b4fa"));
        assert!(!valid_hex_color("red"));
        assert!(!valid_hex_color("#abcd"));
    }
    #[test]
    fn malformed_and_missing_palettes_fall_back() {
        let palette = ThemePalette::from_contents("background = \"red\"\naccent = \"#abcd\"");
        assert_eq!(palette.background, "#121212");
        assert_eq!(palette.accent, "#e68e0d");
        assert_eq!(
            ThemePalette::load_from(Path::new("/not/a/theme")).foreground,
            "#e8e8e8"
        );
    }

    #[test]
    fn resolves_documented_omarchy_config_path() {
        assert_eq!(
            omarchy_palette_path_from(Some("/config".into()), Some("/home/user".into())).unwrap(),
            PathBuf::from("/config/omarchy/current/theme/colors.toml")
        );
        assert_eq!(
            omarchy_palette_path_from(None, Some("/home/user".into())).unwrap(),
            PathBuf::from("/home/user/.config/omarchy/current/theme/colors.toml")
        );
    }

    #[test]
    fn watcher_detects_content_change_and_removal() {
        let directory =
            std::env::temp_dir().join(format!("linguist-theme-watch-{}", std::process::id()));
        let path = directory.join("colors.toml");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(&path, "accent = \"#112233\"\n").unwrap();
        let mut watch = ThemeWatch::new(path.clone());
        assert_eq!(watch.poll().unwrap().accent, "#112233");
        assert!(watch.poll().is_none());
        fs::write(&path, "accent = \"#abcdef\"\n").unwrap();
        assert_eq!(watch.poll().unwrap().accent, "#abcdef");
        fs::remove_file(&path).unwrap();
        assert_eq!(watch.poll().unwrap().accent, ThemePalette::default().accent);
        assert!(watch.poll().is_none());
        fs::remove_dir_all(directory).unwrap();
    }
}
