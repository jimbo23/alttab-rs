use crate::{
    app::{self, Event},
    ffi::{self, Ref},
};
use alttab_core::ItemKey;
use block2::RcBlock;
use objc2::{
    class, msg_send,
    rc::{autoreleasepool, Allocated, Retained},
    runtime::{AnyClass, AnyObject},
};
use std::ptr;

// CGImage is immutable; its owned CF retain may cross worker/main boundaries.
// The integer carries only that retain, never an AppKit object or borrowed image.
pub struct Image(pub usize);
impl Image {
    pub fn bytes(&self) -> usize {
        unsafe {
            ffi::CGImageGetBytesPerRow(self.0 as Ref)
                .saturating_mul(ffi::CGImageGetHeight(self.0 as Ref))
        }
    }
}
impl Drop for Image {
    fn drop(&mut self) {
        unsafe { ffi::CFRelease(self.0 as Ref) }
    }
}
#[derive(Clone, Copy)]
pub struct Request {
    pub key: ItemKey,
    pub id: u32,
    pub generation: u64,
    pub token: u64,
    pub width: usize,
    pub height: usize,
    pub source_pixels: usize,
}
pub fn request(request: Request) {
    use std::sync::{
        mpsc::{self, SyncSender},
        LazyLock,
    };
    static WORKER: LazyLock<SyncSender<Request>> = LazyLock::new(|| {
        let (tx, rx) = mpsc::sync_channel::<Request>(1);
        std::thread::Builder::new().name("alttab screenshot".into()).spawn(move || {
            for request in rx {
                autoreleasepool(|_| unsafe {
                    if !ffi::CGPreflightScreenCaptureAccess(){finish(request,None);return;}
                    static LIBRARY:LazyLock<usize>=LazyLock::new(||unsafe{ffi::dlopen(c"/System/Library/Frameworks/ScreenCaptureKit.framework/ScreenCaptureKit".as_ptr(),1) as usize});
                    let _=*LIBRARY;
                    if AnyClass::get(c"SCScreenshotManager").is_some(){screen_capture(request);}else{fallback(request);}
                });
            }
        }).expect("create capture worker");
        tx
    });
    if WORKER.try_send(request).is_err() {
        finish(request, None);
    }
}
fn guarded(request: Request, work: impl FnOnce()) {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| autoreleasepool(|_| work())));
    if result.is_err() {
        finish(request, None);
    }
}
unsafe fn screen_capture(request: Request) {
    let completion = RcBlock::new(move |content: *mut AnyObject, _: *mut AnyObject| {
        guarded(request, || unsafe {
            if content.is_null() {
                fallback(request);
                return;
            }
            let windows: *mut AnyObject = msg_send![content, windows];
            let count: usize = msg_send![windows, count];
            let mut target = ptr::null_mut::<AnyObject>();
            for i in 0..count {
                let window: *mut AnyObject = msg_send![windows, objectAtIndex: i];
                let id: u32 = msg_send![window, windowID];
                if id == request.id {
                    target = window;
                    break;
                }
            }
            if target.is_null() {
                fallback(request);
                return;
            }
            let filter: Allocated<AnyObject> = msg_send![class!(SCContentFilter), alloc];
            let filter: Retained<AnyObject> =
                msg_send![filter, initWithDesktopIndependentWindow: target];
            let config: Retained<AnyObject> = msg_send![class!(SCStreamConfiguration), new];
            let _: () = msg_send![&config, setWidth: request.width];
            let _: () = msg_send![&config, setHeight: request.height];
            let _: () = msg_send![&config, setShowsCursor: false];
            let completion = RcBlock::new(move |image: Ref, _: *mut AnyObject| {
                guarded(request, || {
                    finish(request, resize(image, request.width, request.height));
                });
            });
            let _: () = msg_send![class!(SCScreenshotManager), captureImageWithFilter: &*filter, configuration: &*config, completionHandler: &*completion];
        });
    });
    let _: () = msg_send![class!(SCShareableContent), getShareableContentExcludingDesktopWindows: true, onScreenWindowsOnly: false, completionHandler: &*completion];
}
unsafe fn fallback(request: Request) {
    // CG's legacy one-shot API is available on macOS 12; do not retain its full
    // resolution image beyond this downscale. Bound the transient admission too.
    if request.source_pixels > 32 * 1024 * 1024 {
        finish(request, None);
        return;
    }
    let Some(image) = ffi::Owned::from_create(ffi::CGWindowListCreateImage(
        ffi::rect(f64::INFINITY, f64::INFINITY, 0.0, 0.0),
        8,
        request.id,
        1 | 8,
    )) else {
        finish(request, None);
        return;
    };
    finish(request, resize(image.0, request.width, request.height));
}
pub unsafe fn resize(image: Ref, max_width: usize, max_height: usize) -> Option<Image> {
    if image.is_null() {
        return None;
    }
    let width = ffi::CGImageGetWidth(image);
    let height = ffi::CGImageGetHeight(image);
    if width == 0 || height == 0 {
        return None;
    }
    let scale = (max_width as f64 / width as f64)
        .min(max_height as f64 / height as f64)
        .min(1.0);
    let w = max_width.max(1);
    let h = max_height.max(1);
    let drawn_w = (width as f64 * scale).floor().max(1.0);
    let drawn_h = (height as f64 * scale).floor().max(1.0);
    let row = (w * 4 + 63) & !63;
    let space = ffi::Owned::from_create(ffi::CGColorSpaceCreateDeviceRGB())?;
    let context = ffi::Owned::from_create(ffi::CGBitmapContextCreate(
        ptr::null_mut(),
        w,
        h,
        8,
        row,
        space.0,
        0x2002,
    ))?;
    ffi::CGContextClearRect(
        context.0 as ffi::MutRef,
        ffi::rect(0.0, 0.0, w as f64, h as f64),
    );
    ffi::CGContextDrawImage(
        context.0 as ffi::MutRef,
        ffi::rect(
            (w as f64 - drawn_w) / 2.0,
            (h as f64 - drawn_h) / 2.0,
            drawn_w,
            drawn_h,
        ),
        image,
    );
    ffi::Owned::from_create(ffi::CGBitmapContextCreateImage(context.0 as ffi::MutRef)).map(
        |owned| {
            let image = Image(owned.0 as usize);
            std::mem::forget(owned);
            image
        },
    )
}
fn finish(request: Request, image: Option<Image>) {
    app::post(Event::Captured { request, image });
}
