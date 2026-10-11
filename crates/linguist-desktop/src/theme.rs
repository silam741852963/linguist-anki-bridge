use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct ThemePalette {
    pub background: String,
    pub surface: String,
    pub foreground: String,
    pub muted: String,
    pub accent: String,
    pub selection: String,
    pub red: String,
    pub yellow: String,
    pub green: String,
    pub cyan: String,
    pub blue: String,
    pub magenta: String,
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
        let colors = parse_top_level_strings(contents);
        let background = color(&colors, "background", String::new());
        let foreground = color(&colors, "foreground", String::new());
        let surface = color(&colors, "lighter_background", background.clone());
        let muted = color(&colors, "muted", foreground.clone());
        let accent = color(&colors, "accent", foreground.clone());
        Self {
            background,
            surface,
            foreground: foreground.clone(),
            muted,
            selection: color(&colors, "selection", accent.clone()),
            red: color(&colors, "red", accent.clone()),
            yellow: color(&colors, "yellow", accent.clone()),
            green: color(&colors, "green", accent.clone()),
            cyan: color(&colors, "cyan", accent.clone()),
            blue: color(&colors, "blue", accent.clone()),
            magenta: color(&colors, "magenta", accent.clone()),
            accent,
        }
    }
}
pub fn omarchy_palette_path() -> Option<PathBuf> {
    omarchy_palette_path_from(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}
fn omarchy_palette_path_from(
    state_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    state_home
        .or_else(|| home.map(|home| home.join(".local/state")))
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
        assert!(palette.background.is_empty());
        assert!(palette.accent.is_empty());
        assert_eq!(
            ThemePalette::load_from(Path::new("/not/a/theme")).foreground,
            ""
        );
    }

    #[test]
    fn resolves_documented_omarchy_config_path() {
        assert_eq!(
            omarchy_palette_path_from(Some("/state".into()), Some("/home/user".into())).unwrap(),
            PathBuf::from("/state/omarchy/current/theme/colors.toml")
        );
        assert_eq!(
            omarchy_palette_path_from(None, Some("/home/user".into())).unwrap(),
            PathBuf::from("/home/user/.local/state/omarchy/current/theme/colors.toml")
        );
    }

    #[test]
    fn loads_semantic_omarchy_colors_for_native_surfaces() {
        let palette = ThemePalette::from_contents(
            "selection = \"#221122\"\nred = \"#aa0000\"\nyellow = \"#bbbb00\"\n\
             green = \"#00aa00\"\ncyan = \"#00bbbb\"\nblue = \"#0000aa\"\nmagenta = \"#aa00aa\"\n",
        );
        assert_eq!(palette.selection, "#221122");
        assert_eq!(palette.red, "#aa0000");
        assert_eq!(palette.yellow, "#bbbb00");
        assert_eq!(palette.green, "#00aa00");
        assert_eq!(palette.cyan, "#00bbbb");
        assert_eq!(palette.blue, "#0000aa");
        assert_eq!(palette.magenta, "#aa00aa");
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
        assert!(watch.poll().unwrap().accent.is_empty());
        assert!(watch.poll().is_none());
        fs::remove_dir_all(directory).unwrap();
    }
}
