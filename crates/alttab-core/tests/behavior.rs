use alttab_core::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn app_key(pid: i32) -> AppKey {
    AppKey {
        pid,
        incarnation: 1,
    }
}
fn window_key(pid: i32, id: u32) -> WindowKey {
    WindowKey {
        app: app_key(pid),
        cg_window_id: id,
        incarnation: 1,
    }
}
fn item(id: u32) -> ItemKey {
    ItemKey::Window(window_key(1, id))
}
fn app(pid: i32) -> App {
    App {
        key: app_key(pid),
        bundle_id: format!("com.example.App{pid}"),
        name: format!("App{pid}"),
        hidden: false,
    }
}
fn inventory() -> Inventory {
    let mut inventory = Inventory::default();
    inventory.upsert_app(app(1));
    inventory.upsert_app(app(2));
    for (pid, id) in [(1, 1), (1, 2), (2, 3)] {
        let mut window = Window::new(window_key(pid, id), format!("Window {id}"));
        window.spaces = Some(vec![SpaceId(u64::from(id))]);
        window.displays = Some(vec![DisplayId(id)]);
        window.created = u64::from(id);
        inventory.upsert_window(window);
    }
    inventory
}
fn list(inventory: &Inventory, profile: &Shortcut, context: &FilterContext) -> Vec<ItemKey> {
    candidates(inventory, &Mru::default(), profile, &[], context)
}
fn open_session(items: Vec<ItemKey>) -> Session {
    let mut session = Session::default();
    session.summon(
        0,
        Modifiers::OPTION,
        ReleaseAction::Focus,
        items,
        Some(item(1)),
        0,
        0,
    );
    session
}
fn temporary() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "alttab-core-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn process_and_window_reuse_reject_old_callbacks() {
    let mut inventory = inventory();
    let original = window_key(1, 1);
    assert!(inventory.confirm_destroyed(original));
    assert!(!inventory.upsert_window(Window::new(original, "late")));
    let replacement = WindowKey {
        incarnation: 2,
        ..original
    };
    assert!(inventory.upsert_window(Window::new(replacement, "replacement")));
    assert!(!inventory.confirm_destroyed(original));
    assert!(inventory.window(replacement).is_some());
    let next_app = AppKey {
        incarnation: 2,
        ..app_key(1)
    };
    assert!(inventory.upsert_app(App {
        key: next_app,
        ..app(1)
    }));
    assert!(!inventory.upsert_app(app(1)));
    assert!(!inventory.remove_app(app_key(1)));
    assert!(!inventory.upsert_window(Window::new(replacement, "stale process")));
    assert!(inventory.app(next_app).is_some());
}

#[test]
fn zero_window_id_is_not_a_placeholder_and_ax_failure_is_not_destruction() {
    let mut inventory = inventory();
    assert!(!inventory.upsert_window(Window::new(window_key(1, 0), "invalid")));
    let mut failed = inventory.window(window_key(1, 1)).unwrap().clone();
    failed.acquisition = Acquisition::Unknown;
    assert!(inventory.upsert_window(failed));
    assert!(list(&inventory, &Shortcut::default(), &FilterContext::default()).contains(&item(1)));
    let mut absent = inventory.window(window_key(1, 1)).unwrap().clone();
    absent.acquisition = Acquisition::Absent;
    inventory.upsert_window(absent);
    assert!(inventory.window(window_key(1, 1)).is_some());
}

#[test]
fn physical_child_surfaces_do_not_become_switch_destinations() {
    let mut inventory = inventory();
    let child = window_key(1, 50);
    inventory.upsert_surface(Surface {
        key: window_key(1, 1),
        parent: None,
        bounds: None,
        visible: Some(true),
    });
    inventory.upsert_surface(Surface {
        key: child,
        parent: Some(window_key(1, 1)),
        bounds: None,
        visible: Some(true),
    });
    assert_eq!(inventory.surface_root(child), Some(window_key(1, 1)));
    assert_eq!(inventory.windows().count(), 3);
    inventory.upsert_surface(Surface {
        key: window_key(1, 1),
        parent: Some(child),
        bounds: None,
        visible: None,
    });
    assert_eq!(inventory.surface_root(child), None);
}

#[test]
fn observed_focus_orders_but_metadata_and_traversal_do_not() {
    let mut inventory = inventory();
    let mut mru = Mru::default();
    mru.seed(&[item(1), item(2)]);
    assert_eq!(
        candidates(
            &inventory,
            &mru,
            &Shortcut::default(),
            &[],
            &FilterContext::default()
        )[0],
        item(1)
    );
    mru.focus(item(2));
    let mut changed = inventory.window(window_key(1, 1)).unwrap().clone();
    changed.title = "New title".into();
    inventory.upsert_window(changed);
    let keys = candidates(
        &inventory,
        &mru,
        &Shortcut::default(),
        &[],
        &FilterContext::default(),
    );
    let mut session = open_session(keys.clone());
    session.handle(Input::Cycle {
        backward: false,
        repeat: false,
    });
    assert_eq!(
        candidates(
            &inventory,
            &mru,
            &Shortcut::default(),
            &[],
            &FilterContext::default()
        ),
        keys
    );
    assert_eq!(keys[0], item(2));
    mru.seed(&[item(1), item(2)]);
    assert_eq!(mru.compare(item(2), item(1)), std::cmp::Ordering::Less);
}

#[test]
fn status_filters_compose_over_every_boolean_combination() {
    for minimized in [false, true] {
        for hidden in [false, true] {
            for fullscreen in [false, true] {
                let mut inventory = Inventory::default();
                inventory.upsert_app(App { hidden, ..app(1) });
                let mut window = Window::new(window_key(1, 1), "window");
                window.minimized = minimized;
                window.fullscreen = fullscreen;
                inventory.upsert_window(window);
                for hide_minimized in [false, true] {
                    for hide_hidden in [false, true] {
                        for hide_fullscreen in [false, true] {
                            let profile = Shortcut {
                                minimized: if hide_minimized {
                                    Visibility::Hide
                                } else {
                                    Visibility::Show
                                },
                                hidden: if hide_hidden {
                                    Visibility::Hide
                                } else {
                                    Visibility::Show
                                },
                                fullscreen: if hide_fullscreen {
                                    FullscreenPolicy::Hide
                                } else {
                                    FullscreenPolicy::Show
                                },
                                ..Shortcut::default()
                            };
                            let expected = !(minimized && hide_minimized
                                || hidden && hide_hidden
                                || fullscreen && hide_fullscreen);
                            assert_eq!(
                                !list(&inventory, &profile, &FilterContext::default()).is_empty(),
                                expected
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn unknown_membership_is_conservative_but_known_empty_is_not_unknown() {
    let mut inventory = inventory();
    let mut window = inventory.window(window_key(1, 1)).unwrap().clone();
    window.spaces = None;
    window.displays = None;
    inventory.upsert_window(window.clone());
    let context = FilterContext {
        visible_spaces: vec![SpaceId(2)],
        switcher_display: Some(DisplayId(2)),
        ..FilterContext::default()
    };
    let mut profile = Shortcut {
        spaces: SpaceScope::Visible,
        screens: ScreenScope::SwitcherScreen,
        ..Shortcut::default()
    };
    assert!(list(&inventory, &profile, &context).contains(&item(1)));
    profile.spaces = SpaceScope::NonVisible;
    assert!(list(&inventory, &profile, &context).contains(&item(1)));
    window.spaces = Some(Vec::new());
    window.displays = Some(Vec::new());
    inventory.upsert_window(window);
    assert!(!list(&inventory, &profile, &context).contains(&item(1)));
    profile.screens = ScreenScope::All;
    assert!(list(&inventory, &profile, &context).contains(&item(1)));
    profile.spaces = SpaceScope::Visible;
    assert!(!list(&inventory, &profile, &context).contains(&item(1)));
}

#[test]
fn spaces_test_all_visible_displays_and_screen_filter_is_independent() {
    let inventory = inventory();
    let context = FilterContext {
        visible_spaces: vec![SpaceId(1), SpaceId(3)],
        switcher_display: Some(DisplayId(1)),
        ..FilterContext::default()
    };
    let mut profile = Shortcut {
        spaces: SpaceScope::Visible,
        ..Shortcut::default()
    };
    assert_eq!(
        list(&inventory, &profile, &context),
        vec![item(1), ItemKey::Window(window_key(2, 3))]
    );
    profile.screens = ScreenScope::SwitcherScreen;
    assert_eq!(list(&inventory, &profile, &context), vec![item(1)]);
}

#[test]
fn active_nonactive_and_cursor_app_scopes_are_distinct() {
    let inventory = inventory();
    let context = FilterContext {
        active_app: Some(app_key(1)),
        cursor_app: Some(app_key(2)),
        cursor_window: Some(window_key(1, 2)),
        visible_spaces: vec![SpaceId(2)],
        ..FilterContext::default()
    };
    for (scope, expected) in [
        (AppScope::Active, vec![item(1), item(2)]),
        (AppScope::NonActive, vec![ItemKey::Window(window_key(2, 3))]),
        (
            AppScope::AppUnderCursor,
            vec![ItemKey::Window(window_key(2, 3))],
        ),
        (AppScope::WindowsUnderCursor, vec![item(2)]),
    ] {
        assert_eq!(
            list(
                &inventory,
                &Shortcut {
                    apps: scope,
                    ..Shortcut::default()
                },
                &context
            ),
            expected
        );
    }
}

#[test]
fn cursor_scope_cannot_admit_hidden_minimized_or_offspace_windows() {
    let profile = Shortcut {
        apps: AppScope::WindowsUnderCursor,
        ..Shortcut::default()
    };
    let context = FilterContext {
        cursor_window: Some(window_key(1, 1)),
        visible_spaces: vec![SpaceId(1)],
        ..FilterContext::default()
    };
    for state in 0..3 {
        let mut inventory = inventory();
        let mut window = inventory.window(window_key(1, 1)).unwrap().clone();
        if state == 0 {
            window.minimized = true;
        }
        if state == 1 {
            inventory.upsert_app(App {
                hidden: true,
                ..app(1)
            });
        }
        if state == 2 {
            window.spaces = Some(vec![SpaceId(2)]);
        }
        inventory.upsert_window(window);
        assert!(list(&inventory, &profile, &context).is_empty());
    }
}

#[test]
fn windowless_placeholders_bypass_real_window_gates_not_app_exceptions() {
    let mut inventory = inventory();
    inventory.upsert_app(app(3));
    let profile = Shortcut {
        spaces: SpaceScope::NonVisible,
        screens: ScreenScope::SwitcherScreen,
        minimized: Visibility::Hide,
        fullscreen: FullscreenPolicy::Hide,
        ..Shortcut::default()
    };
    let context = FilterContext {
        visible_spaces: vec![SpaceId(1), SpaceId(2), SpaceId(3)],
        switcher_display: Some(DisplayId(99)),
        ..FilterContext::default()
    };
    assert_eq!(
        list(&inventory, &profile, &context),
        vec![ItemKey::App(app_key(3))]
    );
    let rule = BlacklistRule {
        bundle_id_prefix: "com.example.App3".into(),
        hide: HideRule::Windowless,
        ..BlacklistRule::default()
    };
    assert!(candidates(&inventory, &Mru::default(), &profile, &[rule], &context).is_empty());
}

#[test]
fn literal_case_sensitive_blacklist_and_active_override_match_consumer_policy() {
    let inventory = inventory();
    let context = FilterContext {
        active_app: Some(app_key(1)),
        ..FilterContext::default()
    };
    let rule = BlacklistRule {
        bundle_id_prefix: "com.example.App1".into(),
        hide: HideRule::Always,
        suppress_shortcuts: SuppressRule::Fullscreen,
        ..BlacklistRule::default()
    };
    assert_eq!(
        candidates(
            &inventory,
            &Mru::default(),
            &Shortcut::default(),
            std::slice::from_ref(&rule),
            &context
        ),
        vec![ItemKey::Window(window_key(2, 3))]
    );
    let profile = Shortcut {
        apps: AppScope::Active,
        ..Shortcut::default()
    };
    assert_eq!(
        candidates(
            &inventory,
            &Mru::default(),
            &profile,
            std::slice::from_ref(&rule),
            &context
        ),
        vec![item(1), item(2)]
    );
    assert!(!rule.suppresses(&app(1), false));
    assert!(rule.suppresses(&app(1), true));
    let title_rule = BlacklistRule {
        hide: HideRule::TitleContains,
        title_patterns: vec!["Window 1".into(), "*".into()],
        ..rule
    };
    assert_eq!(
        candidates(
            &inventory,
            &Mru::default(),
            &profile,
            std::slice::from_ref(&title_rule),
            &context
        ),
        vec![item(2)]
    );
    let wrong_case = BlacklistRule {
        bundle_id_prefix: "Com.example".into(),
        ..title_rule
    };
    assert_eq!(
        candidates(
            &inventory,
            &Mru::default(),
            &profile,
            &[wrong_case],
            &context
        ),
        vec![item(1), item(2)]
    );
}

#[test]
fn end_buckets_and_alternative_orders_keep_deterministic_mru_ties() {
    let mut inventory = inventory();
    inventory.upsert_app(app(3));
    inventory.upsert_app(App {
        hidden: true,
        ..app(2)
    });
    let mut minimized = inventory.window(window_key(1, 1)).unwrap().clone();
    minimized.minimized = true;
    inventory.upsert_window(minimized);
    let profile = Shortcut {
        minimized: Visibility::ShowAtEnd,
        hidden: Visibility::ShowAtEnd,
        ..Shortcut::default()
    };
    assert_eq!(
        list(&inventory, &profile, &FilterContext::default()),
        vec![
            item(2),
            item(1),
            ItemKey::Window(window_key(2, 3)),
            ItemKey::App(app_key(3))
        ]
    );
    let profile = Shortcut {
        order: WindowOrder::RecentlyCreated,
        windowless: Visibility::Hide,
        ..Shortcut::default()
    };
    assert_eq!(
        list(&inventory, &profile, &FilterContext::default()),
        vec![ItemKey::Window(window_key(2, 3)), item(2), item(1)]
    );
    let profile = Shortcut {
        order: WindowOrder::Alphabetical,
        windowless: Visibility::Hide,
        ..Shortcut::default()
    };
    assert_eq!(
        list(&inventory, &profile, &FilterContext::default()),
        vec![item(1), item(2), ItemKey::Window(window_key(2, 3))]
    );
}

#[test]
fn initial_selection_handles_attended_filtered_out_single_and_empty_lists() {
    assert_eq!(
        open_session(vec![item(1), item(2), item(3)]).selected(),
        Some(item(2))
    );
    assert_eq!(
        open_session(vec![item(2), item(3)]).selected(),
        Some(item(2))
    );
    assert_eq!(open_session(vec![item(1)]).selected(), Some(item(1)));
    assert_eq!(open_session(Vec::new()).phase(), Phase::Idle);
}

#[test]
fn quick_release_focuses_without_render_and_clears_before_dispatch() {
    let mut session = Session::default();
    let scheduled = session.summon(
        0,
        Modifiers::OPTION,
        ReleaseAction::Focus,
        vec![item(1), item(2)],
        Some(item(1)),
        100,
        100,
    );
    let generation = session.generation();
    assert_eq!(
        scheduled,
        vec![Effect::ScheduleShow {
            generation,
            deadline_ms: 200
        }]
    );
    assert_eq!(
        session.handle(Input::Modifiers(Modifiers::default())),
        vec![Effect::Hide { generation }, Effect::Focus(item(2))]
    );
    assert_eq!(session.phase(), Phase::Idle);
    assert!(session.items().is_empty());
    assert!(session.handle(Input::Show { generation }).is_empty());
    assert!(session
        .handle(Input::Modifiers(Modifiers::default()))
        .is_empty());
}

#[test]
fn extra_modifiers_do_not_release_hold_and_unrelated_keyup_is_not_acceptance() {
    let mut session = open_session(vec![item(1), item(2)]);
    assert!(session
        .handle(Input::Modifiers(
            Modifiers::OPTION | Modifiers::COMMAND | Modifiers::SHIFT
        ))
        .is_empty());
    assert!(session
        .handle(Input::Modifiers(Modifiers::OPTION))
        .is_empty());
    assert_eq!(session.phase(), Phase::Visible);
    assert!(session
        .handle(Input::Modifiers(Modifiers::COMMAND))
        .iter()
        .any(|e| *e == Effect::Focus(item(2))));
}

#[test]
fn cancel_never_focuses_hold_and_search_styles_wait_for_explicit_accept() {
    for release in [ReleaseAction::Hold, ReleaseAction::Search] {
        let mut session = Session::default();
        session.summon(
            0,
            Modifiers::OPTION,
            release,
            vec![item(1)],
            Some(item(1)),
            0,
            0,
        );
        assert!(session
            .handle(Input::Modifiers(Modifiers::default()))
            .is_empty());
        assert_eq!(session.phase(), Phase::Visible);
        assert!(session
            .handle(Input::Cancel)
            .iter()
            .all(|effect| !matches!(effect, Effect::Focus(_))));
        assert_eq!(session.phase(), Phase::Idle);
    }
    let mut session = open_session(vec![item(1)]);
    assert!(session
        .handle(Input::Accept)
        .contains(&Effect::Focus(item(1))));
}

#[test]
fn manual_navigation_wraps_repeats_stop_and_physical_sources_deduplicate() {
    let mut session = open_session(vec![item(1), item(2), item(3)]);
    session.handle(Input::Physical {
        serial: 1,
        action: PhysicalAction::Tab {
            shift: false,
            repeat: false,
        },
    });
    assert_eq!(session.selected(), Some(item(3)));
    assert!(session
        .handle(Input::Physical {
            serial: 1,
            action: PhysicalAction::Tab {
                shift: false,
                repeat: false
            }
        })
        .is_empty());
    session.handle(Input::Cycle {
        backward: false,
        repeat: true,
    });
    assert_eq!(session.selected(), Some(item(3)));
    session.handle(Input::Cycle {
        backward: false,
        repeat: false,
    });
    assert_eq!(session.selected(), Some(item(1)));
    session.handle(Input::Cycle {
        backward: true,
        repeat: true,
    });
    assert_eq!(session.selected(), Some(item(1)));
    session.handle(Input::Physical {
        serial: 2,
        action: PhysicalAction::ShiftPressed,
    });
    assert_eq!(session.selected(), Some(item(3)));
    session.handle(Input::Physical {
        serial: 3,
        action: PhysicalAction::Tab {
            shift: true,
            repeat: false,
        },
    });
    assert_eq!(session.selected(), Some(item(2)));
}

#[test]
fn selection_follows_identity_and_removal_uses_nearest_surviving_slot() {
    let mut session = open_session(vec![item(1), item(2), item(3)]);
    session.handle(Input::Cycle {
        backward: false,
        repeat: false,
    });
    session.reconcile(vec![item(3), item(1), item(2)]);
    assert_eq!(session.selected(), Some(item(3)));
    session.reconcile(vec![item(1), item(2)]);
    assert_eq!(session.selected(), Some(item(1)));
    let effects = session.reconcile(Vec::new());
    assert_eq!(
        effects,
        vec![Effect::Hide {
            generation: session.generation()
        }]
    );
    assert_eq!(session.phase(), Phase::Idle);
}

#[test]
fn default_selection_settles_until_navigation_and_profiles_fence_old_timers() {
    let mut session = Session::default();
    session.summon(
        0,
        Modifiers::OPTION,
        ReleaseAction::Focus,
        vec![item(1)],
        Some(item(1)),
        0,
        100,
    );
    let old = session.generation();
    session.reconcile(vec![item(1), item(2)]);
    assert_eq!(session.selected(), Some(item(2)));
    session.summon(
        1,
        Modifiers::COMMAND,
        ReleaseAction::Focus,
        vec![item(1), item(3)],
        Some(item(1)),
        10,
        100,
    );
    assert_eq!(session.profile(), 1);
    assert_eq!(session.selected(), Some(item(3)));
    assert!(session.handle(Input::Show { generation: old }).is_empty());
    assert!(session
        .handle(Input::Modifiers(Modifiers::COMMAND | Modifiers::SHIFT))
        .is_empty());
}

#[test]
fn vertical_arrows_choose_nearest_horizontal_center_and_wrap_rows() {
    let mut session = open_session(vec![item(1), item(2), item(3)]);
    session.handle(Input::SetRows(vec![
        Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 20.0,
        },
        Rect {
            x: 90.0,
            y: 0.0,
            width: 80.0,
            height: 20.0,
        },
        Rect {
            x: 50.0,
            y: 30.0,
            width: 80.0,
            height: 20.0,
        },
    ]));
    session.handle(Input::Arrow {
        direction: Direction::Down,
        repeat: false,
    });
    assert_eq!(session.selected(), Some(item(3)));
    session.handle(Input::Arrow {
        direction: Direction::Down,
        repeat: true,
    });
    assert_eq!(session.selected(), Some(item(3)));
    session.handle(Input::Arrow {
        direction: Direction::Down,
        repeat: false,
    });
    assert_eq!(session.selected(), Some(item(2)));
    session.handle(Input::Arrow {
        direction: Direction::Left,
        repeat: false,
    });
    assert_eq!(session.selected(), Some(item(1)));
}

#[test]
fn aspect_fit_never_stretches_or_upscales_without_opt_in() {
    let source = Size {
        width: 400.0,
        height: 200.0,
    };
    assert_eq!(
        aspect_fit(
            source,
            Size {
                width: 100.0,
                height: 100.0
            },
            false
        ),
        Size {
            width: 100.0,
            height: 50.0
        }
    );
    assert_eq!(
        aspect_fit(
            source,
            Size {
                width: 800.0,
                height: 800.0
            },
            false
        ),
        source
    );
    assert_eq!(
        aspect_fit(
            source,
            Size {
                width: 800.0,
                height: 800.0
            },
            true
        ),
        Size {
            width: 800.0,
            height: 400.0
        }
    );
    assert_eq!(
        aspect_fit(
            Size {
                width: f64::NAN,
                height: 20.0
            },
            source,
            false
        ),
        Size::default()
    );
    assert_eq!(
        pixel_size(
            Size {
                width: 1.1,
                height: 3.2
            },
            2.0
        ),
        (3, 7)
    );
}

#[test]
fn grids_bound_geometry_without_overlap_for_every_style_and_scale() {
    let items: Vec<_> = (1..=75)
        .map(|index| Size {
            width: f64::from(index * 103),
            height: f64::from(index % 7 + 1),
        })
        .collect();
    for style in [Style::Thumbnails, Style::Icons, Style::Titles] {
        for size in [
            SizePreset::Small,
            SizePreset::Medium,
            SizePreset::Large,
            SizePreset::Auto,
        ] {
            for (width, height) in [(100.0, 80.0), (900.0, 1600.0), (6000.0, 2000.0)] {
                let result = layout(
                    &items,
                    &LayoutOptions {
                        style,
                        size,
                        screen: Rect {
                            x: -500.0,
                            y: 20.0,
                            width,
                            height,
                        },
                        backing_scale: 2.0,
                        ..LayoutOptions::default()
                    },
                );
                assert_eq!(result.tiles.len(), items.len());
                assert!(result.panel.width <= width && result.panel.height <= height);
                assert!(result.document_size.height >= result.viewport_size.height);
                for (index, tile) in result.tiles.iter().enumerate() {
                    assert!(
                        tile.x >= -0.000001
                            && tile.y >= 0.0
                            && tile.width > 0.0
                            && tile.height > 0.0
                    );
                    assert!(tile.x + tile.width <= result.panel.width + 0.000001);
                    assert!(tile.y + tile.height <= result.document_size.height + 0.000001);
                    for other in &result.tiles[index + 1..] {
                        assert!(!tile.intersects(*other));
                    }
                }
            }
        }
    }
}

#[test]
fn nominal_rows_are_not_a_hard_row_limit_and_tiny_screens_are_safe() {
    let result = layout(
        &vec![
            Size {
                width: 1000.0,
                height: 500.0
            };
            300
        ],
        &LayoutOptions {
            nominal_rows: Some(3),
            ..LayoutOptions::default()
        },
    );
    assert!(result.rows.last().copied().unwrap() > 3);
    assert!(result.document_size.height > result.viewport_size.height);
    for dimension in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let empty = layout(
            &[Size {
                width: 100.0,
                height: 100.0,
            }],
            &LayoutOptions {
                screen: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: dimension,
                    height: dimension,
                },
                ..LayoutOptions::default()
            },
        );
        assert!(empty.tiles.is_empty());
        assert!(empty.panel.width.is_finite());
    }
}

#[test]
fn config_defaults_roundtrip_and_partial_values_have_one_source_of_truth() {
    let defaults = Config::default();
    assert_eq!(
        Config::from_toml(&defaults.to_toml().unwrap()).unwrap(),
        defaults
    );
    assert_eq!(Config::from_toml("").unwrap(), defaults);
    let partial = Config::from_toml("[appearance]\nstyle = 'icons'\n").unwrap();
    assert_eq!(partial.appearance.style, Style::Icons);
    assert_eq!(partial.capture.cache_mib, 32);
    assert_eq!(partial.shortcuts[0].hold_mask(), Modifiers::OPTION);
    assert!(!partial.launch_at_login);
    assert_eq!(
        application_support_path(std::path::Path::new("/home/alice")),
        PathBuf::from("/home/alice/Library/Application Support/alttab-rs/config.toml")
    );
}

#[test]
fn invalid_config_is_reported_not_silently_reset() {
    for text in [
        "schema_version = 2",
        "[capture]\ncache_mib = 129",
        "[capture]\ncache_mib = -1",
        "[appearance]\nstyle = 'unknown'",
        "[appearance]\nmax_height_ratio = 0.0",
        "[appearance]\nmax_width_ratio = nan",
        "shortcuts = []",
        "unrecognized = true",
        "[malformed",
    ] {
        assert!(Config::from_toml(text).is_err(), "{text}");
    }
    let mut config = Config::default();
    config.shortcuts[1] = config.shortcuts[0].clone();
    assert!(config.validate().is_err());
    config.shortcuts[1].id = "different".into();
    assert!(config.validate().is_err());
    config.shortcuts[1].next = "key_above_tab".into();
    config.shortcuts[1].hold.push(Modifier::Option);
    assert!(config.validate().is_err());
}

#[test]
fn atomic_save_creates_parents_preserves_prior_file_on_invalid_snapshot() {
    let directory = temporary();
    let path = directory.join("nested/config.toml");
    let config = Config::default();
    assert_eq!(Config::load(&path).unwrap(), config);
    config.save(&path).unwrap();
    assert_eq!(Config::load(&path).unwrap(), config);
    let previous = fs::read(&path).unwrap();
    let invalid = Config {
        schema_version: 9,
        ..config.clone()
    };
    assert!(invalid.save(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    fs::write(&path, "invalid = [").unwrap();
    assert!(Config::load(&path).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "invalid = [");
    assert!(config.save(&directory).is_err());
    assert!(Config::load(&directory).is_err());
    let blocker = directory.join("blocker");
    fs::write(&blocker, "not a directory").unwrap();
    assert!(config.save(blocker.join("config.toml")).is_err());
    fs::remove_dir_all(directory).unwrap();
}

fn ready(cache: &mut ThumbnailCache, key: ItemKey, bytes: usize) -> Vec<ItemKey> {
    let reservation = cache
        .reserve(
            key,
            PixelSize {
                width: 1,
                height: 1,
            },
            bytes,
        )
        .unwrap();
    let mut evicted = reservation.evicted;
    evicted.extend(
        cache
            .complete(reservation.token, reservation.generation, bytes)
            .unwrap(),
    );
    evicted
}

#[test]
fn cache_charges_pending_reservations_and_evicts_least_displayed_use() {
    let mut cache = ThumbnailCache::new(100);
    cache.begin_session();
    assert!(ready(&mut cache, item(1), 40).is_empty());
    assert!(ready(&mut cache, item(2), 40).is_empty());
    cache.touch(item(1));
    let reservation = cache
        .reserve(
            item(3),
            PixelSize {
                width: 1,
                height: 1,
            },
            40,
        )
        .unwrap();
    assert_eq!(reservation.evicted, vec![item(2)]);
    assert_eq!(cache.charged_bytes(), 80);
    assert_eq!(cache.ready_bytes(), 40);
    assert!(!cache.contains(item(3)));
    cache
        .complete(reservation.token, reservation.generation, 40)
        .unwrap();
    assert!(cache.contains(item(1)) && cache.contains(item(3)));
}

#[test]
fn actual_stride_charge_evicts_before_attachment_and_duplicate_completion_is_stale() {
    let mut cache = ThumbnailCache::new(100);
    cache.begin_session();
    ready(&mut cache, item(1), 40);
    let reservation = cache
        .reserve(
            item(2),
            PixelSize {
                width: 2,
                height: 2,
            },
            16,
        )
        .unwrap();
    assert_eq!(
        cache
            .complete(reservation.token, reservation.generation, 80)
            .unwrap(),
        vec![item(1)]
    );
    assert_eq!(cache.charged_bytes(), 80);
    assert_eq!(
        cache.complete(reservation.token, reservation.generation, 80),
        Err(CacheError::Stale)
    );
    assert_eq!(
        PixelSize {
            width: 2,
            height: 2
        }
        .bytes_with_stride(64),
        Some(128)
    );
    assert_eq!(
        PixelSize {
            width: 2,
            height: 2
        }
        .bytes_with_stride(7),
        None
    );
}

#[test]
fn replacement_failures_oversized_frames_and_budget_shrink_preserve_hard_cap() {
    let mut cache = ThumbnailCache::new(100);
    cache.begin_session();
    ready(&mut cache, item(1), 50);
    assert_eq!(
        cache.reserve(
            item(2),
            PixelSize {
                width: 1,
                height: 1
            },
            101
        ),
        Err(CacheError::Oversized)
    );
    assert!(cache.contains(item(1)));
    let replacing = cache
        .reserve(
            item(1),
            PixelSize {
                width: 1,
                height: 1,
            },
            80,
        )
        .unwrap();
    assert_eq!(replacing.evicted, vec![item(1)]);
    assert!(cache.fail(replacing.token, replacing.generation));
    assert_eq!(cache.charged_bytes(), 0);
    let oversized_actual = cache
        .reserve(
            item(1),
            PixelSize {
                width: 1,
                height: 1,
            },
            40,
        )
        .unwrap();
    assert_eq!(
        cache.complete(oversized_actual.token, oversized_actual.generation, 101),
        Err(CacheError::Oversized)
    );
    assert_eq!(cache.charged_bytes(), 0);
    ready(&mut cache, item(1), 40);
    ready(&mut cache, item(2), 40);
    assert_eq!(cache.set_budget(30), vec![item(1), item(2)]);
    assert_eq!(cache.charged_bytes(), 0);
}

#[test]
fn hide_erases_all_charges_and_late_callbacks_cannot_repopulate_new_session() {
    let mut cache = ThumbnailCache::default();
    assert_eq!(cache.budget(), 32 * 1024 * 1024);
    assert_eq!(
        cache.reserve(
            item(1),
            PixelSize {
                width: 1,
                height: 1
            },
            4
        ),
        Err(CacheError::Hidden)
    );
    cache.begin_session();
    ready(&mut cache, item(1), 4);
    let reservation = cache
        .reserve(
            item(2),
            PixelSize {
                width: 1,
                height: 1,
            },
            4,
        )
        .unwrap();
    assert_eq!(cache.hide(), vec![item(1), item(2)]);
    assert_eq!(cache.ready_bytes(), 0);
    assert_eq!(cache.charged_bytes(), 0);
    cache.begin_session();
    assert_eq!(
        cache.complete(reservation.token, reservation.generation, 4),
        Err(CacheError::Stale)
    );
    assert!(!cache.contains(item(2)));
}

#[test]
fn downscale_fits_extreme_aspects_and_zero_budgets_without_overflow() {
    for source in [
        PixelSize {
            width: 4000,
            height: 2000,
        },
        PixelSize {
            width: u32::MAX,
            height: 1,
        },
        PixelSize {
            width: 1,
            height: u32::MAX,
        },
    ] {
        for budget in [4, 400, 1024 * 1024, 32 * 1024 * 1024] {
            let size = downscale_to_budget(source, budget).unwrap();
            assert!(size.width <= source.width && size.height <= source.height);
            assert!(size.bgra_bytes().unwrap() <= budget);
        }
    }
    assert_eq!(
        downscale_to_budget(
            PixelSize {
                width: 1,
                height: 1
            },
            3
        ),
        None
    );
    assert_eq!(
        downscale_to_budget(
            PixelSize {
                width: 0,
                height: 1
            },
            10
        ),
        None
    );
}

#[test]
fn capture_cadence_is_visible_demand_only_and_selected_has_faster_ceiling() {
    let mut cadence = CaptureCadence::default();
    assert!(!cadence.admit(item(1), 0, true, false, Style::Thumbnails, true));
    assert!(!cadence.admit(item(1), 0, true, true, Style::Icons, true));
    assert!(!cadence.admit(item(1), 0, true, true, Style::Thumbnails, false));
    assert!(cadence.admit(item(1), 0, true, true, Style::Thumbnails, true));
    assert!(!cadence.admit(item(1), 199, true, true, Style::Thumbnails, true));
    assert!(cadence.admit(item(1), 200, true, true, Style::Thumbnails, true));
    assert!(!cadence.admit(item(1), 1199, false, true, Style::Thumbnails, true));
    assert!(cadence.admit(item(1), 1200, false, true, Style::Thumbnails, true));
    cadence.hide();
    assert!(cadence.admit(item(1), 1201, false, true, Style::Thumbnails, true));
}

#[test]
fn optional_hidden_retention_keeps_only_ready_images_and_never_accepts_old_capture() {
    let mut cache = ThumbnailCache::new(100);
    cache.set_evict_on_hide(false);
    cache.begin_session();
    ready(&mut cache, item(1), 40);
    let pending = cache
        .reserve(
            item(2),
            PixelSize {
                width: 1,
                height: 1,
            },
            40,
        )
        .unwrap();
    assert_eq!(cache.hide(), vec![item(2)]);
    assert_eq!(cache.charged_bytes(), 40);
    assert!(cache.contains(item(1)));
    assert_eq!(
        cache.complete(pending.token, pending.generation, 40),
        Err(CacheError::Stale)
    );
    cache.begin_session();
    assert_eq!(
        cache.complete(pending.token, pending.generation, 40),
        Err(CacheError::Stale)
    );
    cache.set_evict_on_hide(true);
    assert_eq!(cache.hide(), vec![item(1)]);
    assert_eq!(cache.charged_bytes(), 0);
    assert!(Config::default().capture.evict_on_hide);
    assert!(
        !Config::from_toml("[capture]\nevict_on_hide = false")
            .unwrap()
            .capture
            .evict_on_hide
    );
}

#[test]
fn configured_selected_capture_rate_is_enforced_without_rounding_above_ceiling() {
    for hz in 1..=5 {
        let config = CaptureConfig {
            selected_max_hz: hz,
            ..CaptureConfig::default()
        };
        let mut cadence = CaptureCadence::from_config(&config);
        let interval = 1000_u64.div_ceil(u64::from(hz));
        assert!(cadence.admit(item(1), 0, true, true, Style::Thumbnails, true));
        assert!(!cadence.admit(item(1), interval - 1, true, true, Style::Thumbnails, true));
        assert!(cadence.admit(item(1), interval, true, true, Style::Thumbnails, true));
    }
}

#[test]
fn search_holds_session_open_for_zero_results_and_can_restore_candidates() {
    let mut session = open_session(vec![item(1), item(2)]);
    assert_eq!(session.handle(Input::EnterSearch), vec![Effect::Render]);
    assert!(session.searching());
    assert!(session
        .handle(Input::Modifiers(Modifiers::default()))
        .is_empty());
    assert_eq!(session.reconcile(Vec::new()), vec![Effect::Render]);
    assert_eq!(session.phase(), Phase::Visible);
    assert_eq!(session.selected(), None);
    assert!(session
        .handle(Input::Cycle {
            backward: false,
            repeat: false
        })
        .is_empty());
    assert_eq!(
        session.reconcile(vec![item(1), item(2)]),
        vec![Effect::Render]
    );
    assert_eq!(session.selected(), Some(item(1)));
    session.handle(Input::ExitSearch);
    assert!(!session.searching());
    assert!(session
        .handle(Input::Modifiers(Modifiers::default()))
        .contains(&Effect::Focus(item(1))));
}

#[test]
fn accepting_or_cancelling_an_empty_search_never_focuses_a_stale_selection() {
    for action in [Input::Accept, Input::Cancel] {
        let mut session = open_session(vec![item(1), item(2)]);
        session.handle(Input::EnterSearch);
        session.reconcile(Vec::new());
        let generation = session.generation();
        assert_eq!(session.handle(action), vec![Effect::Hide { generation }]);
        assert_eq!(session.phase(), Phase::Idle);
        assert!(session.handle(Input::EnterSearch).is_empty());
        assert!(!session.searching());
    }
}

#[test]
fn search_defaults_to_top_ranked_result_even_when_attended_window_is_present() {
    let mut session = open_session(vec![item(1), item(2), item(3)]);
    session.handle(Input::EnterSearch);
    session.reconcile(vec![item(3), item(1), item(2)]);
    assert_eq!(session.selected(), Some(item(3)));
    session.handle(Input::Cycle {
        backward: false,
        repeat: false,
    });
    session.reconcile(vec![item(2), item(3), item(1)]);
    assert_eq!(session.selected(), Some(item(1)));
}
