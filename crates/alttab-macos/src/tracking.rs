use crate::{
    app::{self, Event},
    ffi::{
        self, CFRunLoopRun, CFRunLoopSourceCreate, CFRunLoopSourceSignal, CFRunLoopWakeUp, Owned,
        Ref, SourceContext,
    },
};
use objc2::{class, msg_send, rc::autoreleasepool, runtime::AnyObject};
use objc2_foundation::NSString;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Mutex};
static FOCUS_INTENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[derive(Clone, Copy)]
pub enum Action {
    Focus,
    Close,
    Minimize,
    Fullscreen,
    Quit,
    Hide,
}
#[derive(Clone, Debug)]
pub struct Application {
    pub pid: i32,
    pub generation: u64,
    pub name: String,
    pub bundle: String,
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct Window {
    pub pid: i32,
    pub id: u32,
    pub title: String,
    pub bounds: [f64; 4],
    pub minimized: bool,
    pub fullscreen: bool,
    pub focused: bool,
    pub on_screen: bool,
    pub spaces: Option<Vec<u64>>,
    pub eligible: bool,
}
#[derive(Default)]
struct Requests {
    refresh: bool,
}
struct Shared {
    requests: Mutex<Requests>,
    source: usize,
    loop_: usize,
}
pub struct Tracker(Arc<Shared>);
struct Context {
    shared: Arc<Shared>,
    observers: HashMap<i32, Observer>,
    icons: HashMap<i32, u64>,
}
struct Observer {
    observer: Owned,
    application: Owned,
    windows: HashMap<u32, usize>,
    generation: u64,
}
impl Tracker {
    pub fn start() -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("alttab AX queries".into())
            .spawn(move || unsafe {
                autoreleasepool(|_| {
                    let loop_ = ffi::CFRunLoopGetCurrent();
                    let mut context = Box::new(Context {
                        shared: Arc::new(Shared {
                            requests: Mutex::new(Requests::default()),
                            source: 0,
                            loop_: loop_ as usize,
                        }),
                        observers: HashMap::new(),
                        icons: HashMap::new(),
                    });
                    let mut source_context = SourceContext {
                        version: 0,
                        info: (&mut *context as *mut Context).cast(),
                        retain: ptr::null(),
                        release: ptr::null(),
                        description: ptr::null(),
                        equal: ptr::null(),
                        hash: ptr::null(),
                        schedule: ptr::null(),
                        cancel: ptr::null(),
                        perform: Some(perform),
                    };
                    let source = Owned::from_create(CFRunLoopSourceCreate(
                        ptr::null(),
                        0,
                        &mut source_context,
                    ))
                    .expect("create AX command run-loop source");
                    Arc::get_mut(&mut context.shared)
                        .expect("unshared initialization")
                        .source = source.0 as usize;
                    ffi::CFRunLoopAddSource(loop_, source.0, ffi::kCFRunLoopDefaultMode);
                    if tx.send(context.shared.clone()).is_err() {
                        return;
                    }
                    CFRunLoopRun();
                });
            })
            .expect("create AX worker");
        Self(rx.recv().expect("AX worker startup"))
    }
    pub fn refresh(&self) {
        self.0.requests.lock().unwrap().refresh = true;
        self.wake();
    }
    pub fn cancel_focus(&self) {
        FOCUS_INTENT.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
    pub fn action(&self, app: alttab_core::AppKey, id: u32, action: Action) {
        use std::sync::{Condvar, LazyLock};
        type Lane = Arc<(
            Mutex<Option<(alttab_core::AppKey, u32, Action, u64)>>,
            Condvar,
        )>;
        static COMMANDS: LazyLock<Lane> = LazyLock::new(|| {
            let lane: Lane = Arc::new((Mutex::new(None), Condvar::new()));
            let worker = lane.clone();
            std::thread::Builder::new()
                .name("alttab AX commands".into())
                .spawn(move || loop {
                    let (state, wake) = &*worker;
                    let mut pending = state.lock().unwrap();
                    while pending.is_none() {
                        pending = wake.wait(pending).unwrap();
                    }
                    let (app, id, action, intent) = pending.take().unwrap();
                    drop(pending);
                    autoreleasepool(|_| unsafe {
                        command(app, id, action, intent);
                        app::post(Event::Refresh);
                    });
                })
                .expect("create command worker");
            lane
        });
        let lane = &*COMMANDS;
        let intent = FOCUS_INTENT
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            .wrapping_add(1);
        *lane.0.lock().unwrap() = Some((app, id, action, intent));
        lane.1.notify_one();
    }
    fn wake(&self) {
        unsafe {
            CFRunLoopSourceSignal(self.0.source as Ref);
            CFRunLoopWakeUp(self.0.loop_ as Ref);
        }
    }
}
unsafe extern "C" fn perform(info: *mut c_void) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        autoreleasepool(|_| {
            let context = &mut *info.cast::<Context>();
            let requests = std::mem::take(&mut *context.shared.requests.lock().unwrap());
            if requests.refresh {
                reconcile(context);
            }
        })
    }));
}
unsafe extern "C" fn observed(_: Ref, _: Ref, _: Ref, _: *mut c_void) {
    autoreleasepool(|_| app::post(Event::Refresh));
}
unsafe fn subscribe(observer: Ref, element: Ref, names: &[&str]) {
    for name in names {
        ffi::AXObserverAddNotification(observer, element, ffi::string(name).0, ptr::null_mut());
    }
}
unsafe fn reconcile(context: &mut Context) {
    let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
    let apps: *mut AnyObject = msg_send![workspace, runningApplications];
    let count: usize = msg_send![apps, count];
    let front: *mut AnyObject = msg_send![workspace, frontmostApplication];
    let front_pid: i32 = if front.is_null() {
        0
    } else {
        msg_send![front, processIdentifier]
    };
    let mut living = HashSet::new();
    let mut info = HashMap::new();
    let mut applications = Vec::new();
    let mut icons = Vec::new();
    for i in 0..count {
        let app: *mut AnyObject = msg_send![apps, objectAtIndex: i];
        let pid: i32 = msg_send![app, processIdentifier];
        let policy: isize = msg_send![app, activationPolicy];
        if policy != 0 || pid == std::process::id() as i32 {
            continue;
        }
        let name: *const NSString = msg_send![app, localizedName];
        let bundle: *const NSString = msg_send![app, bundleIdentifier];
        let hidden: bool = msg_send![app, isHidden];
        info.insert(
            pid,
            (
                if name.is_null() {
                    String::new()
                } else {
                    (*name).to_string()
                },
                if bundle.is_null() {
                    String::new()
                } else {
                    (*bundle).to_string()
                },
                hidden,
            ),
        );
        let date: *mut AnyObject = msg_send![app, launchDate];
        let launch: f64 = if date.is_null() {
            0.0
        } else {
            msg_send![date, timeIntervalSince1970]
        };
        applications.push(Application {
            pid,
            generation: launch.to_bits(),
            name: if name.is_null() {
                String::new()
            } else {
                (*name).to_string()
            },
            bundle: if bundle.is_null() {
                String::new()
            } else {
                (*bundle).to_string()
            },
            hidden,
        });
        if context.icons.get(&pid) != Some(&launch.to_bits())
            && (context.icons.len() < 32 || context.icons.contains_key(&pid))
        {
            let icon: Option<objc2::rc::Retained<AnyObject>> = msg_send![app, icon];
            if let Some(icon) = icon {
                let image: *const objc2_core_graphics::CGImage = msg_send![&icon,CGImageForProposedRect:ptr::null_mut::<objc2_foundation::NSRect>(),context:ptr::null::<AnyObject>(),hints:ptr::null::<AnyObject>()];
                if let Some(image) = crate::capture::resize(image.cast(), 128, 128) {
                    icons.push((pid, launch.to_bits(), image));
                    context.icons.insert(pid, launch.to_bits());
                }
            }
        }
        if context
            .observers
            .get(&pid)
            .is_some_and(|observer| observer.generation != launch.to_bits())
        {
            if let Some(observer) = context.observers.remove(&pid) {
                ffi::CFRunLoopRemoveSource(
                    ffi::CFRunLoopGetCurrent(),
                    ffi::AXObserverGetRunLoopSource(observer.observer.0),
                    ffi::kCFRunLoopDefaultMode,
                );
            }
        }
        living.insert(pid);
        if let std::collections::hash_map::Entry::Vacant(entry) = context.observers.entry(pid) {
            let Some(application) = Owned::from_create(ffi::AXUIElementCreateApplication(pid))
            else {
                continue;
            };
            ffi::AXUIElementSetMessagingTimeout(application.0, 0.35);
            let mut observer = ptr::null();
            if ffi::AXObserverCreate(pid, observed, &mut observer) == 0 {
                let observer = Owned(observer);
                subscribe(
                    observer.0,
                    application.0,
                    &[
                        "AXWindowCreated",
                        "AXFocusedWindowChanged",
                        "AXMainWindowChanged",
                        "AXApplicationHidden",
                        "AXApplicationShown",
                    ],
                );
                ffi::CFRunLoopAddSource(
                    ffi::CFRunLoopGetCurrent(),
                    ffi::AXObserverGetRunLoopSource(observer.0),
                    ffi::kCFRunLoopDefaultMode,
                );
                entry.insert(Observer {
                    observer,
                    application,
                    windows: HashMap::new(),
                    generation: launch.to_bits(),
                });
            }
        }
    }
    context.observers.retain(|pid, observer| {
        if living.contains(pid) {
            return true;
        }
        ffi::CFRunLoopRemoveSource(
            ffi::CFRunLoopGetCurrent(),
            ffi::AXObserverGetRunLoopSource(observer.observer.0),
            ffi::kCFRunLoopDefaultMode,
        );
        false
    });
    context.icons.retain(|pid, _| living.contains(pid));
    let Some(cg) = Owned::from_create(ffi::CGWindowListCopyWindowInfo(0, 0)) else {
        return;
    };
    let mut windows = Vec::new();
    for i in 0..ffi::CFArrayGetCount(cg.0) {
        let row = ffi::CFArrayGetValueAtIndex(cg.0, i);
        let pid = ffi::number(ffi::dictionary(row, "kCGWindowOwnerPID")) as i32;
        if !info.contains_key(&pid) {
            continue;
        }
        if ffi::number(ffi::dictionary(row, "kCGWindowLayer")) != 0 {
            continue;
        }
        let id = ffi::number(ffi::dictionary(row, "kCGWindowNumber")) as u32;
        if id == 0 {
            continue;
        }
        let bounds = ffi::dictionary(row, "kCGWindowBounds");
        let mut geometry = [0.0; 4];
        for (slot, key) in geometry.iter_mut().zip(["X", "Y", "Width", "Height"]) {
            let value = ffi::dictionary(bounds, key);
            if !value.is_null() {
                ffi::CFNumberGetValue(value, 6, (slot as *mut f64).cast());
            }
        }
        if geometry[2] < 40.0 || geometry[3] < 20.0 {
            continue;
        }
        let onscreen = ffi::dictionary(row, "kCGWindowIsOnscreen");
        windows.push(Window {
            pid,
            id,
            title: ffi::text(ffi::dictionary(row, "kCGWindowName")),
            bounds: geometry,
            minimized: false,
            fullscreen: false,
            focused: false,
            on_screen: !onscreen.is_null() && ffi::CFBooleanGetValue(onscreen),
            spaces: ffi::spaces(Some(id)),
            eligible: !ffi::window_mapping_available(),
        });
    }
    let mut uncertain = HashSet::new();
    uncertain.extend(
        living
            .iter()
            .copied()
            .filter(|pid| !context.observers.contains_key(pid)),
    );
    for (pid, observer) in &mut context.observers {
        let Some(ax_windows) = ffi::attribute(observer.application.0, "AXWindows") else {
            uncertain.insert(*pid);
            continue;
        };
        let focused = ffi::attribute(observer.application.0, "AXFocusedWindow")
            .and_then(|a| ffi::window_id(a.0));
        let mut subscribed = HashMap::new();
        for i in 0..ffi::CFArrayGetCount(ax_windows.0) {
            let element = ffi::CFArrayGetValueAtIndex(ax_windows.0, i);
            let Some(id) = ffi::window_id(element) else {
                continue;
            };
            subscribed.insert(id, element as usize);
            if observer.windows.get(&id) != Some(&(element as usize)) {
                subscribe(
                    observer.observer.0,
                    element,
                    &[
                        "AXUIElementDestroyed",
                        "AXTitleChanged",
                        "AXWindowMiniaturized",
                        "AXWindowDeminiaturized",
                        "AXMoved",
                        "AXResized",
                    ],
                );
            }
            if !windows.iter().any(|w| w.pid == *pid && w.id == id) {
                let mut geometry = [0.0; 4];
                if let Some(position) = ffi::attribute(element, "AXPosition") {
                    ffi::AXValueGetValue(position.0, 1, geometry.as_mut_ptr().cast());
                }
                if let Some(size) = ffi::attribute(element, "AXSize") {
                    ffi::AXValueGetValue(size.0, 2, geometry.as_mut_ptr().add(2).cast());
                }
                if geometry[2] > 0.0 && geometry[3] > 0.0 {
                    windows.push(Window {
                        pid: *pid,
                        id,
                        title: String::new(),
                        bounds: geometry,
                        minimized: false,
                        fullscreen: false,
                        focused: false,
                        on_screen: false,
                        spaces: ffi::spaces(Some(id)),
                        eligible: true,
                    });
                }
            }
            if let Some(window) = windows.iter_mut().find(|w| w.pid == *pid && w.id == id) {
                if let Some(title) = ffi::attribute(element, "AXTitle") {
                    window.title = ffi::text(title.0);
                }
                window.minimized = ffi::attribute(element, "AXMinimized")
                    .is_some_and(|v| ffi::CFBooleanGetValue(v.0));
                window.fullscreen = ffi::attribute(element, "AXFullScreen")
                    .is_some_and(|v| ffi::CFBooleanGetValue(v.0));
                window.focused = front_pid == *pid && focused == Some(id);
                let role = ffi::attribute(element, "AXRole").map(|role| ffi::text(role.0));
                let subrole = ffi::attribute(element, "AXSubrole")
                    .map(|role| ffi::text(role.0))
                    .unwrap_or_default();
                window.eligible = role.as_deref() == Some("AXWindow")
                    && !matches!(subrole.as_str(), "AXFloatingWindow" | "AXSystemDialog");
            }
        }
        observer.windows = subscribed;
    }
    let visible_spaces = ffi::spaces(None);
    if visible_spaces.is_none() {
        for window in &mut windows {
            window.spaces = None;
        }
    }
    app::post(Event::Inventory {
        windows,
        applications,
        uncertain,
        front_pid,
        visible_spaces: visible_spaces.unwrap_or_default(),
    });
    for (pid, generation, image) in icons {
        app::post(Event::Icon {
            pid,
            generation,
            image,
        });
    }
}
unsafe fn command(key: alttab_core::AppKey, id: u32, action: Action, intent: u64) {
    let app: *mut AnyObject =
        msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: key.pid];
    if app.is_null() {
        return;
    }
    let date: *mut AnyObject = msg_send![app, launchDate];
    let launch: f64 = if date.is_null() {
        0.0
    } else {
        msg_send![date, timeIntervalSince1970]
    };
    if launch.to_bits() != key.incarnation {
        return;
    }
    if intent != FOCUS_INTENT.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    match action {
        Action::Quit => {
            let _: bool = msg_send![app, terminate];
            return;
        }
        Action::Hide => {
            let hidden: bool = msg_send![app, isHidden];
            let _: bool = if hidden {
                msg_send![app, unhide]
            } else {
                msg_send![app, hide]
            };
            return;
        }
        _ => {}
    }
    if id == 0 {
        if matches!(action, Action::Focus) {
            let _: bool = msg_send![app,activateWithOptions:2usize];
        }
        return;
    }
    let mut element = None;
    let Some(application) = Owned::from_create(ffi::AXUIElementCreateApplication(key.pid)) else {
        return;
    };
    ffi::AXUIElementSetMessagingTimeout(application.0, 0.5);
    if let Some(windows) = ffi::attribute(application.0, "AXWindows") {
        for i in 0..ffi::CFArrayGetCount(windows.0) {
            let window = ffi::CFArrayGetValueAtIndex(windows.0, i);
            if ffi::window_id(window) == Some(id) {
                element = Some(Owned(ffi::CFRetain(window)));
                break;
            }
        }
    }
    if !matches!(action, Action::Focus) {
        let Some(window) = element else {
            return;
        };
        match action {
            Action::Close => {
                if let Some(close) = ffi::attribute(window.0, "AXCloseButton") {
                    ffi::AXUIElementPerformAction(close.0, ffi::string("AXPress").0);
                }
            }
            Action::Minimize | Action::Fullscreen => {
                let attribute = if matches!(action, Action::Minimize) {
                    "AXMinimized"
                } else {
                    "AXFullScreen"
                };
                if let Some(value) = ffi::attribute(window.0, attribute) {
                    let value = if ffi::CFBooleanGetValue(value.0) {
                        ffi::kCFBooleanFalse
                    } else {
                        ffi::kCFBooleanTrue
                    };
                    ffi::AXUIElementSetAttributeValue(window.0, ffi::string(attribute).0, value);
                }
            }
            _ => {}
        }
        return;
    }
    if intent != FOCUS_INTENT.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    let hidden: bool = msg_send![app, isHidden];
    if hidden {
        let _: bool = msg_send![app, unhide];
    }
    if let Some(window) = &element {
        ffi::AXUIElementSetAttributeValue(
            window.0,
            ffi::string("AXMinimized").0,
            ffi::kCFBooleanFalse,
        );
        ffi::AXUIElementPerformAction(window.0, ffi::string("AXRaise").0);
    }
    if intent != FOCUS_INTENT.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    let _: bool = msg_send![app, activateWithOptions: 2usize];
    if intent == FOCUS_INTENT.load(std::sync::atomic::Ordering::Acquire) {
        if let Some(window) = element {
            ffi::AXUIElementPerformAction(window.0, ffi::string("AXRaise").0);
        }
    }
}
