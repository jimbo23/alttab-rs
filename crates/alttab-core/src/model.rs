use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppKey {
    pub pid: i32,
    pub incarnation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowKey {
    pub app: AppKey,
    pub cg_window_id: u32,
    pub incarnation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemKey {
    Window(WindowKey),
    App(AppKey),
}

impl ItemKey {
    pub fn app(self) -> AppKey {
        match self {
            Self::Window(key) => key.app,
            Self::App(key) => key,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpaceId(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn center_x(self) -> f64 {
        self.x + self.width / 2.0
    }
    pub fn center_y(self) -> f64 {
        self.y + self.height / 2.0
    }
    pub fn intersects(self, other: Self) -> bool {
        self.width > 0.0
            && self.height > 0.0
            && other.width > 0.0
            && other.height > 0.0
            && self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }
    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub key: AppKey,
    pub bundle_id: String,
    pub name: String,
    pub hidden: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Acquisition {
    #[default]
    Unknown,
    Available,
    Absent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub key: WindowKey,
    pub title: String,
    pub bounds: Option<Rect>,
    pub displays: Option<Vec<DisplayId>>,
    pub spaces: Option<Vec<SpaceId>>,
    pub minimized: bool,
    pub fullscreen: bool,
    pub eligible: bool,
    pub acquisition: Acquisition,
    pub created: u64,
}

impl Window {
    pub fn new(key: WindowKey, title: impl Into<String>) -> Self {
        Self {
            key,
            title: title.into(),
            bounds: None,
            displays: None,
            spaces: None,
            minimized: false,
            fullscreen: false,
            eligible: true,
            acquisition: Acquisition::Unknown,
            created: 0,
        }
    }
}

/// Surface ancestry and semantic destination membership deliberately remain separate.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub key: WindowKey,
    pub parent: Option<WindowKey>,
    pub bounds: Option<Rect>,
    pub visible: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct Inventory {
    apps: BTreeMap<AppKey, App>,
    windows: BTreeMap<WindowKey, Window>,
    app_generations: BTreeMap<i32, u64>,
    window_generations: BTreeMap<(AppKey, u32), u64>,
    dead_apps: BTreeSet<AppKey>,
    dead_windows: BTreeSet<WindowKey>,
    surfaces: BTreeMap<WindowKey, Surface>,
}

impl Inventory {
    pub fn upsert_app(&mut self, app: App) -> bool {
        if app.key.pid <= 0
            || self.dead_apps.contains(&app.key)
            || self
                .app_generations
                .get(&app.key.pid)
                .is_some_and(|g| *g > app.key.incarnation)
        {
            return false;
        }
        let old: Vec<_> = self
            .apps
            .keys()
            .copied()
            .filter(|k| k.pid == app.key.pid && *k != app.key)
            .collect();
        for key in old {
            self.remove_app(key);
        }
        self.app_generations
            .insert(app.key.pid, app.key.incarnation);
        self.apps.insert(app.key, app);
        true
    }
    pub fn upsert_window(&mut self, window: Window) -> bool {
        let key = window.key;
        if key.cg_window_id == 0
            || !self.apps.contains_key(&key.app)
            || self.dead_windows.contains(&key)
            || self
                .window_generations
                .get(&(key.app, key.cg_window_id))
                .is_some_and(|g| *g > key.incarnation)
        {
            return false;
        }
        self.windows
            .retain(|k, _| k.app != key.app || k.cg_window_id != key.cg_window_id || *k == key);
        self.window_generations
            .insert((key.app, key.cg_window_id), key.incarnation);
        self.windows.insert(key, window);
        true
    }
    pub fn remove_app(&mut self, key: AppKey) -> bool {
        if !self.apps.contains_key(&key) {
            return false;
        }
        self.apps.remove(&key);
        self.dead_apps.insert(key);
        self.windows.retain(|k, _| k.app != key);
        self.surfaces.retain(|k, _| k.app != key);
        true
    }
    pub fn confirm_destroyed(&mut self, key: WindowKey) -> bool {
        if !self.windows.contains_key(&key) && !self.surfaces.contains_key(&key) {
            return false;
        }
        self.dead_windows.insert(key);
        self.windows.remove(&key);
        self.surfaces.remove(&key);
        true
    }
    pub fn upsert_surface(&mut self, surface: Surface) -> bool {
        let key = surface.key;
        if key.cg_window_id == 0
            || !self.apps.contains_key(&key.app)
            || self.dead_windows.contains(&key)
            || self
                .window_generations
                .get(&(key.app, key.cg_window_id))
                .is_some_and(|g| *g > key.incarnation)
        {
            return false;
        }
        self.surfaces.insert(key, surface);
        true
    }
    pub fn surface_root(&self, key: WindowKey) -> Option<WindowKey> {
        let mut seen = BTreeSet::new();
        let mut current = key;
        while seen.insert(current) {
            let surface = self.surfaces.get(&current)?;
            match surface.parent {
                Some(parent) => current = parent,
                None => return Some(current),
            }
        }
        None
    }
    pub fn window(&self, key: WindowKey) -> Option<&Window> {
        self.windows.get(&key)
    }
    pub fn app(&self, key: AppKey) -> Option<&App> {
        self.apps.get(&key)
    }
    pub fn windows(&self) -> impl Iterator<Item = &Window> {
        self.windows.values()
    }
    pub fn apps(&self) -> impl Iterator<Item = &App> {
        self.apps.values()
    }
}
