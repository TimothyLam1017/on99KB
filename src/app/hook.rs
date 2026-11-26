//! Narrow `WH_KEYBOARD_LL` hook for the 數墊 (physical numpad) feature and
//! physical Backspace 清碼.
//!
//! - Low-level hook only: no `WH_KEYBOARD` DLL injected into other
//!   processes, no driver, no TSF / system IME, no Raw Input or
//!   `RegisterHotKey` swallow path.
//! - Installed while the window is visible and not minimized **and** 數墊 is
//!   on or a 九方 / 速成 composition or the overlay is active (physical
//!   Backspace 清碼 is not tied to 數墊); removed otherwise, on minimize, and
//!   on exit (`UnhookWindowsHookEx`). The callback re-checks the gates on
//!   every key, so a stale hook passes everything through.
//! - The callback is short: it never composes, draws, does file I/O or calls
//!   `SendInput`. It only reads atomics and the window's visibility,
//!   `PostMessageW`s, and eats or calls `CallNextHookEx`:
//!   - `VK_NUMPAD0`–`VK_NUMPAD9` that pass the 數墊 gate: the first key-down
//!     is posted (`WM_APP_NUMPAD`), auto-repeats are eaten, and the key-up
//!     is eaten and posted (`WM_APP_NUMPAD_UP`) so the UI types a numpad
//!     commit only after the physical key is released.
//!   - `VK_BACK` while a composition / overlay is active: the key-down is
//!     posted (`WM_APP_BACKSPACE`) and eaten; its repeats and key-up are
//!     eaten too. Idle Backspace passes through untouched.
//!   - Every other key goes to `CallNextHookEx`.
//!
//!   Injected events (`LLKHF_INJECTED` / `LLKHF_LOWER_IL_INJECTED`),
//!   including on99KB's own `SendInput`, always pass through. It never
//!   activates or focuses the window.
//! - NumLock: Windows reports the physical numpad keys as `VK_NUMPAD0`–
//!   `VK_NUMPAD9` only while NumLock is on; with NumLock off they arrive as
//!   navigation keys (Insert, End, Down, …), so the gate's NumLock condition
//!   is read from the virtual key itself rather than from a possibly stale
//!   per-thread `GetKeyState`.
//!
//! Best effort, not an IME: elevated targets may be blocked by UIPI, games
//! reading Raw Input may still see the keys, and security software may flag
//! a keyboard hook.

use super::{WM_APP_BACKSPACE, WM_APP_NUMPAD, WM_APP_NUMPAD_UP};
use on99kb::ime::{
    backspace_event, hook_wanted, numpad_digit, numpad_event, BkspGate, BkspVerdict, Gate,
    NumpadHeld, NumpadVerdict,
};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, IsIconic, IsWindowVisible, PostMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

static HOOK: AtomicIsize = AtomicIsize::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);
/// 數墊 is on.
static CAPTURE: AtomicBool = AtomicBool::new(false);
/// "Eat physical Bksp now": a 九方 / 速成 composition or the overlay.
static BKSP_ACTIVE: AtomicBool = AtomicBool::new(false);
/// A captured Backspace key-down whose key-up has not arrived.
static BKSP_EATING: AtomicBool = AtomicBool::new(false);
thread_local! {
    /// Numpad presses taken on key-down, awaiting key-up. A low-level hook
    /// runs on the thread that installed it (the UI thread), as does
    /// `sync`, so a thread-local is enough.
    static NUMPAD_HELD: Cell<NumpadHeld> = Cell::new(NumpadHeld::default());
}

/// Bring the hook in line with 數墊, the composition state and the window's
/// visibility. Call after anything that can start or end a composition or
/// the overlay, change 數墊, or show / hide / minimize the window.
pub(super) fn sync(hwnd: HWND, capture_on: bool, bksp_active: bool) {
    TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    CAPTURE.store(capture_on, Ordering::Relaxed);
    BKSP_ACTIVE.store(bksp_active, Ordering::Relaxed);
    let (visible, minimized) =
        unsafe { (IsWindowVisible(hwnd).as_bool(), IsIconic(hwnd).as_bool()) };
    let installed = HOOK.load(Ordering::Relaxed) != 0;
    let pending_up =
        installed && (BKSP_EATING.load(Ordering::Relaxed) || NUMPAD_HELD.with(|h| h.get().any()));
    let want = hook_wanted(capture_on, bksp_active, visible, minimized, pending_up);
    if want && !installed {
        BKSP_EATING.store(false, Ordering::Relaxed);
        NUMPAD_HELD.with(|h| h.set(NumpadHeld::default()));
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

/// Remove the hook (idempotent). Called when it is no longer wanted, and on
/// close / destroy.
pub(super) fn uninstall() {
    let h = HOOK.swap(0, Ordering::Relaxed);
    BKSP_EATING.store(false, Ordering::Relaxed);
    NUMPAD_HELD.with(|h| h.set(NumpadHeld::default()));
    if h != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(h as *mut _));
        }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        if down || up {
            let hwnd = HWND(TARGET.load(Ordering::Relaxed) as *mut _);
            let visible = !hwnd.0.is_null() && IsWindowVisible(hwnd).as_bool();
            let minimized = IsIconic(hwnd).as_bool();
            let flags = kb.flags.0;

            let gate = Gate {
                capture_on: CAPTURE.load(Ordering::Relaxed),
                visible,
                minimized,
                numlock: numpad_digit(kb.vkCode).is_some(),
            };
            let held = NUMPAD_HELD.with(Cell::get);
            let (v, held2) = numpad_event(gate, held, kb.vkCode, kb.scanCode, flags, down);
            NUMPAD_HELD.with(|h| h.set(held2));
            match v {
                NumpadVerdict::Pass => {}
                NumpadVerdict::Eat => return LRESULT(1),
                NumpadVerdict::Down(d) => {
                    let _ = PostMessageW(hwnd, WM_APP_NUMPAD, WPARAM(usize::from(d)), LPARAM(0));
                    return LRESULT(1);
                }
                NumpadVerdict::Up(d) => {
                    let _ = PostMessageW(hwnd, WM_APP_NUMPAD_UP, WPARAM(usize::from(d)), LPARAM(0));
                    return LRESULT(1);
                }
            }

            let bgate = BkspGate {
                active: BKSP_ACTIVE.load(Ordering::Relaxed),
                visible,
                minimized,
            };
            let eating = BKSP_EATING.load(Ordering::Relaxed);
            let (v, eating2) = backspace_event(bgate, eating, kb.vkCode, flags, down);
            BKSP_EATING.store(eating2, Ordering::Relaxed);
            match v {
                BkspVerdict::Pass => {}
                BkspVerdict::Eat => return LRESULT(1),
                BkspVerdict::Clear => {
                    let _ = PostMessageW(hwnd, WM_APP_BACKSPACE, WPARAM(0), LPARAM(0));
                    return LRESULT(1);
                }
                BkspVerdict::Released => {
                    let _ = PostMessageW(hwnd, WM_APP_BACKSPACE, WPARAM(1), LPARAM(0));
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}
