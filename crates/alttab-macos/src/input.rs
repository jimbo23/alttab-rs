use crate::{
    app::{self, Event},
    ffi,
};
use objc2::rc::autoreleasepool;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static SEARCH: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(false);
static SUPPRESSED: AtomicBool = AtomicBool::new(false);
static ACTIVE_HOLD: AtomicU64 = AtomicU64::new(0);
static CHORDS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];
static SWALLOWED: [AtomicBool; 128] = [const { AtomicBool::new(false) }; 128];
static TAP: AtomicUsize = AtomicUsize::new(0);
static SERIAL: AtomicU64 = AtomicU64::new(1);
pub fn active(active: bool) {
    ACTIVE.store(active, Ordering::Release);
}
pub fn search(search: bool) {
    SEARCH.store(search, Ordering::Release);
}
pub fn enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Release);
}
pub fn suppressed(suppressed: bool) {
    SUPPRESSED.store(suppressed, Ordering::Release);
}
pub fn active_profile(index: usize) {
    ACTIVE_HOLD.store(
        CHORDS
            .get(index)
            .map_or(0, |chord| chord.load(Ordering::Acquire) >> 8),
        Ordering::Release,
    );
}
pub fn hold_is_down(flags: u64) -> bool {
    let hold = ACTIVE_HOLD.load(Ordering::Acquire);
    hold != 0 && flags & hold == hold
}
pub fn event_flags(modifiers: alttab_core::Modifiers) -> u64 {
    let mask = modifiers.0;
    (if mask & 1 != 0 { 1 << 20 } else { 0 })
        | (if mask & 2 != 0 { 1 << 19 } else { 0 })
        | (if mask & 4 != 0 { 1 << 18 } else { 0 })
        | (if mask & 8 != 0 { 1 << 17 } else { 0 })
        | (if mask & 16 != 0 { 1 << 16 } else { 0 })
}
pub fn configure(shortcuts: &[alttab_core::Shortcut]) {
    for (index, chord) in CHORDS.iter().enumerate() {
        let value = shortcuts.get(index).map_or(0, |profile| {
            let flags = event_flags(profile.hold_mask());
            (flags << 8) | if profile.next == "tab" { 48 } else { 50 }
        });
        chord.store(value, Ordering::Release);
    }
}
pub fn start() {
    std::thread::Builder::new().name("alttab keyboard".into()).spawn(|| unsafe {
        autoreleasepool(|_| {
            let tap = ffi::CGEventTapCreate(0, 0, 0, (1 << 10) | (1 << 11) | (1 << 12), callback, ptr::null_mut());
            if tap.is_null() { app::post(Event::Error("Cannot install the global keyboard tap. Grant Accessibility and Input Monitoring permission, then relaunch alttab-rs.".into())); return; }
            TAP.store(tap as usize, Ordering::Release);
            let tap = ffi::Owned(tap);
            let Some(source) = ffi::Owned::from_create(ffi::CFMachPortCreateRunLoopSource(ptr::null(), tap.0, 0)) else{app::post(Event::Error("Cannot create the keyboard run-loop source.".into()));return;};
            ffi::CFRunLoopAddSource(ffi::CFRunLoopGetCurrent(), source.0, ffi::kCFRunLoopDefaultMode);
            ffi::CGEventTapEnable(tap.0 as ffi::MutRef, true);
            ffi::CFRunLoopRun();
        });
    }).expect("create keyboard thread");
}
unsafe extern "C" fn callback(
    _: ffi::MutRef,
    kind: u32,
    event: ffi::MutRef,
    _: ffi::MutRef,
) -> ffi::MutRef {
    let result = std::panic::catch_unwind(|| {
        autoreleasepool(|_| {
            if kind == 0xffff_fffe || kind == 0xffff_ffff {
                ACTIVE.store(false, Ordering::Release);
                app::post(Event::TapReset);
                ffi::CGEventTapEnable(TAP.load(Ordering::Acquire) as ffi::MutRef, true);
                return event;
            }
            if !ENABLED.load(Ordering::Acquire) || SUPPRESSED.load(Ordering::Acquire) {
                return event;
            }
            let flags = ffi::CGEventGetFlags(event);
            let key = ffi::CGEventGetIntegerValueField(event, 9) as u16;
            let active = ACTIVE.load(Ordering::Acquire);
            if kind == 11 {
                return if SWALLOWED
                    .get(key as usize)
                    .is_some_and(|state| state.swap(false, Ordering::AcqRel))
                {
                    ptr::null_mut()
                } else {
                    event
                };
            }
            let invoke = CHORDS.iter().any(|chord| {
                let chord = chord.load(Ordering::Acquire);
                chord != 0 && (chord & 255) == u64::from(key) && flags & (chord >> 8) == chord >> 8
            });
            let active = active
                && (!SEARCH.load(Ordering::Acquire)
                    || (hold_is_down(flags) && matches!(key, 3 | 4 | 12 | 13 | 46)));
            if kind == 12 || (kind == 10 && (invoke || active)) {
                let repeat = ffi::CGEventGetIntegerValueField(event, 8) != 0;
                app::post(Event::Key {
                    key,
                    flags,
                    kind,
                    repeat,
                    serial: SERIAL.fetch_add(1, Ordering::Relaxed),
                });
            }
            if kind == 10 && (invoke || active) {
                if let Some(state) = SWALLOWED.get(key as usize) {
                    state.store(true, Ordering::Release);
                }
                ptr::null_mut()
            } else {
                event
            }
        })
    });
    result.unwrap_or(event)
}
