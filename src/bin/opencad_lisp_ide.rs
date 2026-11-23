#![allow(non_snake_case)]
// On Windows release builds, hide the console window
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use opencad_lisp::ide::app::IdeApp;

#[cfg(windows)]
fn ensure_single_instance() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let name: Vec<u16> = OsStr::new("Local\\OpenCAD_LISP_IDE_SingleInstance")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        extern "system" {
            fn CreateMutexW(lpMutexAttributes: *mut std::ffi::c_void, bInitialOwner: i32, lpName: *const u16) -> *mut std::ffi::c_void;
            fn GetLastError() -> u32;
        }
        let handle = CreateMutexW(std::ptr::null_mut(), 1, name.as_ptr());
        if GetLastError() == 183 { // ERROR_ALREADY_EXISTS
            opencad_lisp::ide::app::force_foreground_ide();
            return false;
        }
        let _ = handle;
    }
    true
}

#[cfg(not(windows))]
fn ensure_single_instance() -> bool {
    true
}

fn main() -> iced::Result {
    if !ensure_single_instance() {
        return Ok(());
    }

    iced::application(IdeApp::new, IdeApp::update, IdeApp::view)
        .title(IdeApp::title)
        .subscription(IdeApp::subscription)
        .theme(|app: &IdeApp| app.theme.clone())
        .window(iced::window::Settings {
            size: iced::Size::new(1150.0, 780.0),
            min_size: Some(iced::Size::new(640.0, 420.0)),
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}
