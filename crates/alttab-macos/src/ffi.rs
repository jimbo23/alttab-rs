//! Narrow C boundaries. Copy/Create results own one retain; Get results borrow.
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::ffi::{c_char, c_void, CString};
use std::ptr;
use std::sync::LazyLock;

pub type Ref = *const c_void;
pub type MutRef = *mut c_void;
pub type AxCallback = unsafe extern "C" fn(Ref, Ref, Ref, MutRef);
pub type EventCallback = unsafe extern "C" fn(MutRef, u32, MutRef, MutRef) -> MutRef;

#[repr(C)]
pub struct SourceContext {
    pub version: isize,
    pub info: MutRef,
    pub retain: Ref,
    pub release: Ref,
    pub description: Ref,
    pub equal: Ref,
    pub hash: Ref,
    pub schedule: Ref,
    pub cancel: Ref,
    pub perform: Option<unsafe extern "C" fn(MutRef)>,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub fn AXIsProcessTrusted() -> bool;
    pub fn AXIsProcessTrustedWithOptions(options: Ref) -> bool;
    pub fn AXUIElementCreateApplication(pid: i32) -> Ref;
    pub fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, result: *mut Ref) -> i32;
    pub fn AXUIElementSetAttributeValue(element: Ref, attribute: Ref, value: Ref) -> i32;
    pub fn AXUIElementPerformAction(element: Ref, action: Ref) -> i32;
    pub fn AXUIElementSetMessagingTimeout(element: Ref, timeout: f32) -> i32;
    pub fn AXValueGetValue(value: Ref, kind: u32, output: MutRef) -> bool;
    pub fn AXObserverCreate(pid: i32, callback: AxCallback, observer: *mut Ref) -> i32;
    pub fn AXObserverAddNotification(
        observer: Ref,
        element: Ref,
        notification: Ref,
        context: MutRef,
    ) -> i32;
    pub fn AXObserverGetRunLoopSource(observer: Ref) -> Ref;
    pub fn CGWindowListCopyWindowInfo(options: u32, relative_to: u32) -> Ref;
    pub fn CGWindowListCreateImage(
        bounds: NSRect,
        options: u32,
        window: u32,
        image_options: u32,
    ) -> Ref;
    pub fn CGPreflightScreenCaptureAccess() -> bool;
    pub fn CGRequestScreenCaptureAccess() -> bool;
    pub fn CGImageGetWidth(image: Ref) -> usize;
    pub fn CGDisplayScreenSize(display: u32) -> NSSize;
    pub fn CGImageGetHeight(image: Ref) -> usize;
    pub fn CGImageGetBytesPerRow(image: Ref) -> usize;
    pub fn CGContextClearRect(context: MutRef, rect: NSRect);
    pub fn CGColorSpaceCreateDeviceRGB() -> Ref;
    pub fn CGBitmapContextCreate(
        data: MutRef,
        width: usize,
        height: usize,
        bits: usize,
        row: usize,
        space: Ref,
        info: u32,
    ) -> MutRef;
    pub fn CGContextDrawImage(context: MutRef, rect: NSRect, image: Ref);
    pub fn CGBitmapContextCreateImage(context: MutRef) -> Ref;
    pub fn CGEventTapCreate(
        location: u32,
        placement: u32,
        options: u32,
        mask: u64,
        callback: EventCallback,
        user: MutRef,
    ) -> MutRef;
    pub fn CGEventTapEnable(tap: MutRef, enable: bool);
    pub fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
    pub fn CGEventGetFlags(event: Ref) -> u64;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub fn CFRetain(value: Ref) -> Ref;
    pub fn CFRelease(value: Ref);
    pub fn CFStringCreateWithCString(allocator: Ref, string: *const c_char, encoding: u32) -> Ref;
    pub fn CFStringGetCString(value: Ref, buffer: *mut c_char, size: isize, encoding: u32) -> bool;
    pub fn CFStringGetLength(value: Ref) -> isize;
    pub fn CFArrayGetCount(value: Ref) -> isize;
    pub fn CFArrayGetValueAtIndex(value: Ref, index: isize) -> Ref;
    pub fn CFDictionaryGetValue(value: Ref, key: Ref) -> Ref;
    pub fn CFNumberGetValue(value: Ref, kind: i32, output: MutRef) -> bool;
    pub fn CFBooleanGetValue(value: Ref) -> bool;
    pub fn CFRunLoopGetCurrent() -> Ref;
    pub fn CFRunLoopSourceCreate(allocator: Ref, order: isize, context: *mut SourceContext) -> Ref;
    pub fn CFRunLoopSourceSignal(source: Ref);
    pub fn CFRunLoopWakeUp(loop_: Ref);
    pub fn CFRunLoopRun();
    pub fn CFRunLoopAddSource(loop_: Ref, source: Ref, mode: Ref);
    pub fn CFRunLoopRemoveSource(loop_: Ref, source: Ref, mode: Ref);
    pub fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
    pub static kCFRunLoopDefaultMode: Ref;
    pub static kCFBooleanTrue: Ref;
    pub static kCFBooleanFalse: Ref;
}
extern "C" {
    fn dlsym(handle: MutRef, name: *const c_char) -> MutRef;
    pub fn dlopen(name: *const c_char, flags: i32) -> MutRef;
}

pub struct Owned(pub Ref);
impl Owned {
    pub unsafe fn from_create(value: Ref) -> Option<Self> {
        if value.is_null() {
            None
        } else {
            Some(Self(value))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}

pub fn string(text: &str) -> Owned {
    let c = CString::new(text.replace('\0', "")).expect("NUL removed");
    unsafe {
        Owned(CFStringCreateWithCString(
            ptr::null(),
            c.as_ptr(),
            0x08000100,
        ))
    }
}
pub unsafe fn text(value: Ref) -> String {
    if value.is_null() {
        return String::new();
    }
    let mut bytes = vec![
        0u8;
        (CFStringGetLength(value) as usize)
            .saturating_mul(4)
            .saturating_add(1)
    ];
    if !CFStringGetCString(
        value,
        bytes.as_mut_ptr().cast(),
        bytes.len() as isize,
        0x08000100,
    ) {
        return String::new();
    }
    let length = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..length]).into_owned()
}
pub unsafe fn attribute(element: Ref, name: &str) -> Option<Owned> {
    let mut value = ptr::null();
    (AXUIElementCopyAttributeValue(element, string(name).0, &mut value) == 0)
        .then(|| Owned::from_create(value))
        .flatten()
}
pub unsafe fn dictionary(value: Ref, key: &str) -> Ref {
    CFDictionaryGetValue(value, string(key).0)
}
pub unsafe fn number(value: Ref) -> i64 {
    let mut result = 0i64;
    if !value.is_null() {
        CFNumberGetValue(value, 4, (&mut result as *mut i64).cast());
    }
    result
}
// Private _AXUIElementGetWindow bridge: upstream AXUIElement.swift:216-220.
// Absence disables semantic ID matching, not public CG-window switching.
fn window_symbol() -> usize {
    static SYMBOL: LazyLock<usize> = LazyLock::new(|| unsafe {
        dlsym((-2isize) as MutRef, c"_AXUIElementGetWindow".as_ptr()) as usize
    });
    *SYMBOL
}
pub fn window_mapping_available() -> bool {
    window_symbol() != 0
}
pub unsafe fn window_id(element: Ref) -> Option<u32> {
    type GetId = unsafe extern "C" fn(Ref, *mut u32) -> i32;
    let symbol = window_symbol();
    if symbol == 0 {
        return None;
    }
    let get: GetId = std::mem::transmute(symbol);
    let mut id = 0;
    (get(element, &mut id) == 0 && id != 0).then_some(id)
}
pub fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

// Optional topology ABI follows SkyLight.framework.swift:37-85. Unknown is not
// an empty membership list when private symbols are unavailable.
pub unsafe fn spaces(window: Option<u32>) -> Option<Vec<u64>> {
    use objc2::{class, msg_send, runtime::AnyObject};
    type Connection = unsafe extern "C" fn() -> u32;
    type WindowSpaces = unsafe extern "C" fn(u32, isize, Ref) -> Ref;
    type Displays = unsafe extern "C" fn(u32) -> Ref;
    static LIBRARY: LazyLock<usize> = LazyLock::new(|| unsafe {
        dlopen(
            c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(),
            1,
        ) as usize
    });
    let _ = *LIBRARY;
    static CONNECTION: LazyLock<usize> = LazyLock::new(|| unsafe {
        dlsym((-2isize) as MutRef, c"CGSMainConnectionID".as_ptr()) as usize
    });
    if *CONNECTION == 0 {
        return None;
    }
    let connection: Connection = std::mem::transmute(*CONNECTION);
    if let Some(window) = window {
        static SYMBOL: LazyLock<usize> = LazyLock::new(|| unsafe {
            dlsym((-2isize) as MutRef, c"CGSCopySpacesForWindows".as_ptr()) as usize
        });
        if *SYMBOL == 0 {
            return None;
        }
        let copy: WindowSpaces = std::mem::transmute(*SYMBOL);
        let number: *mut AnyObject = msg_send![class!(NSNumber), numberWithUnsignedInt: window];
        let array: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: number];
        let result = Owned::from_create(copy(connection(), 7, array.cast()))?;
        return Some(
            (0..CFArrayGetCount(result.0))
                .map(|i| self::number(CFArrayGetValueAtIndex(result.0, i)) as u64)
                .collect(),
        );
    }
    static SYMBOL: LazyLock<usize> = LazyLock::new(|| unsafe {
        dlsym((-2isize) as MutRef, c"CGSCopyManagedDisplaySpaces".as_ptr()) as usize
    });
    if *SYMBOL == 0 {
        return None;
    }
    let copy: Displays = std::mem::transmute(*SYMBOL);
    let result = Owned::from_create(copy(connection()))?;
    let mut spaces = Vec::new();
    for i in 0..CFArrayGetCount(result.0) {
        let display = CFArrayGetValueAtIndex(result.0, i);
        let current = dictionary(display, "Current Space");
        if !current.is_null() {
            let id = number(dictionary(current, "id64")) as u64;
            if id != 0 {
                spaces.push(id);
            }
        }
    }
    Some(spaces)
}
