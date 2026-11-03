use crate::{
    app::{self, Event},
    ffi,
};
use alttab_core::{ItemKey, Layout};
use objc2::{
    class, define_class, msg_send,
    rc::{Allocated, Retained},
    runtime::AnyObject,
    sel, ClassType, MainThreadOnly,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSRect, NSString};
use std::collections::HashMap;

// All objects in this module stay on the AppKit main thread. Actions enqueue
// work instead of re-entering the coordinator while AppKit is dispatching it.
// Borderless nonactivating panels need an explicit key-window override for
// native text input; see upstream TilesPanel.swift:3-4,26-34.
define_class!(
    #[unsafe(super = objc2_app_kit::NSPanel)]
    #[thread_kind = MainThreadOnly]
    struct SwitcherPanel;
    unsafe impl NSObjectProtocol for SwitcherPanel {}
    impl SwitcherPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self)->bool{true}
    }
);
define_class!(
    #[unsafe(super = objc2_app_kit::NSView)]
    #[thread_kind = MainThreadOnly]
    struct TileDocument;
    unsafe impl NSObjectProtocol for TileDocument {}
    impl TileDocument {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { false }
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _: Option<&AnyObject>) -> bool { true }
        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            objc2::rc::autoreleasepool(|_| unsafe {
                let _: () = msg_send![super(self), updateTrackingAreas];
                // The view retains its tracking areas. Remove only our own, in
                // reverse order, before replacing the visible-rect tracker.
                let areas: *mut AnyObject = msg_send![self, trackingAreas];
                let count: usize = msg_send![areas, count];
                for index in (0..count).rev() {
                    let area: *mut AnyObject = msg_send![areas, objectAtIndex: index];
                    let owner: *mut AnyObject = msg_send![area, owner];
                    if owner == (self as *const Self).cast_mut().cast() {
                        let _: () = msg_send![self, removeTrackingArea: area];
                    }
                }
                let area: Allocated<AnyObject> = msg_send![class!(NSTrackingArea), alloc];
                let area: Option<Retained<AnyObject>> = msg_send![area,
                    initWithRect: ffi::rect(0.0, 0.0, 0.0, 0.0),
                    options: (0x01usize | 0x02 | 0x80 | 0x200),
                    owner: self, userInfo: std::ptr::null::<AnyObject>()];
                if let Some(area) = area {
                    let _: () = msg_send![self, addTrackingArea: &*area];
                }
            });
        }
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &AnyObject) { self.post_pointer(event); }
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, event: &AnyObject) { self.post_pointer(event); }
        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _: &AnyObject) { app::post(Event::PointerExited); }
    }
);
impl TileDocument {
    fn post_pointer(&self, event: &AnyObject) {
        objc2::rc::autoreleasepool(|_| unsafe {
            let location: objc2_foundation::NSPoint = msg_send![event, locationInWindow];
            let point: objc2_foundation::NSPoint =
                msg_send![self, convertPoint: location, fromView: std::ptr::null::<AnyObject>()];
            let bounds: NSRect = msg_send![self, bounds];
            let screen: objc2_foundation::NSPoint = msg_send![class!(NSEvent), mouseLocation];
            app::post(Event::Pointer {
                x: point.x,
                y_from_top: bounds.size.height - point.y,
                screen_x: screen.x,
                screen_y: screen.y,
            });
        });
    }
}
define_class!(
    #[unsafe(super = objc2_app_kit::NSButton)]
    #[thread_kind = MainThreadOnly]
    struct TileButton;
    unsafe impl NSObjectProtocol for TileButton {}
    impl TileButton {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _: Option<&AnyObject>) -> bool { true }
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool { false }
    }
);
define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    pub struct Actions;
    unsafe impl NSObjectProtocol for Actions {}
    impl Actions {
        #[unsafe(method(preferences:))]
        fn preferences(&self, _: &AnyObject) { app::post(Event::Preferences); }
        #[unsafe(method(permissions:))]
        fn permissions(&self, _: &AnyObject) { app::post(Event::Permissions); }
        #[unsafe(method(quit:))]
        fn quit(&self, _: &AnyObject) { app::post(Event::Quit); }
        #[unsafe(method(dismissError:))]
        fn dismiss_error(&self,_:&AnyObject){app::post(Event::DismissError);}
        #[unsafe(method(changed:))]
        fn changed(&self, sender: &AnyObject) {
            objc2::rc::autoreleasepool(|_|unsafe { let tag: isize = msg_send![sender, tag]; app::post(Event::Setting(tag)); });
        }
        #[unsafe(method(tile:))]
        fn tile(&self, sender: &AnyObject) {
            objc2::rc::autoreleasepool(|_|unsafe { let tag: isize = msg_send![sender, tag]; app::post(Event::Click(tag as usize)); });
        }
        #[unsafe(method(workspace:))]
        fn workspace(&self, _: &AnyObject) { app::post(Event::Workspace); }
        // Field-editor commands arrive after the input method has consumed IME
        // composition keys, unlike a global tap's raw Return/Escape events.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn search_command(&self,_:&AnyObject,_:&AnyObject,command:objc2::runtime::Sel)->bool{
            objc2::rc::autoreleasepool(|_|unsafe{
                let application:*mut AnyObject=msg_send![class!(NSApplication),sharedApplication];
                let event:*mut AnyObject=msg_send![application,currentEvent];
                let repeat=if event.is_null(){false}else{msg_send![event,isARepeat]};
                let flags:u64=if event.is_null(){0}else{msg_send![event,modifierFlags]};
                let hold=crate::input::hold_is_down(flags);
                let key=if command==sel!(insertNewline:)||command==sel!(insertNewlineIgnoringFieldEditor:){36}else if command==sel!(cancelOperation:){53}
                    else if command==sel!(moveUp:)||(hold&&(command==sel!(moveToBeginningOfParagraph:)||command==sel!(moveToBeginningOfDocument:))){126}
                    else if command==sel!(moveDown:)||(hold&&(command==sel!(moveToEndOfParagraph:)||command==sel!(moveToEndOfDocument:))){125}
                    else if command==sel!(moveLeft:)||(hold&&(command==sel!(moveWordLeft:)||command==sel!(moveToBeginningOfLine:))){123}
                    else if command==sel!(moveRight:)||(hold&&(command==sel!(moveWordRight:)||command==sel!(moveToEndOfLine:))){124}
                    else{return command==sel!(insertTab:)||command==sel!(insertBacktab:);};
                app::post(Event::SearchKey{key,repeat});true
            })
        }
        #[unsafe(method(controlTextDidChange:))]
        fn text_changed(&self,notice:&AnyObject){
            objc2::rc::autoreleasepool(|_|unsafe{
                let field:*mut AnyObject=msg_send![notice,object];
                let text:*const NSString=msg_send![field,stringValue];
                if !text.is_null(){app::post(Event::Search((*text).to_string()));}
            });
        }
        #[unsafe(method(viewport:))]
        fn viewport(&self,_:&AnyObject){app::post(Event::Viewport);}
    }
);
impl Actions {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        use objc2::MainThreadOnly;
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}
#[derive(Clone, Copy)]
pub struct Screen {
    pub rect: alttab_core::Rect,
    pub quartz: alttab_core::Rect,
    pub scale: f64,
    pub id: alttab_core::DisplayId,
    pub physical_width_mm: Option<f64>,
}
struct Tile {
    button: Retained<AnyObject>,
    picture: Retained<AnyObject>,
    label: Retained<AnyObject>,
    icon: Retained<AnyObject>,
    picture_bounds: NSRect,
    fallback_icon_frame: NSRect,
    // Thumbnail tiles put a small icon left of the title once an image exists;
    // without one the large icon is centred, so the title centres under it.
    thumbnails: bool,
    footer_label_frame: NSRect,
    centred_label_frame: NSRect,
}
impl Tile {
    unsafe fn set_picture(&self, image: Option<&crate::capture::Image>) {
        let layer: *mut AnyObject = msg_send![&self.picture, layer];
        if let Some(image) = image {
            let fitted = alttab_core::layout::aspect_fit(
                alttab_core::Size {
                    width: ffi::CGImageGetWidth(image.0 as ffi::Ref) as f64,
                    height: ffi::CGImageGetHeight(image.0 as ffi::Ref) as f64,
                },
                alttab_core::Size {
                    width: self.picture_bounds.size.width,
                    height: self.picture_bounds.size.height,
                },
                true,
            );
            let _: () = msg_send![&self.picture, setFrame: ffi::rect(
                self.picture_bounds.origin.x + (self.picture_bounds.size.width - fitted.width) / 2.0,
                self.picture_bounds.origin.y, fitted.width, fitted.height)];
        }
        let _: () = msg_send![layer, setContents: image.map(|i| i.0 as *const AnyObject).unwrap_or(std::ptr::null())];
        let _: () = msg_send![&self.picture, setHidden: image.is_none()];
        let icon_frame = if image.is_some() {
            ffi::rect(8.0, 10.0, 20.0, 20.0)
        } else {
            self.fallback_icon_frame
        };
        let _: () = msg_send![&self.icon, setFrame: icon_frame];
        if self.thumbnails {
            let footer = image.is_some();
            let _: () = msg_send![&self.label, setAlignment: if footer { 0isize } else { 1isize }];
            let _: () = msg_send![&self.label, setFrame: if footer { self.footer_label_frame } else { self.centred_label_frame }];
        }
    }
}
pub struct Ui {
    pub actions: Retained<Actions>,
    panel: Retained<AnyObject>,
    document: Retained<AnyObject>,
    scroll: Retained<AnyObject>,
    search: Retained<AnyObject>,
    advanced: Option<Retained<AnyObject>>,
    editor: Option<Retained<AnyObject>>,
    tiles: Vec<Tile>,
    pub status: Retained<AnyObject>,
    settings: Option<Retained<AnyObject>>,
    permission_window: Option<Retained<AnyObject>>,
    error_window: Option<Retained<AnyObject>>,
    pub controls: HashMap<isize, Retained<AnyObject>>,
    icons: HashMap<i32, Retained<AnyObject>>,
    truncation: isize,
    icon_style: bool,
}
// Tile children stay accessible but defer hit testing to their containing button.
unsafe fn passive(
    class: &objc2::runtime::AnyClass,
    name: &std::ffi::CStr,
) -> &'static objc2::runtime::AnyClass {
    if let Some(class) = objc2::runtime::AnyClass::get(name) {
        return class;
    }
    extern "C" fn hit_test(
        _: &AnyObject,
        _: objc2::runtime::Sel,
        _: objc2_foundation::NSPoint,
    ) -> *mut AnyObject {
        std::ptr::null_mut()
    }
    let mut builder =
        objc2::runtime::ClassBuilder::new(name, class).expect("unique passive view class");
    builder.add_method(sel!(hitTest:), hit_test as extern "C" fn(_, _, _) -> _);
    builder.register()
}
/// Panel corner radius. Tiles use 10 pt and thumbnails 5 pt, so nested corners
/// shrink with their inset instead of competing with the outer shape.
const PANEL_RADIUS: f64 = 19.0;
/// A stretchable rounded-rect alpha mask (fixed corners, stretched centre).
unsafe fn rounded_mask(radius: f64) -> Retained<AnyObject> {
    let side = radius * 2.0 + 1.0;
    let size = objc2_foundation::NSSize::new(side, side);
    let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
    let image: Retained<AnyObject> = msg_send![image, initWithSize: size];
    let _: () = msg_send![&image, lockFocus];
    let black: *mut AnyObject = msg_send![class!(NSColor), blackColor];
    let _: () = msg_send![black, set];
    let path: *mut AnyObject = msg_send![class!(NSBezierPath), bezierPathWithRoundedRect: ffi::rect(0.0, 0.0, side, side), xRadius: radius, yRadius: radius];
    let _: () = msg_send![path, fill];
    let _: () = msg_send![&image, unlockFocus];
    let insets = objc2_foundation::NSEdgeInsets {
        top: radius,
        left: radius,
        bottom: radius,
        right: radius,
    };
    let _: () = msg_send![&image, setCapInsets: insets];
    let _: () = msg_send![&image, setResizingMode: 1isize];
    image
}
unsafe fn new_view(class: &objc2::runtime::AnyClass, frame: NSRect) -> Retained<AnyObject> {
    let allocated: Allocated<AnyObject> = msg_send![class, alloc];
    msg_send![allocated, initWithFrame: frame]
}
unsafe fn label(text: &str, frame: NSRect) -> Retained<AnyObject> {
    let class = passive(class!(NSTextField), c"AlttabPassiveLabel");
    let label = new_view(class, frame);
    let _: () = msg_send![&label,setStringValue:&*NSString::from_str(text)];
    let _: () = msg_send![&label,setEditable:false];
    let _: () = msg_send![&label,setSelectable:false];
    let _: () = msg_send![&label,setBordered:false];
    let _: () = msg_send![&label,setDrawsBackground:false];
    let _: () = msg_send![&label, setFrame: frame];
    label
}
unsafe fn button(
    text: &str,
    frame: NSRect,
    target: &Actions,
    action: objc2::runtime::Sel,
) -> Retained<AnyObject> {
    let button: Retained<AnyObject> = msg_send![class!(NSButton), buttonWithTitle: &*NSString::from_str(text), target: target, action: action];
    let _: () = msg_send![&button, setFrame: frame];
    button
}
unsafe fn window(title: &str, width: f64, height: f64, style: usize) -> Retained<AnyObject> {
    let window: Allocated<AnyObject> = msg_send![class!(NSWindow), alloc];
    let window: Retained<AnyObject> = msg_send![window, initWithContentRect: ffi::rect(0.0, 0.0, width, height), styleMask: style, backing: 2usize, defer: false];
    let _: () = msg_send![&window, setReleasedWhenClosed: false];
    let _: () = msg_send![&window, setTitle: &*NSString::from_str(title)];
    let _: () = msg_send![&window, center];
    window
}
impl Ui {
    pub fn new(mtm: MainThreadMarker) -> Self {
        unsafe {
            let actions = Actions::new(mtm);
            let panel: Allocated<AnyObject> = msg_send![SwitcherPanel::class(), alloc];
            let panel: Retained<AnyObject> = msg_send![panel, initWithContentRect: ffi::rect(0.0,0.0,500.0,300.0), styleMask: 128usize, backing: 2usize, defer: false];
            let _: () = msg_send![&panel, setReleasedWhenClosed: false];
            let _: () = msg_send![&panel, setLevel: 101isize];
            let _: () = msg_send![&panel, setCollectionBehavior: (1usize | 256usize)];
            let _: () = msg_send![&panel, setHidesOnDeactivate: false];
            let _: () = msg_send![&panel, setOpaque: false];
            let _: () = msg_send![&panel, setAcceptsMouseMovedEvents: true];
            let _: () = msg_send![&panel, setHasShadow: true];
            let color: *mut AnyObject = msg_send![class!(NSColor), clearColor];
            let _: () = msg_send![&panel, setBackgroundColor: color];
            let effect = new_view(
                class!(NSVisualEffectView),
                ffi::rect(0.0, 0.0, 500.0, 300.0),
            );
            let _: () = msg_send![&effect, setMaterial: 13isize];
            let _: () = msg_send![&effect, setBlendingMode: 0isize];
            let _: () = msg_send![&effect, setState: 1isize];
            let _: () = msg_send![&effect, setWantsLayer: true];
            let effect_layer: *mut AnyObject = msg_send![&effect, layer];
            let _: () = msg_send![effect_layer, setCornerRadius: PANEL_RADIUS];
            let _: () = msg_send![effect_layer, setMasksToBounds: true];
            // NSVisualEffectView's behind-window blur is clipped reliably only by
            // its mask image; the layer radius alone can leave square blur corners.
            let _: () = msg_send![&effect, setMaskImage: &*rounded_mask(PANEL_RADIUS)];
            let scroll = new_view(class!(NSScrollView), ffi::rect(0.0, 0.0, 500.0, 300.0));
            let _: () = msg_send![&scroll, setDrawsBackground: false];
            let _: () = msg_send![&scroll, setHasVerticalScroller: true];
            let clip: *mut AnyObject = msg_send![&scroll, contentView];
            let _: () = msg_send![clip,setPostsBoundsChangedNotifications:true];
            let notifications: *mut AnyObject =
                msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![notifications,addObserver:&*actions,selector:sel!(viewport:),name:&*NSString::from_str("NSViewBoundsDidChangeNotification"),object:clip];
            let document = new_view(TileDocument::class(), ffi::rect(0.0, 0.0, 500.0, 300.0));
            let _: () = msg_send![&scroll, setDocumentView: &*document];
            let _: () = msg_send![&effect, addSubview: &*scroll];
            let search = new_view(class!(NSSearchField), ffi::rect(12.0, 260.0, 476.0, 28.0));
            let _: () = msg_send![&search,setDelegate:&*actions];
            let _: () = msg_send![&search,setHidden:true];
            let _: () = msg_send![&effect,addSubview:&*search];
            let _: () = msg_send![&panel, setContentView: &*effect];
            let bar: *mut AnyObject = msg_send![class!(NSStatusBar), systemStatusBar];
            let status: Retained<AnyObject> = msg_send![bar, statusItemWithLength: -1.0f64];
            let status_button: *mut AnyObject = msg_send![&status, button];
            let _: () = msg_send![status_button, setTitle: &*NSString::from_str("⇥")];
            let menu: Retained<AnyObject> = msg_send![class!(NSMenu), new];
            for (title, action) in [
                ("Preferences…", sel!(preferences:)),
                ("Permissions…", sel!(permissions:)),
                ("Quit alttab-rs", sel!(quit:)),
            ] {
                let item: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
                let item: Retained<AnyObject> = msg_send![item, initWithTitle: &*NSString::from_str(title), action: action, keyEquivalent: &*NSString::from_str("")];
                let _: () = msg_send![&item, setTarget: &*actions];
                let _: () = msg_send![&menu, addItem: &*item];
            }
            let _: () = msg_send![&status, setMenu: &*menu];
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let center: *mut AnyObject = msg_send![workspace, notificationCenter];
            for name in [
                "NSWorkspaceDidLaunchApplicationNotification",
                "NSWorkspaceDidTerminateApplicationNotification",
                "NSWorkspaceDidActivateApplicationNotification",
                "NSWorkspaceDidHideApplicationNotification",
                "NSWorkspaceDidUnhideApplicationNotification",
                "NSWorkspaceActiveSpaceDidChangeNotification",
            ] {
                let _: () = msg_send![center, addObserver: &*actions, selector: sel!(workspace:), name: &*NSString::from_str(name), object: std::ptr::null::<AnyObject>()];
            }
            let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
            for name in [
                "NSApplicationDidBecomeActiveNotification",
                "NSApplicationDidChangeScreenParametersNotification",
            ] {
                let _: () = msg_send![center, addObserver: &*actions, selector: sel!(workspace:), name: &*NSString::from_str(name), object: std::ptr::null::<AnyObject>()];
            }
            let distributed: *mut AnyObject =
                msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
            let _: () = msg_send![distributed,addObserver:&*actions,selector:sel!(workspace:),name:&*NSString::from_str("com.apple.tcc.access.changed"),object:std::ptr::null::<AnyObject>()];
            Self {
                actions,
                panel,
                document,
                scroll,
                search,
                advanced: None,
                editor: None,
                tiles: Vec::new(),
                status,
                settings: None,
                permission_window: None,
                error_window: None,
                controls: HashMap::new(),
                icons: HashMap::new(),
                truncation: 4,
                icon_style: false,
            }
        }
    }
    pub fn screens(&self) -> Vec<Screen> {
        unsafe {
            let array: *mut AnyObject = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![array, count];
            if count == 0 {
                return Vec::new();
            }
            let first: *mut AnyObject = msg_send![array,objectAtIndex:0usize];
            let primary: NSRect = msg_send![first, frame];
            (0..count)
                .map(|index| {
                    let screen: *mut AnyObject = msg_send![array,objectAtIndex:index];
                    let full: NSRect = msg_send![screen, frame];
                    let visible: NSRect = msg_send![screen, visibleFrame];
                    let scale: f64 = msg_send![screen, backingScaleFactor];
                    let description: *mut AnyObject = msg_send![screen, deviceDescription];
                    let number: *mut AnyObject =
                        msg_send![description,objectForKey:&*NSString::from_str("NSScreenNumber")];
                    let id: u32 = msg_send![number, unsignedIntValue];
                    let physical = ffi::CGDisplayScreenSize(id);
                    let physical_width_mm = if physical.width > 0.0
                        && physical.height > 0.0
                        && ((physical.width / physical.height)
                            / (full.size.width / full.size.height)
                            - 1.0)
                            .abs()
                            < 0.1
                    {
                        Some(physical.width)
                    } else {
                        None
                    };
                    Screen {
                        rect: alttab_core::Rect {
                            x: visible.origin.x,
                            y: visible.origin.y,
                            width: visible.size.width,
                            height: visible.size.height,
                        },
                        quartz: alttab_core::Rect {
                            x: full.origin.x,
                            y: primary.size.height - full.origin.y - full.size.height,
                            width: full.size.width,
                            height: full.size.height,
                        },
                        scale,
                        id: alttab_core::DisplayId(id),
                        physical_width_mm,
                    }
                })
                .collect()
        }
    }
    pub fn pointer_quartz(&self) -> (f64, f64) {
        unsafe {
            let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            let height = if count == 0 {
                0.0
            } else {
                let first: *mut AnyObject = msg_send![screens,objectAtIndex:0usize];
                let frame: NSRect = msg_send![first, frame];
                frame.size.height
            };
            let mouse: objc2_foundation::NSPoint = msg_send![class!(NSEvent), mouseLocation];
            (mouse.x, height - mouse.y)
        }
    }
    /// Pointer in AppKit screen coordinates, the frame `Event::Pointer` uses.
    pub fn pointer_cocoa(&self) -> (f64, f64) {
        unsafe {
            let mouse: objc2_foundation::NSPoint = msg_send![class!(NSEvent), mouseLocation];
            (mouse.x, mouse.y)
        }
    }
    pub fn screen(
        &self,
        placement: alttab_core::ScreenPlacement,
        attended: Option<alttab_core::Rect>,
    ) -> Screen {
        unsafe {
            let screens = self.screens();
            let fallback = Screen {
                rect: alttab_core::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1280.0,
                    height: 800.0,
                },
                quartz: alttab_core::Rect::default(),
                scale: 1.0,
                id: alttab_core::DisplayId(0),
                physical_width_mm: None,
            };
            let first = screens.first().copied().unwrap_or(fallback);
            match placement {
                alttab_core::ScreenPlacement::Menubar => first,
                alttab_core::ScreenPlacement::Active => attended
                    .and_then(|r| {
                        screens
                            .iter()
                            .find(|s| s.quartz.contains(r.center_x(), r.center_y()))
                            .copied()
                    })
                    .unwrap_or(first),
                alttab_core::ScreenPlacement::Mouse => {
                    let mouse: objc2_foundation::NSPoint =
                        msg_send![class!(NSEvent), mouseLocation];
                    screens
                        .iter()
                        .find(|s| s.rect.contains(mouse.x, mouse.y))
                        .copied()
                        .unwrap_or(first)
                }
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        layout: &Layout,
        entries: &[(ItemKey, i32, String)],
        selected: Option<ItemKey>,
        images: &HashMap<ItemKey, crate::capture::Image>,
        style: alttab_core::Style,
        searching: bool,
        scroll: bool,
    ) {
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let thumbnails = style == alttab_core::Style::Thumbnails;
            let titles = style == alttab_core::Style::Titles;
            self.icon_style = !thumbnails && !titles;
            while self.tiles.len() < entries.len() {
                let tile = new_view(TileButton::class(), ffi::rect(0.0, 0.0, 1.0, 1.0));
                let _: () = msg_send![&tile, setTitle: &*NSString::from_str("")];
                let _: () = msg_send![&tile, setTarget: &*self.actions];
                let _: () = msg_send![&tile, setAction: sel!(tile:)];
                let _: () = msg_send![&tile, setFocusRingType: 1usize];
                let _: () = msg_send![&tile, setBordered: false];
                let _: () = msg_send![&tile, setWantsLayer: true];
                let tile_layer: *mut AnyObject = msg_send![&tile, layer];
                let _: () = msg_send![tile_layer, setCornerRadius: 10.0f64];
                // Keep NSButton's normal down/up action tracking, without its
                // cell drawing a second selection highlight over our layers.
                let cell: *mut AnyObject = msg_send![&tile, cell];
                let _: () = msg_send![cell, setHighlightsBy: 0usize];
                let picture = new_view(
                    passive(class!(NSView), c"AlttabPassivePicture"),
                    ffi::rect(0.0, 0.0, 1.0, 1.0),
                );
                let _: () = msg_send![&picture, setWantsLayer: true];
                let picture_layer: *mut AnyObject = msg_send![&picture, layer];
                let _: () =
                    msg_send![picture_layer, setContentsGravity: &*NSString::from_str("resize")];
                let _: () = msg_send![picture_layer, setCornerRadius: 5.0f64];
                let _: () = msg_send![picture_layer, setMasksToBounds: true];
                let _: () = msg_send![picture_layer, setBorderWidth: 0.5f64];
                let text = label("", ffi::rect(0.0, 0.0, 1.0, 1.0));
                let font: *mut AnyObject = msg_send![class!(NSFont), systemFontOfSize: 12.0f64];
                let _: () = msg_send![&text, setFont: font];
                let _: () = msg_send![&text, setMaximumNumberOfLines: 1isize];
                let _: () = msg_send![&text, setUsesSingleLineMode: true];
                let _: () = msg_send![&text,setLineBreakMode:self.truncation];
                let text_cell: *mut AnyObject = msg_send![&text, cell];
                let _: () = msg_send![text_cell, setTruncatesLastVisibleLine: true];
                let icon = new_view(
                    passive(class!(NSImageView), c"AlttabPassiveIcon"),
                    ffi::rect(0.0, 0.0, 24.0, 24.0),
                );
                let _: () = msg_send![&icon,setImageScaling:0usize];
                let _: () = msg_send![&tile,addSubview:&*icon];
                let _: () = msg_send![&tile, addSubview: &*picture];
                let _: () = msg_send![&tile, addSubview: &*text];
                let _: () = msg_send![&self.document, addSubview: &*tile];
                self.tiles.push(Tile {
                    button: tile,
                    picture,
                    label: text,
                    icon,
                    picture_bounds: ffi::rect(0.0, 0.0, 0.0, 0.0),
                    fallback_icon_frame: ffi::rect(0.0, 0.0, 0.0, 0.0),
                    thumbnails: false,
                    footer_label_frame: ffi::rect(0.0, 0.0, 0.0, 0.0),
                    centred_label_frame: ffi::rect(0.0, 0.0, 0.0, 0.0),
                });
            }
            let frame = ffi::rect(
                layout.panel.x,
                layout.panel.y,
                layout.panel.width,
                layout.panel.height,
            );
            let previous_frame: NSRect = msg_send![&self.panel, frame];
            if previous_frame != frame {
                let _: () = msg_send![&self.panel, setFrame: frame, display: false];
                let _: () = msg_send![&self.panel, invalidateShadow];
            }
            let _: () = msg_send![&self.scroll, setFrame: ffi::rect(0.0,0.0,layout.panel.width,(layout.panel.height-if searching{40.0}else{0.0}).max(1.0))];
            let _: () = msg_send![&self.search,setFrame:ffi::rect(12.0,(layout.panel.height-35.0).max(0.0),(layout.panel.width-24.0).max(1.0),28.0)];
            let _: () = msg_send![&self.search,setHidden:!searching];
            let _: () = msg_send![&self.document, setFrame: ffi::rect(0.0,0.0,layout.document_size.width,layout.document_size.height)];
            for (index, tile) in self.tiles.iter_mut().enumerate() {
                let _: () = msg_send![&tile.button, setHidden: index >= entries.len()];
                if index >= entries.len() {
                    let layer: *mut AnyObject = msg_send![&tile.picture, layer];
                    let _: () = msg_send![layer,setContents:std::ptr::null::<AnyObject>()];
                    let _: () = msg_send![&tile.icon,setImage:std::ptr::null::<AnyObject>()];
                    continue;
                }
                let (key, pid, title) = &entries[index];
                let r = layout.tiles[index];
                let _: () = msg_send![&tile.button, setFrame: ffi::rect(r.x,layout.document_size.height-r.y-r.height,r.width,r.height)];
                let _: () = msg_send![&tile.button, setTag: index as isize];
                let title = NSString::from_str(title);
                let _: () = msg_send![&tile.label, setStringValue: &*title];
                let _: () = msg_send![&tile.button, setAccessibilityLabel: &*title];
                let label_x = if self.icon_style { 8.0 } else { 36.0 };
                let label_y = if titles {
                    (r.height - 20.0) / 2.0
                } else {
                    10.0
                };
                let _: () = msg_send![&tile.label, setAlignment: if self.icon_style { 1isize } else { 0isize }];
                let _: () = msg_send![&tile.label, setFrame: ffi::rect(label_x, label_y, (r.width - label_x - 8.0).max(1.0), 20.0)];
                tile.thumbnails = thumbnails;
                tile.footer_label_frame = ffi::rect(36.0, label_y, (r.width - 44.0).max(1.0), 20.0);
                tile.centred_label_frame = ffi::rect(8.0, label_y, (r.width - 16.0).max(1.0), 20.0);
                tile.picture_bounds = ffi::rect(
                    8.0,
                    40.0,
                    (r.width - 16.0).max(0.0),
                    (r.height - 48.0).max(0.0),
                );
                let icon_size = if titles {
                    20.0
                } else {
                    (r.height - 40.0).min(r.width - 16.0).clamp(1.0, 128.0)
                };
                tile.fallback_icon_frame = ffi::rect(
                    if titles {
                        8.0
                    } else {
                        (r.width - icon_size) / 2.0
                    },
                    if titles {
                        (r.height - icon_size) / 2.0
                    } else {
                        30.0
                    },
                    icon_size,
                    icon_size,
                );
                tile.set_picture(if thumbnails { images.get(key) } else { None });
                let _: () = msg_send![&tile.icon,setImage:self.icons.get(pid).map(|icon|&**icon)];
            }
            self.selection(
                entries
                    .iter()
                    .position(|(key, _, _)| Some(*key) == selected),
                scroll,
            );
            let _: () = msg_send![class!(CATransaction), commit];
            self.show();
        }
    }
    fn show(&self) {
        unsafe {
            let visible: bool = msg_send![&self.panel, isVisible];
            if visible {
                return;
            }
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let reduce_motion: bool = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
            let _: () =
                msg_send![&self.panel, setAlphaValue: if reduce_motion { 1.0f64 } else { 0.0f64 }];
            let _: () = msg_send![&self.panel, orderFrontRegardless];
            if !reduce_motion {
                let _: () = msg_send![class!(NSAnimationContext), beginGrouping];
                let context: *mut AnyObject = msg_send![class!(NSAnimationContext), currentContext];
                let _: () = msg_send![context, setDuration: 0.12f64];
                let timing: *mut AnyObject = msg_send![class!(CAMediaTimingFunction), functionWithName: &*NSString::from_str("easeOut")];
                let _: () = msg_send![context, setTimingFunction: timing];
                let animator: *mut AnyObject = msg_send![&self.panel, animator];
                let _: () = msg_send![animator, setAlphaValue: 1.0f64];
                let _: () = msg_send![class!(NSAnimationContext), endGrouping];
            }
        }
    }
    pub fn start_search(&self) {
        unsafe {
            let _: () = msg_send![&self.search,setStringValue:&*NSString::from_str("")];
            let _: () = msg_send![&self.search,setHidden:false];
            let _: () = msg_send![&self.panel,makeKeyAndOrderFront:std::ptr::null::<AnyObject>()];
            let _: bool = msg_send![&self.panel,makeFirstResponder:&*self.search];
        }
    }
    pub fn end_search(&self) {
        unsafe {
            let _: () = msg_send![&self.search,setHidden:true];
            let _: bool = msg_send![&self.panel,makeFirstResponder:std::ptr::null::<AnyObject>()];
        }
    }
    pub fn retain_icons(&mut self, apps: &HashMap<i32, alttab_core::AppKey>) {
        if self.icons.keys().any(|pid| !apps.contains_key(pid)) {
            unsafe {
                for tile in &self.tiles {
                    let _: () = msg_send![&tile.icon,setImage:std::ptr::null::<AnyObject>()];
                }
            }
            self.icons.retain(|pid, _| apps.contains_key(pid));
        }
    }
    pub fn selection(&self, index: Option<usize>, scroll: bool) {
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let previous: Option<Retained<AnyObject>> =
                msg_send![class!(NSAppearance), currentAppearance];
            let appearance: *mut AnyObject = msg_send![&self.panel, effectiveAppearance];
            let _: () = msg_send![class!(NSAppearance), setCurrentAppearance: appearance];
            let accent: *mut AnyObject = msg_send![class!(NSColor), controlAccentColor];
            let fill: *mut AnyObject = msg_send![accent, colorWithAlphaComponent: 0.22f64];
            let ring: *mut AnyObject = msg_send![accent, colorWithAlphaComponent: 0.55f64];
            let clear: *mut AnyObject = msg_send![class!(NSColor), clearColor];
            let label: *mut AnyObject = msg_send![class!(NSColor), labelColor];
            let secondary: *mut AnyObject = msg_send![class!(NSColor), secondaryLabelColor];
            let hairline: *mut AnyObject = msg_send![label, colorWithAlphaComponent: 0.1f64];
            let fill_cg: *const objc2_core_graphics::CGColor = msg_send![fill, CGColor];
            let ring_cg: *const objc2_core_graphics::CGColor = msg_send![ring, CGColor];
            let clear_cg: *const objc2_core_graphics::CGColor = msg_send![clear, CGColor];
            let hairline_cg: *const objc2_core_graphics::CGColor = msg_send![hairline, CGColor];
            for (i, tile) in self.tiles.iter().enumerate() {
                let selected = Some(i) == index;
                let layer: *mut AnyObject = msg_send![&tile.button, layer];
                let _: () =
                    msg_send![layer, setBackgroundColor: if selected { fill_cg } else { clear_cg }];
                let _: () =
                    msg_send![layer, setBorderColor: if selected { ring_cg } else { clear_cg }];
                let _: () =
                    msg_send![layer, setBorderWidth: if selected { 1.0f64 } else { 0.0f64 }];
                let _: () =
                    msg_send![&tile.label, setTextColor: if selected { label } else { secondary }];
                let hide_label = if self.icon_style {
                    let icon: *mut AnyObject = msg_send![&tile.icon, image];
                    !selected || icon.is_null()
                } else {
                    false
                };
                let _: () = msg_send![&tile.label, setHidden: hide_label];
                let picture: *mut AnyObject = msg_send![&tile.picture, layer];
                let _: () = msg_send![picture, setBorderColor: hairline_cg];
            }
            let _: () = msg_send![class!(NSAppearance), setCurrentAppearance: previous.as_deref()];
            if scroll {
                if let Some(tile) = index.and_then(|i| self.tiles.get(i)) {
                    let frame: NSRect = msg_send![&tile.button, frame];
                    let _: bool = msg_send![&self.document, scrollRectToVisible: frame];
                }
            }
            let _: () = msg_send![class!(CATransaction), commit];
        }
    }
    pub fn viewport(&self) -> (f64, f64) {
        unsafe {
            let clip: *mut AnyObject = msg_send![&self.scroll, contentView];
            let bounds: NSRect = msg_send![clip, bounds];
            let frame: NSRect = msg_send![&self.document, frame];
            let top = frame.size.height - bounds.origin.y - bounds.size.height;
            (top, top + bounds.size.height)
        }
    }
    pub fn screenshot(&self, index: usize, image: Option<&crate::capture::Image>) {
        unsafe {
            if let Some(tile) = self.tiles.get(index) {
                let _: () = msg_send![class!(CATransaction), begin];
                let _: () = msg_send![class!(CATransaction), setDisableActions: true];
                tile.set_picture(image);
                let _: () = msg_send![class!(CATransaction), commit];
            }
        }
    }
    pub fn icon(&mut self, pid: i32, image: &crate::capture::Image) {
        unsafe {
            if self.icons.len() >= 32 && !self.icons.contains_key(&pid) {
                return;
            }
            self.remove_icon(pid);
            let allocated: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
            let icon: Retained<AnyObject> = msg_send![allocated,initWithCGImage:image.0 as *const objc2_core_graphics::CGImage,size:objc2_foundation::NSSize::new(128.0,128.0)];
            self.icons.insert(pid, icon);
        }
    }
    pub fn remove_icon(&mut self, pid: i32) {
        unsafe {
            if self.icons.contains_key(&pid) {
                for tile in &self.tiles {
                    let _: () = msg_send![&tile.icon,setImage:std::ptr::null::<AnyObject>()];
                }
                self.icons.remove(&pid);
            }
        }
    }
    pub fn appearance(
        &mut self,
        appearance: &alttab_core::Appearance,
        menubar: alttab_core::MenubarIcon,
    ) {
        unsafe {
            self.truncation = match appearance.title_truncation {
                alttab_core::Truncation::Start => 3,
                alttab_core::Truncation::Middle => 5,
                alttab_core::Truncation::End => 4,
            };
            for tile in &self.tiles {
                let _: () = msg_send![&tile.label,setLineBreakMode:self.truncation];
            }
            let native: *mut AnyObject = match appearance.theme {
                alttab_core::Theme::System => std::ptr::null_mut(),
                theme => {
                    msg_send![class!(NSAppearance),appearanceNamed:&*NSString::from_str(if theme==alttab_core::Theme::Dark{"NSAppearanceNameDarkAqua"}else{"NSAppearanceNameAqua"})]
                }
            };
            let _: () = msg_send![&self.panel,setAppearance:native];
            let button: *mut AnyObject = msg_send![&self.status, button];
            let image: Option<Retained<AnyObject>> = msg_send![class!(NSImage),imageWithSystemSymbolName:&*NSString::from_str(if menubar==alttab_core::MenubarIcon::Filled{"rectangle.on.rectangle.fill"}else{"rectangle.on.rectangle"}),accessibilityDescription:&*NSString::from_str("alttab-rs")];
            if let Some(image) = image {
                let _: () = msg_send![&image,setTemplate:true];
                let _: () = msg_send![button,setTitle:&*NSString::from_str("")];
                let _: () = msg_send![button,setImage:&*image];
            }
        }
    }
    pub fn hide(&mut self) {
        unsafe {
            let _: () = msg_send![&self.panel, orderOut: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![class!(NSAnimationContext), beginGrouping];
            let context: *mut AnyObject = msg_send![class!(NSAnimationContext), currentContext];
            let _: () = msg_send![context, setDuration: 0.0f64];
            let animator: *mut AnyObject = msg_send![&self.panel, animator];
            let _: () = msg_send![animator, setAlphaValue: 1.0f64];
            let _: () = msg_send![class!(NSAnimationContext), endGrouping];
            let _: () = msg_send![&self.panel, setAlphaValue: 1.0f64];
            for tile in &self.tiles {
                let layer: *mut AnyObject = msg_send![&tile.picture, layer];
                let _: () = msg_send![layer, setContents: std::ptr::null::<AnyObject>()];
                let _: () = msg_send![&tile.icon, setImage: std::ptr::null::<AnyObject>()];
            }
        }
    }
    pub fn error(&mut self, message: &str) {
        unsafe {
            self.dismiss_error();
            let window = window("alttab-rs", 540.0, 300.0, 1 | 2);
            let content: *mut AnyObject = msg_send![&window, contentView];
            let scroll = new_view(class!(NSScrollView), ffi::rect(15.0, 65.0, 510.0, 220.0));
            let _: () = msg_send![&scroll,setHasVerticalScroller:true];
            let text = new_view(class!(NSTextView), ffi::rect(0.0, 0.0, 490.0, 220.0));
            let _: () = msg_send![&text,setRichText:false];
            let _: () = msg_send![&text,setEditable:false];
            let _: () = msg_send![&text,setVerticallyResizable:true];
            let _: () = msg_send![&text,setString:&*NSString::from_str(message)];
            let _: () = msg_send![&scroll,setDocumentView:&*text];
            let _: () = msg_send![content,addSubview:&*scroll];
            let ok = button(
                "OK",
                ffi::rect(405.0, 18.0, 120.0, 32.0),
                &self.actions,
                sel!(dismissError:),
            );
            let _: () = msg_send![content,addSubview:&*ok];
            let _: () = msg_send![&window,makeKeyAndOrderFront:std::ptr::null::<AnyObject>()];
            self.error_window = Some(window);
        }
    }
    pub fn dismiss_error(&mut self) {
        unsafe {
            if let Some(window) = self.error_window.take() {
                let _: () = msg_send![&window,orderOut:std::ptr::null::<AnyObject>()];
            }
        }
    }
    pub fn preferences(&mut self, config: &alttab_core::Config) {
        unsafe {
            if self.settings.is_none() {
                let window = window("alttab-rs Preferences", 520.0, 480.0, 1 | 2);
                let content: *mut AnyObject = msg_send![&window, contentView];
                let heading = label(
                    "Changes are saved immediately to config.toml.",
                    ffi::rect(25.0, 437.0, 470.0, 23.0),
                );
                let _: () = msg_send![content, addSubview: &*heading];
                for (tag, title, choices, y) in [
                    (1, "Hold modifier", vec!["Option", "Command"], 390.0),
                    (
                        2,
                        "Appearance",
                        vec!["Thumbnails", "App icons", "Titles"],
                        345.0,
                    ),
                    (
                        3,
                        "Minimized windows",
                        vec!["Show", "Hide", "Show at end"],
                        300.0,
                    ),
                    (
                        4,
                        "Hidden applications",
                        vec!["Show", "Hide", "Show at end"],
                        255.0,
                    ),
                    (5, "Fullscreen windows", vec!["Show", "Hide"], 210.0),
                ] {
                    let text = label(title, ffi::rect(25.0, y, 200.0, 25.0));
                    let _: () = msg_send![content, addSubview: &*text];
                    let popup = new_view(class!(NSPopUpButton), ffi::rect(235.0, y, 250.0, 28.0));
                    for choice in choices {
                        let _: () =
                            msg_send![&popup, addItemWithTitle: &*NSString::from_str(choice)];
                    }
                    let _: () = msg_send![&popup, setTag: tag];
                    let _: () = msg_send![&popup, setTarget: &*self.actions];
                    let _: () = msg_send![&popup, setAction: sel!(changed:)];
                    let _: () = msg_send![content, addSubview: &*popup];
                    self.controls.insert(tag, popup);
                }
                let login = button(
                    "Launch at login (macOS 13+)",
                    ffi::rect(25.0, 155.0, 450.0, 28.0),
                    &self.actions,
                    sel!(changed:),
                );
                let _: () = msg_send![&login, setButtonType: 3usize];
                let _: () = msg_send![&login, setTag: 6isize];
                let _: () = msg_send![content, addSubview: &*login];
                self.controls.insert(6, login);
                let advanced = button(
                    "Advanced configuration…",
                    ffi::rect(25.0, 108.0, 280.0, 30.0),
                    &self.actions,
                    sel!(changed:),
                );
                let _: () = msg_send![&advanced,setTag:7isize];
                let _: () = msg_send![content,addSubview:&*advanced];
                let note = label("Public AX focus may be limited across Spaces.\\nScreen Recording is optional; icons and titles always work.",ffi::rect(25.0,28.0,470.0,70.0));
                let _: () = msg_send![content, addSubview: &*note];
                self.settings = Some(window);
            }
            let _ = config;
            let window = self.settings.as_ref().unwrap();
            let _: () = msg_send![window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
            let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![application, activateIgnoringOtherApps: true];
        }
    }
    pub fn advanced(&mut self, config: &str) {
        unsafe {
            if self.advanced.is_none() {
                let window = window("alttab-rs Advanced Configuration", 720.0, 600.0, 1 | 2 | 8);
                let content: *mut AnyObject = msg_send![&window, contentView];
                let scroll = new_view(class!(NSScrollView), ffi::rect(15.0, 60.0, 690.0, 525.0));
                let _: () = msg_send![&scroll,setHasVerticalScroller:true];
                let _: () = msg_send![&scroll,setAutoresizingMask:18usize];
                let editor = new_view(class!(NSTextView), ffi::rect(0.0, 0.0, 670.0, 525.0));
                let _: () = msg_send![&editor,setRichText:false];
                let _: () = msg_send![&editor,setAutomaticQuoteSubstitutionEnabled:false];
                let _: () = msg_send![&editor,setAutomaticDashSubstitutionEnabled:false];
                let _: () = msg_send![&editor,setVerticallyResizable:true];
                let font: *mut AnyObject =
                    msg_send![class!(NSFont),monospacedSystemFontOfSize:12.0f64,weight:0.0f64];
                let _: () = msg_send![&editor,setFont:font];
                let _: () = msg_send![&scroll,setDocumentView:&*editor];
                let _: () = msg_send![content,addSubview:&*scroll];
                let save = button(
                    "Validate and Save",
                    ffi::rect(510.0, 15.0, 195.0, 32.0),
                    &self.actions,
                    sel!(changed:),
                );
                let _: () = msg_send![&save,setTag:8isize];
                let _: () = msg_send![content,addSubview:&*save];
                let note=label("All profiles, filters, exceptions and appearance settings use the documented TOML schema.",ffi::rect(15.0,17.0,490.0,30.0));
                let _: () = msg_send![content,addSubview:&*note];
                self.editor = Some(editor);
                self.advanced = Some(window);
            }
            if let Some(editor) = &self.editor {
                let _: () = msg_send![editor,setString:&*NSString::from_str(config)];
            }
            let _: () = msg_send![self.advanced.as_ref().unwrap(),makeKeyAndOrderFront:std::ptr::null::<AnyObject>()];
        }
    }
    pub fn configuration_text(&self) -> String {
        unsafe {
            let Some(editor) = &self.editor else {
                return String::new();
            };
            let text: *const NSString = msg_send![editor, string];
            if text.is_null() {
                String::new()
            } else {
                (*text).to_string()
            }
        }
    }
    pub fn control(&self, tag: isize) -> isize {
        unsafe {
            let control = &self.controls[&tag];
            if tag == 6 {
                msg_send![control, state]
            } else {
                msg_send![control, indexOfSelectedItem]
            }
        }
    }
    pub fn set_control(&self, tag: isize, index: isize) {
        unsafe {
            if let Some(control) = self.controls.get(&tag) {
                if tag == 6 {
                    let _: () = msg_send![control, setState: index];
                } else {
                    let _: () = msg_send![control, selectItemAtIndex: index];
                }
            }
        }
    }
    pub fn permissions(&mut self, trusted: bool, recording: bool, started: bool) {
        unsafe {
            if let Some(old) = self.permission_window.take() {
                let _: () = msg_send![&old, orderOut: std::ptr::null::<AnyObject>()];
            }
            let window = window(
                "alttab-rs Permissions",
                540.0,
                300.0,
                if started { 1 | 2 } else { 1 },
            );
            let content: *mut AnyObject = msg_send![&window, contentView];
            let text = label(&format!("Accessibility: {}\nScreen Recording: {}\nAccessibility is required to switch windows.\nScreen Recording is optional and only used for thumbnails.\nReturn from System Settings to recheck permission.",if trusted {"granted"} else {"required"},if recording {"granted"} else {"not granted"}),ffi::rect(25.0,115.0,490.0,160.0));
            let _: () = msg_send![content, addSubview: &*text];
            for (tag, title, x, width) in [
                (10, "Grant Accessibility", 20.0, 170.0),
                (11, "Grant Recording", 195.0, 150.0),
                (12, "Continue without thumbnails", 20.0, 270.0),
                (13, "Continue", 355.0, 155.0),
            ] {
                let y = if tag == 12 { 30.0 } else { 75.0 };
                let button = button(
                    title,
                    ffi::rect(x, y, width, 32.0),
                    &self.actions,
                    sel!(changed:),
                );
                let _: () = msg_send![&button, setTag: tag];
                let _: () = msg_send![content, addSubview: &*button];
            }
            let quit = button(
                "Quit",
                ffi::rect(370.0, 30.0, 140.0, 32.0),
                &self.actions,
                sel!(quit:),
            );
            let _: () = msg_send![content, addSubview: &*quit];
            let _: () = msg_send![&window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
            self.permission_window = Some(window);
            let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![application, activateIgnoringOtherApps: true];
        }
    }
    pub fn close_permissions(&mut self) {
        unsafe {
            if let Some(window) = self.permission_window.take() {
                let _: () = msg_send![&window, orderOut: std::ptr::null::<AnyObject>()];
            }
        }
    }
}
