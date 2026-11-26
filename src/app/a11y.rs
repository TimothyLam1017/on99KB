//! Screen reader support (MSAA `IAccessible`).
//!
//! The keyboard is one custom-drawn window, so without this a screen reader
//! only sees an unnamed client area. We answer `WM_GETOBJECT` for the client
//! with an `IAccessible` whose children are the keys currently on screen.
//! Each key is a simple element (child id = index + 1 in [`visible_hits`])
//! with a name, a push-button role, hover/pressed/checked state, its screen
//! location and a default action that presses it. UI Automation clients such
//! as Narrator read MSAA through the built-in MSAA-to-UIA proxy.

use super::*;
use windows::core::{implement, Error, Interface, BSTR, GUID, VARIANT};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, S_FALSE};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{
    IDispatch, IDispatch_Impl, ITypeInfo, DISPATCH_FLAGS, DISPPARAMS, EXCEPINFO,
};
use windows::Win32::UI::Accessibility::{
    CreateStdAccessibleObject, IAccessible, IAccessible_Impl, LresultFromObject, NotifyWinEvent,
    NAVDIR_FIRSTCHILD, NAVDIR_LASTCHILD, NAVDIR_NEXT, NAVDIR_PREVIOUS, ROLE_SYSTEM_CLIENT,
    ROLE_SYSTEM_PUSHBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_REORDER, EVENT_OBJECT_STATECHANGE, OBJID_CLIENT,
    STATE_SYSTEM_CHECKED, STATE_SYSTEM_HOTTRACKED,
};

// Not exported under these names by windows 0.58's UI::Accessibility.
const STATE_PRESSED: i32 = 0x8;
const STATE_UNAVAILABLE: i32 = 0x1;

/// Every hit target drawn right now, in reading order.
pub(super) fn visible_hits(state: &AppState, client: RECT) -> Vec<Hit> {
    let mut out = Vec::with_capacity(272);
    if state.mode == Mode::Simplex {
        out.push(Hit::Brand);
        if !state.symbols_open {
            let n = state.current_page_items().len().min(9);
            out.extend((0..n as u8).map(Hit::Candidate));
        }
        out.push(Hit::PagePrev);
        out.push(Hit::PageNext);
        let layout = state.layout(client);
        out.extend(layout.digits.iter().map(|(c, _)| Hit::Digit(*c)));
        out.push(Hit::Backspace);
        out.extend(layout.puncts.iter().map(|(c, _)| Hit::Punct(*c)));
        out.extend(layout.navs.iter().map(|(n, _)| Hit::Nav(*n)));
        if state.symbols_open {
            out.extend(layout.symbols.iter().map(|(c, _, _)| Hit::Symbol(*c)));
        } else {
            out.extend(layout.letters.iter().map(|(c, _)| Hit::Letter(*c)));
        }
        out.extend(layout.english.iter().map(|(c, _)| Hit::Latin(*c)));
        out.extend(layout.actions.iter().map(|(h, _, _)| *h));
    } else {
        out.extend((0u8..=255).map(Hit::Cp437));
    }
    out.push(Hit::Mode(Mode::Cp437));
    out.push(Hit::Mode(Mode::Simplex));
    out.push(Hit::Opacity);
    out.push(Hit::AsciiSpace);
    out
}

/// Spoken name for a key. Short, and says what pressing it does.
pub(super) fn hit_name(state: &AppState, hit: Hit) -> String {
    let on_off = |b: bool| if b { "on" } else { "off" };
    match hit {
        Hit::Cp437(0) | Hit::Brand => format!("Keep on top, {}", on_off(state.topmost)),
        Hit::Cp437(32) => "Backspace".to_string(),
        Hit::Cp437(255) => "No-break space, Alt+255".to_string(),
        Hit::Cp437(code) => match cp437::glyph(code) {
            Some(ch) => format!("{ch}, Alt+{code}, U+{:04X}", ch as u32),
            None => format!("Alt+{code}"),
        },
        Hit::Mode(Mode::Cp437) => "CP437 keyboard".to_string(),
        Hit::Mode(Mode::Simplex) => "速成 Simplex keyboard".to_string(),
        Hit::Opacity => format!("Opacity {}", state.opacity.label()),
        Hit::AsciiSpace => "Space".to_string(),
        Hit::Candidate(i) => match state.current_page_items().get(i as usize) {
            Some(ch) => format!("Candidate {}, {ch}", i + 1),
            None => format!("Candidate {}, empty", i + 1),
        },
        Hit::PagePrev => "Previous candidates".to_string(),
        Hit::PageNext => "Next candidates".to_string(),
        Hit::Letter(ch) => match simplex::radical_for(ch) {
            Some(rad) => format!("{rad}, code {}", ch.to_ascii_uppercase()),
            None => ch.to_ascii_uppercase().to_string(),
        },
        Hit::Latin(',') => "Comma; hold for less-than".to_string(),
        Hit::Latin('.') => "Period; hold for greater-than".to_string(),
        Hit::Latin(ch) => format!("{ch}; hold for capital"),
        Hit::Digit(ch) | Hit::Punct(ch) | Hit::Symbol(ch) => ch.to_string(),
        Hit::Nav(NavKey::Home) => "Home".to_string(),
        Hit::Nav(NavKey::End) => "End".to_string(),
        Hit::Nav(NavKey::PageUp) => "Page Up".to_string(),
        Hit::Nav(NavKey::PageDown) => "Page Down".to_string(),
        Hit::Backspace => {
            if has_simplex_composition(state) {
                "Backspace, cancel codes".to_string()
            } else {
                "Backspace".to_string()
            }
        }
        Hit::Delete => "刪, delete one code".to_string(),
        Hit::Clear => "清除, clear codes".to_string(),
        Hit::Space => "空格, type first candidate".to_string(),
        Hit::SymbolToggle => "符號, symbols".to_string(),
    }
}

fn hit_state_flags(state: &AppState, hit: Hit) -> i32 {
    let mut flags = 0;
    if state.hover == Some(hit) {
        flags |= STATE_SYSTEM_HOTTRACKED as i32;
    }
    if state.pressed == Some(hit) {
        flags |= STATE_PRESSED;
    }
    let checked = match hit {
        Hit::Mode(m) => state.mode == m,
        Hit::Cp437(0) | Hit::Brand => state.topmost,
        Hit::SymbolToggle => state.symbols_open,
        _ => false,
    };
    if checked {
        flags |= STATE_SYSTEM_CHECKED as i32;
    }
    let disabled = match hit {
        Hit::PagePrev => state.symbols_open || !simplex::can_page_prev(state.page),
        Hit::PageNext => {
            state.symbols_open
                || !simplex::can_page_next(state.current_candidates().len(), state.page)
        }
        _ => false,
    };
    if disabled {
        flags |= STATE_UNAVAILABLE;
    }
    flags
}

/// Child id (1-based) of `hit` in the current key list, if it is on screen.
fn child_id(state: &AppState, client: RECT, hit: Hit) -> Option<i32> {
    visible_hits(state, client)
        .iter()
        .position(|h| *h == hit)
        .map(|i| i as i32 + 1)
}

/// Tell listening screen readers a key's hover/pressed state changed.
/// Cheap when nothing is listening.
pub(super) fn notify_state(hwnd: HWND, state: &AppState, hits: &[Option<Hit>]) {
    let client = client_rect(hwnd);
    for hit in hits.iter().flatten() {
        if let Some(id) = child_id(state, client, *hit) {
            unsafe { NotifyWinEvent(EVENT_OBJECT_STATECHANGE, hwnd, OBJID_CLIENT.0, id) };
        }
    }
}

/// The set of keys (or their names) changed, e.g. new candidates or mode.
pub(super) fn notify_reorder(hwnd: HWND) {
    unsafe {
        NotifyWinEvent(
            EVENT_OBJECT_REORDER,
            hwnd,
            OBJID_CLIENT.0,
            CHILDID_SELF as i32,
        )
    };
}

/// Answer `WM_GETOBJECT` for the client area; `None` means let
/// `DefWindowProc` handle it.
pub(super) fn on_get_object(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if lparam.0 as i32 != OBJID_CLIENT.0 {
        return None;
    }
    let std_obj: IAccessible = unsafe {
        let mut p: *mut std::ffi::c_void = std::ptr::null_mut();
        CreateStdAccessibleObject(hwnd, OBJID_CLIENT.0, &IAccessible::IID, &mut p).ok()?;
        IAccessible::from_raw(p)
    };
    let acc: IAccessible = KeyboardAccessible { hwnd, std_obj }.into();
    unsafe { Some(LresultFromObject(&IAccessible::IID, wparam, &acc)) }
}

#[implement(IAccessible)]
struct KeyboardAccessible {
    hwnd: HWND,
    /// Default client-area object; handles parent/window-level queries.
    std_obj: IAccessible,
}

impl KeyboardAccessible {
    fn state(&self) -> windows::core::Result<&'static mut AppState> {
        state_mut(self.hwnd).ok_or_else(|| Error::from(E_INVALIDARG))
    }

    /// `Ok(None)` = the client itself (CHILDID_SELF), `Ok(Some(hit))` = a key.
    fn child(&self, var: &VARIANT) -> windows::core::Result<Option<Hit>> {
        let id = i32::try_from(var).map_err(|_| Error::from(E_INVALIDARG))?;
        if id == CHILDID_SELF as i32 {
            return Ok(None);
        }
        let state = self.state()?;
        let hits = visible_hits(state, client_rect(self.hwnd));
        usize::try_from(id - 1)
            .ok()
            .and_then(|i| hits.get(i).copied())
            .map(Some)
            .ok_or_else(|| Error::from(E_INVALIDARG))
    }
}

impl IDispatch_Impl for KeyboardAccessible_Impl {
    fn GetTypeInfoCount(&self) -> windows::core::Result<u32> {
        Ok(0)
    }
    fn GetTypeInfo(&self, _: u32, _: u32) -> windows::core::Result<ITypeInfo> {
        Err(E_NOTIMPL.into())
    }
    fn GetIDsOfNames(
        &self,
        _: *const GUID,
        _: *const PCWSTR,
        _: u32,
        _: u32,
        _: *mut i32,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Invoke(
        &self,
        _: i32,
        _: *const GUID,
        _: u32,
        _: DISPATCH_FLAGS,
        _: *const DISPPARAMS,
        _: *mut VARIANT,
        _: *mut EXCEPINFO,
        _: *mut u32,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
}

impl IAccessible_Impl for KeyboardAccessible_Impl {
    fn accParent(&self) -> windows::core::Result<IDispatch> {
        unsafe { self.std_obj.accParent() }
    }

    fn accChildCount(&self) -> windows::core::Result<i32> {
        let state = self.state()?;
        Ok(visible_hits(state, client_rect(self.hwnd)).len() as i32)
    }

    fn get_accChild(&self, _: &VARIANT) -> windows::core::Result<IDispatch> {
        // Keys are simple elements, not separate objects.
        Err(Error::from(S_FALSE))
    }

    fn get_accName(&self, var: &VARIANT) -> windows::core::Result<BSTR> {
        match self.child(var)? {
            None => Ok(BSTR::from(match self.state()?.mode {
                Mode::Cp437 => "on99KB CP437 keyboard",
                Mode::Simplex => "on99KB 速成 keyboard",
            })),
            Some(hit) => Ok(BSTR::from(hit_name(self.state()?, hit))),
        }
    }

    fn get_accValue(&self, var: &VARIANT) -> windows::core::Result<BSTR> {
        // The client's value is the 速成 code typed so far.
        match self.child(var)? {
            None => {
                let state = self.state()?;
                if state.mode == Mode::Simplex && !state.codes.is_empty() {
                    Ok(BSTR::from(state.codes.to_ascii_uppercase()))
                } else {
                    Err(Error::from(S_FALSE))
                }
            }
            Some(_) => Err(Error::from(S_FALSE)),
        }
    }

    fn get_accDescription(&self, _: &VARIANT) -> windows::core::Result<BSTR> {
        Err(Error::from(S_FALSE))
    }

    fn get_accRole(&self, var: &VARIANT) -> windows::core::Result<VARIANT> {
        Ok(VARIANT::from(match self.child(var)? {
            None => ROLE_SYSTEM_CLIENT as i32,
            Some(_) => ROLE_SYSTEM_PUSHBUTTON as i32,
        }))
    }

    fn get_accState(&self, var: &VARIANT) -> windows::core::Result<VARIANT> {
        match self.child(var)? {
            None => unsafe { self.std_obj.get_accState(var) },
            Some(hit) => Ok(VARIANT::from(hit_state_flags(self.state()?, hit))),
        }
    }

    fn get_accHelp(&self, _: &VARIANT) -> windows::core::Result<BSTR> {
        Err(Error::from(S_FALSE))
    }

    fn get_accHelpTopic(&self, _: *mut BSTR, _: &VARIANT) -> windows::core::Result<i32> {
        Err(Error::from(S_FALSE))
    }

    fn get_accKeyboardShortcut(&self, _: &VARIANT) -> windows::core::Result<BSTR> {
        Err(Error::from(S_FALSE))
    }

    fn accFocus(&self) -> windows::core::Result<VARIANT> {
        // The window never takes focus (WS_EX_NOACTIVATE).
        Ok(VARIANT::default())
    }

    fn accSelection(&self) -> windows::core::Result<VARIANT> {
        Ok(VARIANT::default())
    }

    fn get_accDefaultAction(&self, var: &VARIANT) -> windows::core::Result<BSTR> {
        match self.child(var)? {
            None => Err(Error::from(S_FALSE)),
            Some(_) => Ok(BSTR::from("Press")),
        }
    }

    fn accSelect(&self, _: i32, _: &VARIANT) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn accLocation(
        &self,
        x: *mut i32,
        y: *mut i32,
        w: *mut i32,
        h: *mut i32,
        var: &VARIANT,
    ) -> windows::core::Result<()> {
        let Some(hit) = self.child(var)? else {
            return unsafe { self.std_obj.accLocation(x, y, w, h, var) };
        };
        let state = self.state()?;
        let rc = state
            .hit_rect(client_rect(self.hwnd), hit)
            .ok_or_else(|| Error::from(E_INVALIDARG))?;
        let mut pt = POINT {
            x: rc.left,
            y: rc.top,
        };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut pt);
            *x = pt.x;
            *y = pt.y;
            *w = rc.right - rc.left;
            *h = rc.bottom - rc.top;
        }
        Ok(())
    }

    fn accNavigate(&self, dir: i32, start: &VARIANT) -> windows::core::Result<VARIANT> {
        let state = self.state()?;
        let count = visible_hits(state, client_rect(self.hwnd)).len() as i32;
        let from = i32::try_from(start).map_err(|_| Error::from(E_INVALIDARG))?;
        let to = match dir as u32 {
            NAVDIR_FIRSTCHILD if from == 0 => 1,
            NAVDIR_LASTCHILD if from == 0 => count,
            NAVDIR_NEXT if from > 0 => from + 1,
            NAVDIR_PREVIOUS if from > 0 => from - 1,
            _ if from == 0 => return unsafe { self.std_obj.accNavigate(dir, start) },
            _ => return Err(Error::from(S_FALSE)),
        };
        if (1..=count).contains(&to) {
            Ok(VARIANT::from(to))
        } else {
            Err(Error::from(S_FALSE))
        }
    }

    fn accHitTest(&self, x: i32, y: i32) -> windows::core::Result<VARIANT> {
        let state = self.state()?;
        let mut pt = POINT { x, y };
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut pt);
        }
        let client = client_rect(self.hwnd);
        if !pt_in(client, pt.x, pt.y) {
            return Ok(VARIANT::default());
        }
        let id = state
            .hit_test(client, pt.x, pt.y)
            .and_then(|hit| child_id(state, client, hit))
            .unwrap_or(CHILDID_SELF as i32);
        Ok(VARIANT::from(id))
    }

    fn accDoDefaultAction(&self, var: &VARIANT) -> windows::core::Result<()> {
        let Some(hit) = self.child(var)? else {
            return Err(Error::from(S_FALSE));
        };
        let state = self.state()?;
        activate_hit(self.hwnd, state, hit);
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
        notify_reorder(self.hwnd);
        Ok(())
    }

    fn put_accName(&self, _: &VARIANT, _: &BSTR) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn put_accValue(&self, _: &VARIANT, _: &BSTR) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
}
