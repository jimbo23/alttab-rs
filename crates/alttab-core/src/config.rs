use crate::Modifiers;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! choice {
    ($name:ident, $default:ident, $($other:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { #[default] $default, $($other),* }
    };
}
choice!(Style, Thumbnails, Icons, Titles);
choice!(SizePreset, Auto, Small, Medium, Large);
choice!(Theme, System, Light, Dark);
choice!(ScreenPlacement, Active, Mouse, Menubar);
choice!(TitleContent, WindowTitle, AppName, AppAndWindow);
choice!(Truncation, End, Start, Middle);
choice!(Visibility, Show, Hide, ShowAtEnd);
choice!(FullscreenPolicy, Show, Hide);
choice!(
    AppScope,
    All,
    Active,
    NonActive,
    WindowsUnderCursor,
    AppUnderCursor
);
choice!(SpaceScope, All, Visible, NonVisible);
choice!(ScreenScope, All, SwitcherScreen);
choice!(
    WindowOrder,
    RecentlyFocused,
    RecentlyCreated,
    Alphabetical,
    Space
);
choice!(ReleaseAction, Focus, Hold, Search);
choice!(HideRule, Never, Always, Windowless, TitleContains);
choice!(SuppressRule, Never, Always, Fullscreen);
choice!(Modifier, Option, Command, Control, Shift, CapsLock);
choice!(MenubarIcon, Outlined, Filled);

impl Modifier {
    pub fn mask(self) -> Modifiers {
        match self {
            Self::Option => Modifiers::OPTION,
            Self::Command => Modifiers::COMMAND,
            Self::Control => Modifiers::CONTROL,
            Self::Shift => Modifiers::SHIFT,
            Self::CapsLock => Modifiers::CAPS_LOCK,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub style: Style,
    pub size: SizePreset,
    pub theme: Theme,
    pub screen: ScreenPlacement,
    pub nominal_rows: Option<usize>,
    pub max_width_ratio: Option<f64>,
    pub max_height_ratio: f64,
    pub title_content: TitleContent,
    pub title_truncation: Truncation,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            style: Style::Thumbnails,
            size: SizePreset::Auto,
            theme: Theme::System,
            screen: ScreenPlacement::Active,
            nominal_rows: None,
            max_width_ratio: None,
            max_height_ratio: 0.8,
            title_content: TitleContent::WindowTitle,
            title_truncation: Truncation::End,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureConfig {
    pub cache_mib: u16,
    pub evict_on_hide: bool,
    pub selected_max_hz: u8,
    pub other_max_hz: u8,
}
impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            cache_mib: 32,
            evict_on_hide: true,
            selected_max_hz: 5,
            other_max_hz: 1,
        }
    }
}
impl CaptureConfig {
    pub fn budget_bytes(&self) -> usize {
        usize::from(self.cache_mib) * 1024 * 1024
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Shortcut {
    pub id: String,
    pub hold: Vec<Modifier>,
    pub next: String,
    pub previous: String,
    pub release_action: ReleaseAction,
    pub apps: AppScope,
    pub spaces: SpaceScope,
    pub screens: ScreenScope,
    pub minimized: Visibility,
    pub hidden: Visibility,
    pub fullscreen: FullscreenPolicy,
    pub windowless: Visibility,
    pub order: WindowOrder,
}
impl Default for Shortcut {
    fn default() -> Self {
        Self {
            id: "all_windows".into(),
            hold: vec![Modifier::Option],
            next: "tab".into(),
            previous: "shift".into(),
            release_action: ReleaseAction::Focus,
            apps: AppScope::All,
            spaces: SpaceScope::All,
            screens: ScreenScope::All,
            minimized: Visibility::Show,
            hidden: Visibility::Show,
            fullscreen: FullscreenPolicy::Show,
            windowless: Visibility::ShowAtEnd,
            order: WindowOrder::RecentlyFocused,
        }
    }
}
impl Shortcut {
    pub fn hold_mask(&self) -> Modifiers {
        self.hold
            .iter()
            .fold(Modifiers::default(), |mask, item| mask | item.mask())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BlacklistRule {
    pub bundle_id_prefix: String,
    pub hide: HideRule,
    pub title_patterns: Vec<String>,
    pub suppress_shortcuts: SuppressRule,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub launch_at_login: bool,
    pub menubar_icon: MenubarIcon,
    pub menubar_visible: bool,
    pub appearance: Appearance,
    pub capture: CaptureConfig,
    pub shortcuts: Vec<Shortcut>,
    pub blacklist: Vec<BlacklistRule>,
}
impl Default for Config {
    fn default() -> Self {
        let active = Shortcut {
            id: "active_app".into(),
            next: "key_above_tab".into(),
            apps: AppScope::Active,
            ..Shortcut::default()
        };
        Self {
            schema_version: 1,
            launch_at_login: false,
            menubar_icon: MenubarIcon::Outlined,
            menubar_visible: true,
            appearance: Appearance::default(),
            capture: CaptureConfig::default(),
            shortcuts: vec![Shortcut::default(), active],
            blacklist: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
    Validation(String),
}
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "configuration I/O: {e}"),
            Self::Parse(e) => write!(f, "invalid TOML: {e}"),
            Self::Serialize(e) => write!(f, "configuration serialization: {e}"),
            Self::Validation(e) => f.write_str(e),
        }
    }
}
impl std::error::Error for ConfigError {}
impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |message: &str| Err(ConfigError::Validation(message.into()));
        if self.schema_version != 1 {
            return invalid("unsupported schema_version (expected 1)");
        }
        if !(1..=9).contains(&self.shortcuts.len()) {
            return invalid("shortcuts must contain 1–9 profiles");
        }
        if self.capture.cache_mib > 128 {
            return invalid("capture.cache_mib must be 0–128");
        }
        if !(1..=5).contains(&self.capture.selected_max_hz) || self.capture.other_max_hz != 1 {
            return invalid("capture cadence must be selected 1–5 Hz and other 1 Hz");
        }
        let ratio = |v: f64| v.is_finite() && v > 0.0 && v <= 1.0;
        if !ratio(self.appearance.max_height_ratio)
            || self.appearance.max_width_ratio.is_some_and(|v| !ratio(v))
        {
            return invalid("appearance ratios must be finite and in (0, 1]");
        }
        if self
            .appearance
            .nominal_rows
            .is_some_and(|n| n == 0 || n > 100)
        {
            return invalid("nominal_rows must be 1–100");
        }
        for (index, shortcut) in self.shortcuts.iter().enumerate() {
            if shortcut.id.trim().is_empty()
                || !matches!(shortcut.next.as_str(), "tab" | "key_above_tab")
                || !matches!(shortcut.previous.as_str(), "shift" | "shift+tab")
            {
                return invalid("shortcut requires a nonempty id, tab/key_above_tab next and shift/shift+tab previous");
            }
            let bits = shortcut.hold_mask().0.count_ones() as usize;
            if bits != shortcut.hold.len() {
                return invalid("duplicate shortcut hold modifier");
            }
            for other in &self.shortcuts[..index] {
                if other.id == shortcut.id {
                    return invalid("duplicate shortcut id");
                }
                if other.hold_mask() == shortcut.hold_mask() && other.next == shortcut.next {
                    return invalid("conflicting shortcut chords");
                }
            }
        }
        if self
            .blacklist
            .iter()
            .any(|rule| rule.bundle_id_prefix.is_empty())
        {
            return invalid("blacklist bundle_id_prefix cannot be empty");
        }
        Ok(())
    }
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text).map_err(ConfigError::Parse)?;
        config.validate()?;
        Ok(config)
    }
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        self.validate()?;
        toml::to_string_pretty(self).map_err(ConfigError::Serialize)
    }
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let text = self.to_toml()?;
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let file_name = path.file_name().ok_or_else(|| {
            ConfigError::Validation("configuration path needs a file name".into())
        })?;
        let (temporary, mut file) = loop {
            let mut name = file_name.to_os_string();
            name.push(format!(
                ".{}.{}.tmp",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            let temporary = parent.join(name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => break (temporary, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        };
        let result = (|| -> Result<(), ConfigError> {
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)?;
            #[cfg(unix)]
            fs::File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

pub fn application_support_path(home: &Path) -> PathBuf {
    home.join("Library/Application Support/alttab-rs/config.toml")
}
