//! Turns off WebView2's browser shortcuts (ADR 0013). This crate exists to hold the one call: the Windows
//! bindings mark every WebView2 call `unsafe`, and `tsugiki-core` and `tsugiki-app` forbid unsafe code.
//! Delete it when Tauri releases its window option for this (tauri-apps/tauri#16397).

#![cfg(windows)]

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Controller, ICoreWebView2Settings3,
};
use windows_core::Interface;

/// Print, Reload, Find, zoom and DevTools stop reaching the engine, whatever the page does.
pub fn turn_off(controller: &ICoreWebView2Controller) -> windows_core::Result<()> {
    #[expect(
        unsafe_code,
        reason = "the bindings mark every COM call unsafe, and Tauri hands us a live controller on its own thread"
    )]
    // SAFETY: `controller` is a valid COM reference for the whole call, and each call returns an owned
    // reference or an error. The call passes no pointers of its own.
    unsafe {
        controller
            .CoreWebView2()?
            .Settings()?
            .cast::<ICoreWebView2Settings3>()?
            .SetAreBrowserAcceleratorKeysEnabled(false)
    }
}
