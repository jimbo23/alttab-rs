use crate::{
    capture::{self, Image, Request},
    ffi, input, tracking,
    ui::Ui,
};
use alttab_core::*;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::{class, msg_send, rc::autoreleasepool, runtime::AnyObject};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{MainThreadMarker, NSString};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub enum Event {
    Repeat {
        generation: u64,
        token: u64,
    },
    Discovered(u64),
    Key {
        key: u16,
        flags: u64,
        kind: u32,
        repeat: bool,
        serial: u64,
    },
    Inventory {
        windows: Vec<tracking::Window>,
        applications: Vec<tracking::Application>,
        uncertain: HashSet<i32>,
        front_pid: i32,
        visible_spaces: Vec<u64>,
    },
    Captured {
        request: Request,
        image: Option<Image>,
    },
    Icon {
        pid: i32,
        generation: u64,
        image: Image,
    },
    Search(String),
    Cleanup(u64),
    SearchKey {
        key: u16,
        repeat: bool,
    },
    Focus {
        key: ItemKey,
        generation: u64,
    },
    Viewport,
    Recapture {
        key: ItemKey,
        generation: u64,
    },
    Show(u64),
    Refresh,
    Workspace,
    Preferences,
    Permissions,
    Setting(isize),
    Click(usize),
    Pointer {
        x: f64,
        y_from_top: f64,
        screen_x: f64,
        screen_y: f64,
    },
    PointerExited,
    DismissError,
    TapReset,
    Error(String),
    Quit,
}
thread_local! { static APP: RefCell<Option<Coordinator>> = const { RefCell::new(None) }; }
pub fn post(event: Event) {
    static REFRESH_PENDING: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    let refresh = matches!(event, Event::Refresh);
    if refresh && REFRESH_PENDING.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    DispatchQueue::main().exec_async(move || {
        autoreleasepool(|_| {
            if refresh {
                REFRESH_PENDING.store(false, std::sync::atomic::Ordering::Release);
            }
            APP.with(|cell| {
                if let Some(app) = cell.borrow_mut().as_mut() {
                    app.event(event);
                }
            });
        })
    });
}
fn later(milliseconds: u64, event: Event) {
    if let Ok(time) = DispatchTime::try_from(Duration::from_millis(milliseconds)) {
        let _ = DispatchQueue::main().after(time, move || post(event));
    }
}
pub fn run() {
    let application = autoreleasepool(|_| {
        let mtm = MainThreadMarker::new().expect("AppKit requires the main thread");
        let application = NSApplication::sharedApplication(mtm);
        application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let path = application_support_path(&home);
        let loaded = Config::load(&path).and_then(|config| {
            if config.capture.evict_on_hide {
                Ok(config)
            } else {
                Err(ConfigError::Validation(
                    "The native adapter requires capture.evict_on_hide = true.".into(),
                ))
            }
        });
        let error = loaded.as_ref().err().map(ToString::to_string);
        let config = loaded.unwrap_or_default();
        let mut app = Coordinator {
            cache: ThumbnailCache::new(config.capture.budget_bytes()),
            config,
            path,
            ui: Ui::new(mtm),
            tracker: tracking::Tracker::start(),
            inventory: Inventory::default(),
            mru: Mru::default(),
            session: Session::default(),
            pointer_gate: PointerGate::new(HOVER_DEAD_ZONE),
            app_keys: HashMap::new(),
            window_keys: HashMap::new(),
            serial: 0,
            cursor_window: None,
            zorder: Vec::new(),
            seeded: false,
            active_app: None,
            attended: None,
            visible_spaces: Vec::new(),
            images: HashMap::new(),
            captures: VecDeque::new(),
            capture_busy: false,
            refresh_scheduled: HashSet::new(),
            repeat_token: 0,
            repeat_timing: repeat_timing(),
            shift_advanced: false,
            pending_summon: None,
            discovery_serial: 0,
            discovery_complete: false,
            capture_attempts: HashMap::new(),
            layout: Layout::default(),
            entries: Vec::new(),
            sizes: Vec::new(),
            render_dirty: true,
            query: String::new(),
            search_started: false,
            started: false,
            tap_started: false,
            skipped: recording_skipped(None),
            trusted: false,
            recording: false,
            flags: Modifiers::default(),
            clock: Instant::now(),
            frozen_screen: None,
        };
        app.apply_config();
        app.permissions(false);
        if app.config.launch_at_login {
            if let Err(error) = login(true) {
                post(Event::Error(error));
            }
        }
        APP.with(|cell| *cell.borrow_mut() = Some(app));
        if let Some(error) = error {
            post(Event::Error(format!(
                "Your configuration was preserved but could not be loaded: {error}"
            )));
        }
        application
    });
    application.run();
}
struct Coordinator {
    config: Config,
    path: PathBuf,
    ui: Ui,
    tracker: tracking::Tracker,
    inventory: Inventory,
    mru: Mru,
    session: Session,
    pointer_gate: PointerGate,
    app_keys: HashMap<i32, AppKey>,
    window_keys: HashMap<(AppKey, u32), WindowKey>,
    serial: u64,
    active_app: Option<AppKey>,
    attended: Option<ItemKey>,
    visible_spaces: Vec<SpaceId>,
    cursor_window: Option<WindowKey>,
    zorder: Vec<WindowKey>,
    seeded: bool,
    cache: ThumbnailCache,
    images: HashMap<ItemKey, Image>,
    captures: VecDeque<ItemKey>,
    capture_busy: bool,
    capture_attempts: HashMap<ItemKey, u64>,
    layout: Layout,
    entries: Vec<(ItemKey, i32, String)>,
    sizes: Vec<Size>,
    render_dirty: bool,
    refresh_scheduled: HashSet<ItemKey>,
    repeat_token: u64,
    repeat_timing: (u64, u64),
    shift_advanced: bool,
    pending_summon: Option<(u64, usize, bool)>,
    discovery_serial: u64,
    discovery_complete: bool,
    query: String,
    search_started: bool,
    started: bool,
    tap_started: bool,
    skipped: bool,
    trusted: bool,
    recording: bool,
    flags: Modifiers,
    clock: Instant,
    frozen_screen: Option<crate::ui::Screen>,
}
impl Coordinator {
    fn now(&self) -> u64 {
        self.clock.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }
    fn event(&mut self, event: Event) {
        match event {
            Event::Key {
                key,
                flags,
                kind,
                repeat,
                serial,
            } => self.key(key, flags, kind, repeat, serial),
            Event::Inventory {
                windows,
                applications,
                uncertain,
                front_pid,
                visible_spaces,
            } => self.inventory(windows, applications, uncertain, front_pid, visible_spaces),
            Event::Captured { request, image } => self.captured(request, image),
            Event::Icon {
                pid,
                generation,
                image,
            } => {
                if self
                    .app_keys
                    .get(&pid)
                    .is_some_and(|key| key.incarnation == generation)
                {
                    self.ui.icon(pid, &image);
                    self.render_dirty = true;
                    if self.session.phase() == Phase::Visible {
                        self.render();
                    }
                }
            }
            Event::Show(generation) => {
                let showing = self.session.phase() == Phase::PendingShow
                    && self.session.generation() == generation;
                let reveal = showing && self.session.searching();
                if showing {
                    self.arm_pointer();
                }
                let effects = self.session.handle(Input::Show { generation });
                self.effects(effects);
                if reveal {
                    self.ui.start_search();
                }
            }
            Event::Search(query) => {
                if self.session.searching() {
                    self.query = query;
                    self.render_dirty = true;
                    let items = self.candidates(self.session.profile());
                    let effects = self.session.reconcile(items);
                    self.effects(effects);
                }
            }
            Event::SearchKey { key, repeat } => {
                if self.session.searching() {
                    self.key(key, input::event_flags(self.flags), 10, repeat, 0);
                }
            }
            Event::Cleanup(generation) => {
                if self.session.phase() == Phase::Idle && self.session.generation() == generation {
                    self.ui.end_search();
                }
            }
            Event::Focus { key, generation } => {
                if self.session.phase() == Phase::Idle
                    && self.session.generation() == generation
                    && self.inventory.app(key.app()).is_some()
                {
                    self.tracker.action(
                        key.app(),
                        match key {
                            ItemKey::Window(key) => key.cg_window_id,
                            _ => 0,
                        },
                        tracking::Action::Focus,
                    );
                }
            }
            Event::Refresh => self.tracker.refresh(),
            Event::Viewport => self.capture_next(),
            Event::Repeat { generation, token } => {
                if token == self.repeat_token
                    && generation == self.session.generation()
                    && self.session.phase() != Phase::Idle
                    && self.flags.contains(Modifiers::SHIFT)
                    && !self.session.searching()
                {
                    let selected = self.session.selected();
                    let effects = self.session.handle(Input::Cycle {
                        backward: true,
                        repeat: true,
                    });
                    self.effects(effects);
                    if selected != self.session.selected() {
                        later(self.repeat_timing.1, Event::Repeat { generation, token });
                    }
                }
            }
            Event::Discovered(serial) => {
                if let Some((pending, profile, backward)) = self.pending_summon {
                    if pending == serial {
                        self.pending_summon = None;
                        self.discovery_complete = true;
                        if self.trusted {
                            self.summon(profile, backward);
                            let effects = self.session.handle(Input::Modifiers(self.flags));
                            self.effects(effects);
                        }
                    }
                }
            }
            Event::Recapture { key, generation } => {
                if self.cache.generation() == generation && self.session.phase() == Phase::Visible {
                    self.refresh_scheduled.remove(&key);
                    self.capture_attempts.remove(&key);
                    self.evict_image(key);
                    self.capture_next();
                }
            }
            Event::Workspace => {
                self.permissions(true);
                if self.started {
                    self.tracker.refresh();
                }
            }
            Event::Preferences => {
                let effects = self.session.handle(Input::Cancel);
                self.effects(effects);
                self.ui.preferences(&self.config);
                self.sync_settings();
            }
            Event::DismissError => self.ui.dismiss_error(),
            Event::Permissions => self
                .ui
                .permissions(self.trusted, self.recording, self.started),
            Event::Setting(tag) => self.setting(tag),
            Event::Click(index) => {
                if let Some(&(key, _, _)) = self.entries.get(index) {
                    let effects = self.session.handle(Input::Pick(key));
                    self.effects(effects);
                }
            }
            Event::Pointer {
                x,
                y_from_top,
                screen_x,
                screen_y,
            } => {
                if self.session.phase() == Phase::Visible
                    && self.pointer_gate.moved(screen_x, screen_y)
                {
                    let key = self
                        .layout
                        .hit_test(x, y_from_top)
                        .and_then(|index| self.entries.get(index))
                        .map(|entry| entry.0);
                    let effects = self.session.handle(Input::Hover(key));
                    self.effects(effects);
                }
            }
            Event::PointerExited => {
                if self.session.phase() == Phase::Visible {
                    let effects = self.session.handle(Input::Hover(None));
                    self.effects(effects);
                }
            }
            Event::TapReset => {
                let effects = self.session.handle(Input::Cancel);
                self.effects(effects);
            }
            Event::Error(error) => self.ui.error(&error),
            Event::Quit => unsafe {
                input::active(false);
                self.ui.hide();
                self.images.clear();
                let application: *mut AnyObject =
                    msg_send![class!(NSApplication), sharedApplication];
                let _: () = msg_send![application,terminate:std::ptr::null::<AnyObject>()];
            },
        }
    }
    fn key(&mut self, key: u16, flags: u64, kind: u32, repeat: bool, serial: u64) {
        if !self.started || !self.trusted {
            return;
        }
        let modifiers = modifiers(flags);
        let old = self.flags;
        self.flags = modifiers;
        if kind == 12 {
            if !modifiers.contains(Modifiers::SHIFT) {
                self.repeat_token = self.repeat_token.wrapping_add(1);
                self.shift_advanced = false;
            }
            let effects = self.session.handle(Input::Physical {
                serial,
                action: PhysicalAction::Modifiers(modifiers),
            });
            self.effects(effects);
            if modifiers.contains(Modifiers::SHIFT)
                && !old.contains(Modifiers::SHIFT)
                && self.session.phase() != Phase::Idle
                && self.config.shortcuts[self.session.profile()].previous == "shift"
                && !self.session.searching()
            {
                let effects = self.session.handle(Input::Physical {
                    serial: serial.wrapping_add(1 << 63),
                    action: PhysicalAction::ShiftPressed,
                });
                self.effects(effects);
                self.shift_advanced = true;
                self.start_repeat();
            }
            return;
        }
        if self.pending_summon.is_some() && key == 53 {
            self.pending_summon = None;
            input::active(false);
            return;
        }
        if key == 48 && modifiers.contains(Modifiers::SHIFT) && self.shift_advanced {
            self.shift_advanced = false;
            return;
        }
        if key != 48 {
            self.repeat_token = self.repeat_token.wrapping_add(1);
            self.shift_advanced = false;
        }
        if self.session.searching() && key == 48 {
            return;
        }
        if let Some(profile) = self.config.shortcuts.iter().position(|profile| {
            modifiers.contains(profile.hold_mask())
                && key == if profile.next == "tab" { 48 } else { 50 }
        }) {
            if self.session.phase() == Phase::Idle || self.session.profile() != profile {
                self.summon(profile, modifiers.contains(Modifiers::SHIFT));
            } else {
                let effects = self.session.handle(Input::Physical {
                    serial,
                    action: PhysicalAction::Tab {
                        shift: modifiers.contains(Modifiers::SHIFT),
                        repeat,
                    },
                });
                self.effects(effects);
            }
            return;
        }
        if self.session.phase() == Phase::Idle {
            return;
        }
        if key == 1 && !self.session.searching() {
            self.begin_search();
            return;
        }
        if key == 53 && self.session.searching() && !self.search_started {
            self.session.handle(Input::ExitSearch);
            self.query.clear();
            input::search(false);
            self.ui.end_search();
            self.render_dirty = true;
            let items = self.candidates(self.session.profile());
            let effects = self.session.reconcile(items);
            self.effects(effects);
            return;
        }
        if let Some(action) = match key {
            13 => Some(tracking::Action::Close),
            46 => Some(tracking::Action::Minimize),
            3 => Some(tracking::Action::Fullscreen),
            12 => Some(tracking::Action::Quit),
            4 => Some(tracking::Action::Hide),
            _ => None,
        } {
            if let Some(key) = self.session.selected() {
                self.tracker.action(
                    key.app(),
                    match key {
                        ItemKey::Window(key) => key.cg_window_id,
                        _ => 0,
                    },
                    action,
                );
            }
            return;
        }
        let input = match key {
            48 | 50 => Input::Cycle {
                backward: modifiers.contains(Modifiers::SHIFT),
                repeat,
            },
            36 | 76 => Input::Accept,
            53 => Input::Cancel,
            123 => Input::Arrow {
                direction: Direction::Left,
                repeat,
            },
            124 => Input::Arrow {
                direction: Direction::Right,
                repeat,
            },
            125 => Input::Arrow {
                direction: Direction::Down,
                repeat,
            },
            126 => Input::Arrow {
                direction: Direction::Up,
                repeat,
            },
            _ => return,
        };
        let effects = self.session.handle(input);
        self.effects(effects);
    }
    fn summon(&mut self, profile: usize, backward: bool) {
        self.tracker.cancel_focus();
        if !self.discovery_complete && self.inventory.apps().next().is_none() {
            if let Some((serial, _, _)) = self.pending_summon {
                self.pending_summon = Some((serial, profile, backward));
            } else {
                self.discovery_serial = self.discovery_serial.wrapping_add(1);
                self.pending_summon = Some((self.discovery_serial, profile, backward));
                later(400, Event::Discovered(self.discovery_serial));
            }
            input::active(true);
            self.tracker.refresh();
            return;
        }
        self.repeat_token = self.repeat_token.wrapping_add(1);
        self.shift_advanced = false;
        if let Some(app) = self.active_app.and_then(|key| self.inventory.app(key)) {
            let fullscreen = self
                .attended
                .and_then(|key| match key {
                    ItemKey::Window(key) => self.inventory.window(key),
                    _ => None,
                })
                .is_some_and(|w| w.fullscreen);
            if self
                .config
                .blacklist
                .iter()
                .any(|rule| rule.suppresses(app, fullscreen))
            {
                return;
            }
        }
        let pointer = self.ui.pointer_quartz();
        self.cursor_window = self.zorder.iter().copied().find(|key| {
            self.inventory.window(*key).is_some_and(|window| {
                !window.minimized
                    && window
                        .bounds
                        .is_some_and(|r| r.contains(pointer.0, pointer.1))
            }) && self.inventory.app(key.app).is_some_and(|app| !app.hidden)
        });
        let attended = self.attended.and_then(|key| match key {
            ItemKey::Window(key) => self.inventory.window(key).and_then(|w| w.bounds),
            _ => None,
        });
        if self.frozen_screen.is_none() {
            self.frozen_screen = Some(self.ui.screen(self.config.appearance.screen, attended));
        }
        self.query.clear();
        let items = self.candidates(profile);
        self.arm_pointer();
        let profile_config = &self.config.shortcuts[profile];
        self.cache.begin_session();
        self.capture_attempts.clear();
        self.render_dirty = true;
        let effects = self.session.summon(
            profile,
            profile_config.hold_mask(),
            profile_config.release_action,
            items,
            self.attended,
            self.now(),
            100,
        );
        input::active(self.session.phase() != Phase::Idle);
        self.search_started =
            self.config.shortcuts[profile].release_action == ReleaseAction::Search;
        input::search(self.search_started);
        input::active_profile(profile);
        self.effects(effects);
        if self.search_started && self.session.phase() == Phase::Visible {
            self.ui.start_search();
        }
        if backward && self.session.phase() != Phase::Idle {
            let steps = if self
                .attended
                .is_some_and(|a| self.session.items().contains(&a))
                && self.session.items().len() > 1
            {
                2
            } else {
                1
            };
            for _ in 0..steps {
                let effects = self.session.handle(Input::Cycle {
                    backward: true,
                    repeat: false,
                });
                self.effects(effects);
            }
            if self.config.shortcuts[profile].previous == "shift" {
                self.start_repeat();
            }
        }
        self.tracker.refresh();
    }
    fn suppressed(&self) -> bool {
        self.active_app
            .and_then(|key| self.inventory.app(key))
            .is_some_and(|app| {
                let fullscreen = self
                    .attended
                    .and_then(|key| match key {
                        ItemKey::Window(key) => self.inventory.window(key),
                        _ => None,
                    })
                    .is_some_and(|window| window.fullscreen);
                self.config
                    .blacklist
                    .iter()
                    .any(|rule| rule.suppresses(app, fullscreen))
            })
    }
    fn candidates(&self, profile: usize) -> Vec<ItemKey> {
        let mut items = candidates(
            &self.inventory,
            &self.mru,
            &self.config.shortcuts[profile],
            &self.config.blacklist,
            &FilterContext {
                active_app: self.active_app,
                visible_spaces: self.visible_spaces.clone(),
                switcher_display: self.frozen_screen.map(|s| s.id),
                cursor_window: self.cursor_window,
                cursor_app: self.cursor_window.map(|key| key.app),
            },
        );
        if !self.query.is_empty() {
            let query = self.query.to_lowercase();
            let mut ranked = Vec::with_capacity(items.len());
            for key in &items {
                let app = self
                    .inventory
                    .app(key.app())
                    .and_then(|app| app.name.to_lowercase().find(&query));
                let title = match key {
                    ItemKey::Window(key) => self
                        .inventory
                        .window(*key)
                        .and_then(|window| window.title.to_lowercase().find(&query)),
                    _ => None,
                };
                if let Some(score) = app.into_iter().chain(title).min() {
                    ranked.push((*key, score));
                }
            }
            ranked.sort_unstable_by(|(a, score_a), (b, score_b)| {
                score_a.cmp(score_b).then_with(|| self.mru.compare(*a, *b))
            });
            items.clear();
            items.extend(ranked.into_iter().map(|(key, _)| key));
        }
        items
    }
    fn effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Render => self.render(),
                Effect::ScheduleShow {
                    generation,
                    deadline_ms,
                } => later(
                    deadline_ms.saturating_sub(self.now()),
                    Event::Show(generation),
                ),
                Effect::Hide { .. } => {
                    self.pointer_gate.reset();
                    input::active(false);
                    input::search(false);
                    self.ui.hide();
                    self.cache.hide();
                    self.images.clear();
                    self.captures.clear();
                    self.capture_attempts.clear();
                    self.refresh_scheduled.clear();
                    self.frozen_screen = None;
                    self.query.clear();
                    later(0, Event::Cleanup(self.session.generation()));
                }
                Effect::Focus(key) => self.focus(key),
            }
        }
    }
    /// Anchor the hover dead zone where the pointer rests when the panel
    /// appears, so a cursor already over a tile cannot steal the selection
    /// until it deliberately moves (upstream CursorEvents.swift:157-187).
    fn arm_pointer(&mut self) {
        self.pointer_gate.reset();
        let (x, y) = self.ui.pointer_cocoa();
        self.pointer_gate.moved(x, y);
    }
    fn focus(&self, key: ItemKey) {
        post(Event::Focus {
            key,
            generation: self.session.generation(),
        });
    }
    fn remove_app(&mut self, key: AppKey) {
        self.ui.remove_icon(key.pid);
        let windows: Vec<_> = self
            .window_keys
            .values()
            .copied()
            .filter(|window| window.app == key)
            .collect();
        for window in windows {
            self.mru.remove(ItemKey::Window(window));
            self.cache.remove(ItemKey::Window(window));
            self.evict_image(ItemKey::Window(window));
        }
        self.inventory.remove_app(key);
        self.app_keys.remove(&key.pid);
        self.window_keys.retain(|(owner, _), _| *owner != key);
        self.mru.remove(ItemKey::App(key));
        self.capture_attempts.retain(|item, _| item.app() != key);
        self.refresh_scheduled.retain(|item| item.app() != key);
        if self.attended.is_some_and(|item| item.app() == key) {
            self.attended = None;
        }
    }
    fn inventory(
        &mut self,
        windows: Vec<tracking::Window>,
        applications: Vec<tracking::Application>,
        uncertain: HashSet<i32>,
        front_pid: i32,
        visible_spaces: Vec<u64>,
    ) {
        let mut living = HashSet::new();
        let screens = self.ui.screens();
        if let Some(frozen) = self.frozen_screen {
            self.frozen_screen = screens
                .iter()
                .find(|s| s.id == frozen.id)
                .copied()
                .or_else(|| screens.first().copied());
        }
        let cursor = self.ui.pointer_quartz();
        self.cursor_window = None;
        for app in applications {
            let key = AppKey {
                pid: app.pid,
                incarnation: app.generation,
            };
            if self.app_keys.get(&app.pid).is_some_and(|old| *old != key) {
                self.remove_app(self.app_keys[&app.pid]);
            }
            self.app_keys.insert(app.pid, key);
            living.insert(key);
            self.inventory.upsert_app(App {
                key,
                bundle_id: app.bundle,
                name: app.name,
                hidden: app.hidden,
            });
        }
        let dead: Vec<_> = self
            .app_keys
            .values()
            .copied()
            .filter(|key| !living.contains(key))
            .collect();
        for key in dead {
            self.remove_app(key);
        }
        self.ui.retain_icons(&self.app_keys);
        let mut seen = HashSet::new();
        let mut seed = Vec::new();
        let mut focused = None;
        self.zorder.clear();
        for native in windows {
            let Some(app) = self.app_keys.get(&native.pid).copied() else {
                continue;
            };
            let key = *self.window_keys.entry((app, native.id)).or_insert_with(|| {
                self.serial = self.serial.wrapping_add(1);
                WindowKey {
                    app,
                    cg_window_id: native.id,
                    incarnation: self.serial,
                }
            });
            seen.insert(key);
            seed.push(ItemKey::Window(key));
            if native.on_screen {
                self.zorder.push(key);
            }
            let mut window = Window::new(key, native.title);
            window.bounds = Some(Rect {
                x: native.bounds[0],
                y: native.bounds[1],
                width: native.bounds[2],
                height: native.bounds[3],
            });
            window.minimized = native.minimized;
            window.fullscreen = native.fullscreen;
            window.acquisition = if uncertain.contains(&native.pid) {
                Acquisition::Unknown
            } else {
                Acquisition::Available
            };
            window.spaces = native
                .spaces
                .map(|spaces| spaces.into_iter().map(SpaceId).collect());
            window.displays = window.bounds.map(|bounds| {
                screens
                    .iter()
                    .filter(|screen| screen.quartz.intersects(bounds))
                    .map(|s| s.id)
                    .collect()
            });
            window.eligible = native.eligible;
            if uncertain.contains(&native.pid) {
                if let Some(previous) = self.inventory.window(key) {
                    window.minimized = previous.minimized;
                    window.fullscreen = previous.fullscreen;
                    window.eligible = previous.eligible;
                    if window.title.is_empty() {
                        window.title.clone_from(&previous.title);
                    }
                    if window.spaces.is_none() {
                        window.spaces.clone_from(&previous.spaces);
                    }
                }
            }
            self.inventory.upsert_surface(Surface {
                key,
                parent: None,
                bounds: window.bounds,
                visible: Some(native.on_screen),
            });
            if self.cursor_window.is_none()
                && native.on_screen
                && window
                    .bounds
                    .is_some_and(|bounds| bounds.contains(cursor.0, cursor.1))
            {
                self.cursor_window = Some(key);
            }
            window.created = key.incarnation;
            let changed = self.inventory.window(key).is_some_and(|old| {
                old.bounds != window.bounds
                    || old.title != window.title
                    || old.minimized != window.minimized
                    || old.fullscreen != window.fullscreen
            });
            if changed || (native.focused && self.attended != Some(ItemKey::Window(key))) {
                self.refresh_capture(ItemKey::Window(key));
            }
            self.inventory.upsert_window(window);
            if native.focused {
                focused = Some(ItemKey::Window(key));
            }
        }
        let removed: Vec<_> = self
            .window_keys
            .values()
            .copied()
            .filter(|key| !seen.contains(key) && !uncertain.contains(&key.app.pid))
            .collect();
        for key in removed {
            self.inventory.confirm_destroyed(key);
            self.window_keys.remove(&(key.app, key.cg_window_id));
            self.mru.remove(ItemKey::Window(key));
            self.cache.remove(ItemKey::Window(key));
            self.evict_image(ItemKey::Window(key));
        }
        if !self.seeded && !seed.is_empty() {
            self.mru.seed(&seed);
            self.seeded = true;
        }
        let active = self.app_keys.get(&front_pid).copied();
        if active != self.active_app && focused.is_none() {
            if let Some(key) = active {
                self.attended = Some(ItemKey::App(key));
                self.mru.focus(ItemKey::App(key));
            }
        }
        self.active_app = active;
        if let Some(focus) = focused {
            if self.attended != Some(focus) {
                self.mru.focus(focus);
                self.attended = Some(focus);
            }
        }
        self.visible_spaces = visible_spaces.into_iter().map(SpaceId).collect();
        let suppressed = self.suppressed();
        input::suppressed(suppressed);
        if suppressed {
            let effects = self.session.handle(Input::Cancel);
            self.effects(effects);
        }
        self.render_dirty = true;
        if self.session.phase() != Phase::Idle {
            let candidates = self.candidates(self.session.profile());
            let effects = self.session.reconcile(candidates);
            self.effects(effects);
        }
    }
    fn render(&mut self) {
        if self.session.phase() != Phase::Visible {
            return;
        }
        if !self.render_dirty {
            self.ui.selection(
                self.session
                    .selected()
                    .and_then(|key| self.entries.iter().position(|entry| entry.0 == key)),
                self.session.selection_source() == SelectionSource::Keyboard,
            );
            self.capture_next();
            return;
        }
        self.render_dirty = false;
        let screen = self
            .frozen_screen
            .unwrap_or_else(|| self.ui.screen(self.config.appearance.screen, None));
        self.entries.clear();
        self.sizes.clear();
        for key in self.session.items() {
            let Some(app) = self.inventory.app(key.app()) else {
                continue;
            };
            let window = match key {
                ItemKey::Window(key) => self.inventory.window(*key),
                _ => None,
            };
            let title = window
                .map(|window| window.title.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(&app.name);
            let title = match self.config.appearance.title_content {
                TitleContent::AppName => app.name.clone(),
                TitleContent::WindowTitle => title.to_owned(),
                TitleContent::AppAndWindow => {
                    if title == app.name {
                        title.to_owned()
                    } else {
                        format!("{} — {title}", app.name)
                    }
                }
            };
            self.entries.push((*key, app.key.pid, title));
            self.sizes.push(
                window
                    .and_then(|w| w.bounds)
                    .map(|r| Size {
                        width: r.width,
                        height: r.height,
                    })
                    .unwrap_or(Size {
                        width: 160.0,
                        height: 120.0,
                    }),
            );
        }
        if !self.sizes.is_empty() || !self.session.searching() {
            self.layout = layout(
                &self.sizes,
                &LayoutOptions {
                    screen: screen.rect,
                    backing_scale: screen.scale,
                    style: self.config.appearance.style,
                    size: self.config.appearance.size,
                    max_width_ratio: self.config.appearance.max_width_ratio,
                    max_height_ratio: self.config.appearance.max_height_ratio,
                    nominal_rows: self.config.appearance.nominal_rows,
                    physical_width_mm: screen.physical_width_mm,
                    ..LayoutOptions::default()
                },
            );
        }
        self.session
            .handle(Input::SetRows(self.layout.tiles.clone()));
        self.ui.render(
            &self.layout,
            &self.entries,
            self.session.selected(),
            &self.images,
            self.config.appearance.style,
            self.session.searching(),
            self.session.selection_source() == SelectionSource::Keyboard,
        );
        for (key, _, _) in &self.entries {
            self.cache.touch(*key);
        }
        self.capture_next();
    }
    fn begin_search(&mut self) {
        self.search_started = false;
        self.query.clear();
        self.render_dirty = true;
        input::search(true);
        let effects = self.session.handle(Input::EnterSearch);
        self.effects(effects);
        self.ui.start_search();
    }
    fn start_repeat(&mut self) {
        self.repeat_token = self.repeat_token.wrapping_add(1);
        later(
            self.repeat_timing.0,
            Event::Repeat {
                generation: self.session.generation(),
                token: self.repeat_token,
            },
        );
    }
    fn refresh_capture(&mut self, key: ItemKey) {
        if self.session.phase() != Phase::Visible
            || self.config.appearance.style != Style::Thumbnails
            || !self.recording
            || !self.session.items().contains(&key)
            || !self.refresh_scheduled.insert(key)
        {
            return;
        }
        let interval = if self.session.selected() == Some(key) {
            1000 / u64::from(self.config.capture.selected_max_hz)
        } else {
            1000
        };
        let delay = self.capture_attempts.get(&key).map_or(0, |last| {
            last.saturating_add(interval).saturating_sub(self.now())
        });
        later(
            delay,
            Event::Recapture {
                key,
                generation: self.cache.generation(),
            },
        );
    }
    fn capture_next(&mut self) {
        if self.capture_busy
            || !self.recording
            || self.session.phase() != Phase::Visible
            || self.config.appearance.style != Style::Thumbnails
        {
            return;
        }
        self.captures.clear();
        let viewport = self.ui.viewport();
        if let Some(key) = self.session.selected() {
            self.captures.push_back(key);
        }
        self.captures.extend(
            self.session
                .items()
                .iter()
                .copied()
                .filter(|key| Some(*key) != self.session.selected()),
        );
        while let Some(key) = self.captures.pop_front() {
            if self.images.contains_key(&key) || self.capture_attempts.contains_key(&key) {
                continue;
            }
            let ItemKey::Window(window) = key else {
                continue;
            };
            let Some(index) = self.entries.iter().position(|entry| entry.0 == key) else {
                continue;
            };
            let Some(tile) = self.layout.tiles.get(index) else {
                continue;
            };
            if (tile.y + tile.height < viewport.0 || tile.y > viewport.1)
                && Some(key) != self.session.selected()
            {
                continue;
            }
            let scale = self.frozen_screen.map_or(1.0, |screen| screen.scale);
            let Some(pixels) = downscale_to_budget(
                PixelSize {
                    width: ((tile.width - 16.0).max(1.0) * scale).ceil() as u32,
                    height: ((tile.height - 37.0).max(1.0) * scale).ceil() as u32,
                },
                self.config.capture.budget_bytes().min(8 * 1024 * 1024),
            ) else {
                continue;
            };
            let width = pixels.width as usize;
            let height = pixels.height as usize;
            let bytes = ((width * 4 + 63) & !63).saturating_mul(height);
            let Ok(reservation) = self.cache.reserve(key, pixels, bytes) else {
                continue;
            };
            for key in reservation.evicted {
                self.evict_image(key);
            }
            self.capture_attempts.insert(key, self.now());
            self.capture_busy = true;
            let source_pixels = self
                .inventory
                .window(window)
                .and_then(|w| w.bounds)
                .map_or(usize::MAX, |bounds| {
                    (bounds.width * bounds.height * scale * scale).max(0.0) as usize
                });
            capture::request(Request {
                key,
                id: window.cg_window_id,
                generation: reservation.generation,
                token: reservation.token,
                width,
                height,
                source_pixels,
            });
            break;
        }
    }
    fn captured(&mut self, request: Request, image: Option<Image>) {
        self.capture_busy = false;
        if let Some(image) = image {
            if let Ok(evicted) =
                self.cache
                    .complete(request.token, request.generation, image.bytes())
            {
                for key in evicted {
                    self.evict_image(key);
                }
                self.images.insert(request.key, image);
                if let Some(index) = self.entries.iter().position(|entry| entry.0 == request.key) {
                    self.ui.screenshot(index, self.images.get(&request.key));
                }
            }
        } else {
            self.cache.fail(request.token, request.generation);
        }
        if self.session.phase() == Phase::Visible {
            self.render();
        }
    }
    fn evict_image(&mut self, key: ItemKey) {
        if let Some(index) = self.entries.iter().position(|(item, _, _)| *item == key) {
            self.ui.screenshot(index, None);
        }
        self.images.remove(&key);
    }
    fn permissions(&mut self, recheck: bool) {
        let trusted = unsafe { ffi::AXIsProcessTrusted() };
        let recording = unsafe { ffi::CGPreflightScreenCaptureAccess() };
        if self.trusted && !trusted {
            let effects = self.session.handle(Input::Cancel);
            self.effects(effects);
            input::active(false);
        }
        if self.recording && !recording {
            let effects = self.session.handle(Input::Cancel);
            self.effects(effects);
            self.cache.hide();
            self.images.clear();
        }
        self.trusted = trusted;
        self.recording = recording;
        input::enabled(trusted && self.started);
        if trusted && (recording || self.skipped) {
            if !self.started {
                self.started = true;
                if !self.tap_started {
                    input::start();
                    self.tap_started = true;
                }
                self.tracker.refresh();
            }
            self.ui.close_permissions();
            input::enabled(true);
        } else if !recheck || !self.started {
            self.ui.permissions(trusted, recording, self.started);
        }
    }
    fn setting(&mut self, tag: isize) {
        if tag == 7 {
            match self.config.to_toml() {
                Ok(text) => self.ui.advanced(&text),
                Err(error) => self.ui.error(&error.to_string()),
            };
            return;
        }
        if tag == 8 {
            match Config::from_toml(&self.ui.configuration_text()) {
                Ok(config) => self.save_config(config),
                Err(error) => self.ui.error(&error.to_string()),
            }
            return;
        }
        if tag >= 10 {
            match tag {
                10 => unsafe {
                    let key = ffi::string("AXTrustedCheckOptionPrompt");
                    let dictionary: *mut AnyObject = msg_send![class!(NSDictionary),dictionaryWithObject:ffi::kCFBooleanTrue as *const AnyObject,forKey:key.0 as *const AnyObject];
                    if !ffi::AXIsProcessTrustedWithOptions(dictionary.cast()) {
                        open_settings("Privacy_Accessibility");
                    }
                    self.permissions(false);
                },
                11 => {
                    self.skipped = false;
                    recording_skipped(Some(false));
                    std::thread::spawn(|| {
                        autoreleasepool(|_| unsafe {
                            if !ffi::CGRequestScreenCaptureAccess() {
                                open_settings("Privacy_ScreenCapture");
                            }
                            post(Event::Workspace);
                        })
                    });
                }
                12 => {
                    self.skipped = true;
                    recording_skipped(Some(true));
                    self.permissions(false);
                }
                13 => self.permissions(false),
                _ => {}
            }
            return;
        }
        let mut config = self.config.clone();
        let index = self.ui.control(tag);
        match tag {
            1 => {
                for profile in &mut config.shortcuts {
                    profile.hold = vec![if index == 0 {
                        Modifier::Option
                    } else {
                        Modifier::Command
                    }];
                }
            }
            2 => {
                config.appearance.style = match index {
                    1 => Style::Icons,
                    2 => Style::Titles,
                    _ => Style::Thumbnails,
                }
            }
            3 => {
                for profile in &mut config.shortcuts {
                    profile.minimized = visibility(index);
                }
            }
            4 => {
                for profile in &mut config.shortcuts {
                    profile.hidden = visibility(index);
                }
            }
            5 => {
                for profile in &mut config.shortcuts {
                    profile.fullscreen = if index == 1 {
                        FullscreenPolicy::Hide
                    } else {
                        FullscreenPolicy::Show
                    };
                }
            }
            6 => config.launch_at_login = index != 0,
            _ => return,
        }
        self.save_config(config);
    }
    fn save_config(&mut self, config: Config) {
        if let Err(error) = config.validate() {
            self.ui.error(&error.to_string());
            self.sync_settings();
            return;
        }
        if !config.capture.evict_on_hide {
            self.ui
                .error("The native adapter requires capture.evict_on_hide = true.");
            return;
        }
        if config.launch_at_login != self.config.launch_at_login {
            if let Err(error) = login(config.launch_at_login) {
                self.ui.error(&error);
                self.sync_settings();
                return;
            }
        }
        if let Err(error) = config.save(&self.path) {
            self.ui.error(&error.to_string());
            self.sync_settings();
            return;
        }
        self.config = config;
        let effects = self.session.handle(Input::Cancel);
        self.effects(effects);
        self.apply_config();
        self.sync_settings();
    }
    fn apply_config(&mut self) {
        input::configure(&self.config.shortcuts);
        input::suppressed(self.suppressed());
        self.ui
            .appearance(&self.config.appearance, self.config.menubar_icon);
        let evicted = self.cache.set_budget(self.config.capture.budget_bytes());
        for key in evicted {
            self.evict_image(key);
        }
        unsafe {
            let _: () = msg_send![&self.ui.status,setVisible:self.config.menubar_visible];
        }
    }
    fn sync_settings(&self) {
        let profile = &self.config.shortcuts[0];
        self.ui
            .set_control(1, isize::from(profile.hold.contains(&Modifier::Command)));
        self.ui.set_control(
            2,
            match self.config.appearance.style {
                Style::Thumbnails => 0,
                Style::Icons => 1,
                Style::Titles => 2,
            },
        );
        self.ui.set_control(3, visibility_index(profile.minimized));
        self.ui.set_control(4, visibility_index(profile.hidden));
        self.ui
            .set_control(5, isize::from(profile.fullscreen == FullscreenPolicy::Hide));
        self.ui
            .set_control(6, isize::from(self.config.launch_at_login));
    }
}
fn visibility(index: isize) -> Visibility {
    match index {
        1 => Visibility::Hide,
        2 => Visibility::ShowAtEnd,
        _ => Visibility::Show,
    }
}
fn visibility_index(value: Visibility) -> isize {
    match value {
        Visibility::Show => 0,
        Visibility::Hide => 1,
        Visibility::ShowAtEnd => 2,
    }
}
fn repeat_timing() -> (u64, u64) {
    unsafe {
        let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let initial: f64 =
            msg_send![defaults,doubleForKey:&*NSString::from_str("InitialKeyRepeat")];
        let repeat: f64 = msg_send![defaults,doubleForKey:&*NSString::from_str("KeyRepeat")];
        let ticks = |value: f64, fallback: f64| {
            if value.is_finite() && value > 0.0 {
                value.clamp(1.0, 600.0)
            } else {
                fallback
            }
        };
        (
            (ticks(initial, 25.0) * 1000.0 / 60.0) as u64,
            (ticks(repeat, 6.0) * 1000.0 / 60.0) as u64,
        )
    }
}
fn modifiers(flags: u64) -> Modifiers {
    Modifiers(
        (if flags & (1 << 20) != 0 { 1 } else { 0 })
            | (if flags & (1 << 19) != 0 { 2 } else { 0 })
            | (if flags & (1 << 18) != 0 { 4 } else { 0 })
            | (if flags & (1 << 17) != 0 { 8 } else { 0 })
            | (if flags & (1 << 16) != 0 { 16 } else { 0 }),
    )
}
fn open_settings(pane: &str) {
    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let url: *mut AnyObject = msg_send![class!(NSURL),URLWithString:&*NSString::from_str(&format!("x-apple.systempreferences:com.apple.preference.security?{pane}"))];
        if !url.is_null() {
            let _: bool = msg_send![workspace,openURL:url];
        }
    }
}
fn recording_skipped(set: Option<bool>) -> bool {
    unsafe {
        let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let key = NSString::from_str("alttab-rs.screenRecordingSkipped");
        if let Some(value) = set {
            let _: () = msg_send![defaults,setBool:value,forKey:&*key];
        }
        msg_send![defaults,boolForKey:&*key]
    }
}
fn login(enabled: bool) -> Result<(), String> {
    unsafe {
        static LIBRARY: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| unsafe {
            ffi::dlopen(
                c"/System/Library/Frameworks/ServiceManagement.framework/ServiceManagement"
                    .as_ptr(),
                1,
            ) as usize
        });
        let _ = *LIBRARY;
        let Some(class) = objc2::runtime::AnyClass::get(c"SMAppService") else {
            return Err("Login registration requires macOS 13 or later. Start alttab-rs manually on macOS 12.".into());
        };
        let service: *mut AnyObject = msg_send![class, mainAppService];
        let status: isize = msg_send![service, status];
        if (enabled && status == 1) || (!enabled && (status == 0 || status == 3)) {
            return Ok(());
        }
        if enabled && status == 2 {
            return Err("Login launch requires approval in System Settings → General → Login Items. Approve alttab-rs, then enable this preference again.".into());
        }
        let mut error = std::ptr::null_mut::<AnyObject>();
        let success: bool = if enabled {
            msg_send![service,registerAndReturnError:&mut error]
        } else {
            msg_send![service,unregisterAndReturnError:&mut error]
        };
        if success {
            return Ok(());
        }
        if error.is_null() {
            return Err("The OS could not update login registration. Run the signed application bundle from Applications.".into());
        }
        let message: *const NSString = msg_send![error, localizedDescription];
        Err(if message.is_null() {
            "Login registration failed".into()
        } else {
            (*message).to_string()
        })
    }
}
