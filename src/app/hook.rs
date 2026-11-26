//! Narrow `WH_KEYBOARD_LL` hook for the 數墊 (physical numpad) feature.
//!
//! - Low-level hook only: no `WH_KEYBOARD` DLL injected into other
//!   processes, no driver, no TSF / system IME, no Raw Input or
//!   `RegisterHotKey` swallow path.
//! - Installed only while 數墊 is on and the window is visible and not
//!   minimized; removed when 數墊 is turned off, on minimize, and on exit
//!   (`UnhookWindowsHookEx`). The callback re-checks the same gate on every
//!   key, so a stale hook passes everything through.
//! - The callback is short: it never composes, draws, does file I/O or calls
//!   `SendInput`. For `VK_NUMPAD0`–`VK_NUMPAD9` that pass the gate it
//!   `PostMessageW`s the digit to the window and eats the key; every other
//!   key goes to `CallNextHookEx`. Injected events (`LLKHF_INJECTED` /
//!   `LLKHF_LOWER_IL_INJECTED`), including on99KB's own `SendInput`, always
//!   pass through. It never activates or focuses the window.
//! - NumLock: Windows reports the physical numpad keys as `VK_NUMPAD0`–
//!   `VK_NUMPAD9` only while NumLock is on; with NumLock off they arrive as
//!   navigation keys (Insert, End, Down, …), so the gate's NumLock condition
//!   is read from the virtual key itself rather than from a possibly stale
//!   per-thread `GetKeyState`.
//!
//! Best effort, not an IME: elevated targets may be blocked by UIPI, games
//! reading Raw Input may still see the keys, and security software may flag
//! a keyboard hook.

use super::WM_APP_NUMPAD;
use on99kb::ime::{numpad_digit, should_capture, Gate};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, IsIconic, IsWindowVisible, PostMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_SYSKEYDOWN,
};

static HOOK: AtomicIsize = AtomicIsize::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static CAPTURE: AtomicBool = AtomicBool::new(false);

/// Bring the hook in line with 數墊 and the window's visibility.
pub(super) fn sync(hwnd: HWND, capture_on: bool) {
    TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    CAPTURE.store(capture_on, Ordering::Relaxed);
    let want =
        unsafe { capture_on && IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() };
    let installed = HOOK.load(Ordering::Relaxed) != 0;
    if want && !installed {
        let module: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .map(Into::into)
            .unwrap_or_default();
        if let Ok(h) = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0) } {
            HOOK.store(h.0 as isize, Ordering::Relaxed);
        }
    } else if !want && installed {
        uninstall();
    }
}

/// Remove the hook (idempotent). Called when 數墊 goes off, on minimize, and
/// on close / destroy.
pub(super) fn uninstall() {
    let h = HOOK.swap(0, Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(h as *mut _));
        }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let hwnd = HWND(TARGET.load(Ordering::Relaxed) as *mut _);
        let gate = Gate {
            capture_on: CAPTURE.load(Ordering::Relaxed),
            visible: !hwnd.0.is_null() && IsWindowVisible(hwnd).as_bool(),
            minimized: IsIconic(hwnd).as_bool(),
            numlock: numpad_digit(kb.vkCode).is_some(),
        };
        if let Some(d) = should_capture(gate, kb.vkCode, kb.flags.0) {
            let msg = wparam.0 as u32;
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                let _ = PostMessageW(hwnd, WM_APP_NUMPAD, WPARAM(usize::from(d)), LPARAM(0));
            }
            // Eat key down and key up so the digit never reaches the app.
            return LRESULT(1);
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}
