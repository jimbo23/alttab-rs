#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod capture;
#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
mod input;
#[cfg(target_os = "macos")]
mod tracking;
#[cfg(target_os = "macos")]
mod ui;

#[cfg(target_os = "macos")]
fn main() {
    app::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("alttab-rs is a native macOS application (macOS 12 or later). The portable alttab-core library and its tests are available on this platform.");
}
