use crate::{
    App, AppKey, AppScope, BlacklistRule, DisplayId, FullscreenPolicy, HideRule, Inventory,
    ItemKey, Mru, ScreenScope, Shortcut, SpaceId, SpaceScope, SuppressRule, Visibility, Window,
    WindowKey, WindowOrder,
};
use std::cmp::Ordering;

#[derive(Clone, Debug, Default)]
pub struct FilterContext {
    pub active_app: Option<AppKey>,
    pub visible_spaces: Vec<SpaceId>,
    pub switcher_display: Option<DisplayId>,
    pub cursor_window: Option<WindowKey>,
    pub cursor_app: Option<AppKey>,
}

impl BlacklistRule {
    pub fn matches(&self, bundle_id: &str) -> bool {
        !self.bundle_id_prefix.is_empty() && bundle_id.starts_with(&self.bundle_id_prefix)
    }
    pub fn suppresses(&self, app: &App, fullscreen: bool) -> bool {
        self.matches(&app.bundle_id)
            && match self.suppress_shortcuts {
                SuppressRule::Never => false,
                SuppressRule::Always => true,
                SuppressRule::Fullscreen => fullscreen,
            }
    }
}

fn allowed(
    app: &App,
    window: Option<&Window>,
    profile: &Shortcut,
    rules: &[BlacklistRule],
    context: &FilterContext,
) -> bool {
    let app_matches = match profile.apps {
        AppScope::All => true,
        AppScope::Active => context.active_app == Some(app.key),
        AppScope::NonActive => context.active_app != Some(app.key),
        AppScope::WindowsUnderCursor => {
            window.is_some_and(|w| context.cursor_window == Some(w.key))
        }
        AppScope::AppUnderCursor => context.cursor_app == Some(app.key),
    };
    if !app_matches || (app.hidden && profile.hidden == Visibility::Hide) {
        return false;
    }
    let bypass_blanket = matches!(profile.apps, AppScope::Active | AppScope::AppUnderCursor);
    for rule in rules.iter().filter(|rule| rule.matches(&app.bundle_id)) {
        match rule.hide {
            HideRule::Always if !bypass_blanket => return false,
            HideRule::Windowless if window.is_none() && !bypass_blanket => return false,
            HideRule::TitleContains
                if window.is_some_and(|w| {
                    rule.title_patterns
                        .iter()
                        .any(|p| !p.is_empty() && w.title.contains(p))
                }) =>
            {
                return false
            }
            _ => {}
        }
    }
    let Some(window) = window else {
        return profile.windowless != Visibility::Hide;
    };
    if !window.eligible
        || (window.minimized && profile.minimized == Visibility::Hide)
        || (window.fullscreen && profile.fullscreen == FullscreenPolicy::Hide)
    {
        return false;
    }
    // Unknown membership is not an empty membership set (SPEC §3.3).
    let visible = window
        .spaces
        .as_ref()
        .map(|spaces| spaces.iter().any(|s| context.visible_spaces.contains(s)));
    match profile.spaces {
        SpaceScope::Visible if visible == Some(false) => return false,
        SpaceScope::NonVisible if visible == Some(true) => return false,
        _ => {}
    }
    if profile.screens == ScreenScope::SwitcherScreen {
        if let (Some(display), Some(displays)) = (context.switcher_display, &window.displays) {
            if !displays.contains(&display) {
                return false;
            }
        }
    }
    if profile.apps == AppScope::WindowsUnderCursor
        && (window.minimized || app.hidden || visible == Some(false))
    {
        return false;
    }
    true
}

/// End partitions use windowless, hidden, minimized lexicographic precedence.
/// Source: src/switcher/state/WindowOrderResolver.swift:54–108.
pub fn candidates(
    inventory: &Inventory,
    mru: &Mru,
    profile: &Shortcut,
    rules: &[BlacklistRule],
    context: &FilterContext,
) -> Vec<ItemKey> {
    let mut items = Vec::new();
    for app in inventory.apps() {
        let mut has_window = false;
        for window in inventory
            .windows()
            .filter(|window| window.key.app == app.key && window.eligible)
        {
            has_window = true;
            if allowed(app, Some(window), profile, rules, context) {
                items.push(ItemKey::Window(window.key));
            }
        }
        if !has_window && allowed(app, None, profile, rules, context) {
            items.push(ItemKey::App(app.key));
        }
    }
    let buckets = |key: ItemKey| {
        let app = inventory.app(key.app()).expect("candidate app");
        let window = match key {
            ItemKey::Window(key) => inventory.window(key),
            ItemKey::App(_) => None,
        };
        (
            window.is_none() && profile.windowless == Visibility::ShowAtEnd,
            app.hidden && profile.hidden == Visibility::ShowAtEnd,
            window.is_some_and(|w| w.minimized) && profile.minimized == Visibility::ShowAtEnd,
        )
    };
    let title = |key: ItemKey| match key {
        ItemKey::Window(key) => inventory.window(key).map_or("", |w| w.title.as_str()),
        ItemKey::App(_) => "",
    };
    let created = |key: ItemKey| match key {
        ItemKey::Window(key) => inventory.window(key).map_or(0, |w| w.created),
        ItemKey::App(_) => 0,
    };
    let space = |key: ItemKey| match key {
        ItemKey::Window(key) => inventory
            .window(key)
            .and_then(|w| w.spaces.as_ref())
            .and_then(|s| s.iter().min())
            .copied(),
        ItemKey::App(_) => None,
    };
    items.sort_by(|a, b| {
        buckets(*a).cmp(&buckets(*b)).then_with(|| {
            let primary = match profile.order {
                WindowOrder::RecentlyFocused => Ordering::Equal,
                WindowOrder::RecentlyCreated => created(*b).cmp(&created(*a)),
                WindowOrder::Alphabetical => inventory
                    .app(a.app())
                    .unwrap()
                    .name
                    .cmp(&inventory.app(b.app()).unwrap().name)
                    .then_with(|| title(*a).cmp(title(*b))),
                WindowOrder::Space => space(*a).cmp(&space(*b)),
            };
            primary.then_with(|| mru.compare(*a, *b))
        })
    });
    items
}
