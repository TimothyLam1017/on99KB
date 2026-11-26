//! Native Win32 on-screen keyboard.
//!
//! The window is always-on-top, resizable, and created with `WS_EX_NOACTIVATE`
//! so clicks type into the previously focused application instead of stealing
//! focus. Characters are injected with `SendInput` + `KEYEVENTF_UNICODE`.
//!
//! First launch (no saved size) opens at the measured CP437 client size
//! 480×460 — not the old 900×820 default. 速成 keeps that compactness when it
//! fits, and otherwise grows just enough (SWP_NOACTIVATE) for the candidate
//! bar, auxiliary rows, a readable radical keyboard, a lowercase English
//! QWERTY band, a compact action row, and the status bar. Last window size
//! and keyboard mode are remembered.
//!
//! 1.3.0 adds a third layout, 九方 (shape input; computer numpad — the
//! common, unofficial name; not affiliated with 九方科技), a 繁｜簡 output
//! control (OpenCC hk2s; the Windows IME is never touched), and a 數墊 toggle
//! for a narrowly scoped `WH_KEYBOARD_LL` hook that turns physical numpad
//! 0–9 into 九方 keys in every layout. On CP437 / 速成 those keys open a
//! focus-free overlay panel drawn inside this same window; nothing ever
//! calls SetFocus / SetForegroundWindow / SetActiveWindow.

#![cfg(windows)]

mod a11y;
mod hook;

use on99kb::ime::{self, Action};
use on99kb::prefs::{KeyboardMode as Mode, OutputScript};
use on99kb::{cp437, prefs, q9, script, simplex};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::{w, Result as WinResult, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetGlyphIndicesW,
    GetStockObject, IntersectClipRect, InvalidateRect, RoundRect, ScreenToClient, SelectClipRgn,
    SelectObject, SetBkMode, SetTextColor, BLACK_BRUSH, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_END_ELLIPSIS, DT_NOCLIP, DT_RIGHT, DT_SINGLELINE,
    DT_TOP, DT_VCENTER, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, GGI_MARK_NONEXISTING_GLYPHS, HBITMAP,
    HBRUSH, HDC, HFONT, HGDIOBJ, HPEN, HRGN, OUT_TT_PRECIS, PAINTSTRUCT, PS_INSIDEFRAME, PS_SOLID,
    SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, ReleaseCapture, SendInput, SetCapture, TrackMouseEvent, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MAPVK_VK_TO_VSC, TME_LEAVE, TRACKMOUSEEVENT, VIRTUAL_KEY, VK_BACK, VK_END, VK_HOME, VK_NEXT,
    VK_PRIOR,
};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos,
    GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, IsIconic, KillTimer,
    LoadCursorW, LoadImageW, MessageBoxW, MoveWindow, PostQuitMessage, RegisterClassExW, SetCursor,
    SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
    ShowWindow, TranslateMessage, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA,
    HICON, HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW, IDC_HAND, IMAGE_ICON, LR_DEFAULTCOLOR,
    LR_DEFAULTSIZE, LWA_ALPHA, MA_NOACTIVATE, MB_ICONERROR, MB_OK, MINMAXINFO, MSG, SM_CXSMICON,
    SM_CXVIRTUALSCREEN, SM_CYSMICON, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_EXITSIZEMOVE,
    WM_GETMINMAXINFO, WM_GETOBJECT, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_MOVE, WM_PAINT, WM_SETCURSOR, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_CAPTION,
    WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_MINIMIZEBOX, WS_POPUP,
    WS_SYSMENU, WS_THICKFRAME,
};

const WM_MOUSELEAVE: u32 = 0x02A3;
const SIZE_MINIMIZED: u32 = 1;
const SAVE_TIMER_ID: usize = 1;
const SAVE_DEBOUNCE_MS: u32 = 400;
const LATIN_HOLD_TIMER_ID: usize = 2;
const LATIN_HOLD_MS: u32 = 350;

const COLS: i32 = 16;
const ROWS: i32 = 16;
const GRID_PAD: i32 = 8;
const KEY_GAP: i32 = 3;
const CANDIDATE_ROW_H: i32 = 48;
/// Bottom control bar, left to right: `CP437｜速成｜九方`, opacity, `繁｜簡`,
/// 數墊, then a wide Space. Widths are design pixels; at the default 480 the
/// Space bar gets 136, above `SPACE_BAR_MIN_W`.
const BAR_PAD: i32 = 8;
const BAR_GAP: i32 = 6;
const MODE_SEG_W: i32 = 52;
const OPACITY_BTN_W: i32 = 44;
const OUTPUT_SEG_W: i32 = 30;
const NUMPAD_BTN_W: i32 = 44;
const SPACE_BAR_MIN_W: i32 = 96;
const SPACE_BAR_MAX_W: i32 = 280;
/// Dedicated full-width row above the control bar for the hint text, so it
/// is never squeezed into the space left beside the Space bar.
/// This is its minimum height; it grows with the status font.
const HINT_ROW_MIN_H: i32 = 20;
/// Below this key height the CP437 caption shrinks to the bare number.
const CP437_FULL_CAPTION_H: i32 = 30;
const CP437_COMPACT_CAPTION_H: i32 = 20;
const CP437_COMPACT_CAPTION_BAND: i32 = 9;
const ICON_RESOURCE_ID: u16 = 1;
const AUX_ROW_H: i32 = 28;
const LETTER_ROW_MIN_H: i32 = 46;
const LETTER_ROW_MAX_H: i32 = 54;
const ACTION_ROW_H: i32 = 26;
const SIMPLEX_GAP: i32 = 3;
const ENGLISH_ROW_MIN_H: i32 = 22;
const ENGLISH_ROW_MAX_H: i32 = 54;
/// 九方 pad rows (3×3 plus the wide 0) on the 九方 layout.
const Q9_ROW_MIN_H: i32 = 34;
const Q9_ROW_MAX_H: i32 = 64;
const Q9_KEY_MAX_W: i32 = 120;
/// Overlay (over CP437 / 速成): header, pad rows, action row.
const OV_HEADER_H: i32 = 24;
const OV_ROW_H: i32 = 38;
const OV_ACTION_H: i32 = 26;
const OV_PAD: i32 = 6;
/// Private message the keyboard hook posts for a captured numpad digit.
const WM_APP_NUMPAD: u32 = 0x8000 + 0x39; // WM_APP + 0x39
/// The key-up of a captured numpad digit (a deferred numpad commit is typed
/// only now).
const WM_APP_NUMPAD_UP: u32 = 0x8000 + 0x3A;
/// Physical Backspace 清碼 (wparam 0) or the key-up of that press (1).
const WM_APP_BACKSPACE: u32 = 0x8000 + 0x3B;
/// Safety net: type a deferred numpad commit even if its key-up never comes.
const SEND_FLUSH_TIMER_ID: usize = 3;
const SEND_FLUSH_MS: u32 = 400;
/// Shown when SendInput could not type a commit; the composition is kept.
const SEND_FAILED_HINT: &str = "無法輸出到目前視窗";

/// Layout constants above (and the minimum client size in `prefs`) are in
/// device-independent pixels at 96 DPI (100%). Everything is scaled to the
/// window's current DPI with [`dp`] at the point of use, so the keyboard looks
/// the same at 100%, 150% and 200% and follows the window across monitors.
const BASE_DPI: u32 = 96;
static WINDOW_DPI: AtomicU32 = AtomicU32::new(BASE_DPI);

fn current_dpi() -> u32 {
    WINDOW_DPI.load(Ordering::Relaxed)
}

fn set_current_dpi(dpi: u32) {
    let dpi = if dpi == 0 { BASE_DPI } else { dpi };
    WINDOW_DPI.store(dpi, Ordering::Relaxed);
}

fn scale_by(v: i32, num: u32, den: u32) -> i32 {
    ((v as i64 * num as i64 * 2 + den as i64 * v.signum() as i64) / (den as i64 * 2)) as i32
}

/// 96-DPI design pixels → physical pixels at the window's current DPI.
fn dp(v: i32) -> i32 {
    scale_by(v, current_dpi(), BASE_DPI)
}

/// Physical pixels at the window's current DPI → 96-DPI design pixels.
fn undp(v: i32) -> i32 {
    scale_by(v, BASE_DPI, current_dpi())
}

/// Window size for a client size, using the frame metrics of `dpi`.
fn window_size_for_client(client_w: i32, client_h: i32, dpi: u32) -> (i32, i32) {
    let mut wr = RECT {
        left: 0,
        top: 0,
        right: client_w,
        bottom: client_h,
    };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut wr, APP_STYLE, false, APP_EX_STYLE_BASE, dpi);
    }
    (wr.right - wr.left, wr.bottom - wr.top)
}

const DIGIT_CHARS: [char; 10] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'];
const PUNCT_CHARS: [char; 4] = ['，', '。', '：', '；'];
const ENGLISH_ROWS: [&[char]; 3] = [
    &['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p'],
    &['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'],
    &['z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.'],
];

const APP_STYLE: WINDOW_STYLE =
    WINDOW_STYLE(WS_POPUP.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0);
/// Base extended style without TOPMOST so frame math stays consistent.
/// Pinning uses `WS_EX_APPWINDOW` (no `WS_EX_TOOLWINDOW`) so the app can be
/// pinned to the taskbar; Alt+Tab may then show it.
const APP_EX_STYLE_BASE: WINDOW_EX_STYLE =
    WINDOW_EX_STYLE(WS_EX_NOACTIVATE.0 | WS_EX_APPWINDOW.0 | WS_EX_LAYERED.0);
fn app_ex_style(topmost: bool) -> WINDOW_EX_STYLE {
    if topmost {
        WINDOW_EX_STYLE(APP_EX_STYLE_BASE.0 | WS_EX_TOPMOST.0)
    } else {
        APP_EX_STYLE_BASE
    }
}

fn latin_shifted_char(ch: char) -> char {
    match ch {
        ',' => '<',
        '.' => '>',
        _ => ch.to_ascii_uppercase(),
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

fn point_from_lparam(lparam: LPARAM) -> (i32, i32) {
    let v = lparam.0 as u32;
    let x = v as u16 as i16 as i32;
    let y = (v >> 16) as u16 as i16 as i32;
    (x, y)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn utf16_char(ch: char) -> Vec<u16> {
    let mut buf = [0u16; 2];
    ch.encode_utf16(&mut buf).to_vec()
}

fn handle_nonzero<T: Copy + PartialEq + Default>(h: T) -> bool {
    h != T::default()
}

fn pt_in(rc: RECT, x: i32, y: i32) -> bool {
    x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom
}

fn rects_overlap(a: RECT, b: RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Repaint just the keys whose hover/pressed look changed, plus the hint
/// row above the control bar (its text follows hover). Falls back to a full repaint if a key's
/// rect is unknown.
fn invalidate_hits(hwnd: HWND, state: &AppState, hits: &[Option<Hit>]) {
    let client = client_rect(hwnd);
    unsafe {
        for hit in hits.iter().flatten() {
            match state.hit_rect(client, *hit) {
                Some(rc) => {
                    // Rounded-rect outlines sit on the edge; pad a little,
                    // scaled so 150% / 200% outlines are covered too.
                    let pad = dp(2).max(1);
                    let rc = RECT {
                        left: rc.left - pad,
                        top: rc.top - pad,
                        right: rc.right + pad,
                        bottom: rc.bottom + pad,
                    };
                    let _ = InvalidateRect(hwnd, Some(&rc), false);
                }
                None => {
                    let _ = InvalidateRect(hwnd, None, false);
                    return;
                }
            }
        }
        let hint = AppState::hint_rect(client);
        let _ = InvalidateRect(hwnd, Some(&hint), false);
    }
}

/// Title follows the *visible* layout; the numpad overlay never changes it.
fn title_for(mode: Mode) -> &'static str {
    match mode {
        Mode::Cp437 => "on99KB  —  CP437 螢幕鍵盤",
        Mode::Simplex => "on99KB  —  速成 螢幕鍵盤",
        Mode::Jiufang => "on99KB  —  九方 螢幕鍵盤",
    }
}

const MODES: [(Mode, &str); 3] = [
    (Mode::Cp437, "CP437"),
    (Mode::Simplex, "速成"),
    (Mode::Jiufang, "九方"),
];

const OUTPUTS: [(OutputScript, &str); 2] = [
    (OutputScript::Traditional, "繁"),
    (OutputScript::Simplified, "簡"),
];

fn has_simplex_composition(state: &AppState) -> bool {
    state.ime.mode == Mode::Simplex
        && (!state.ime.codes.is_empty() || !state.current_candidates().is_empty())
}

/// The candidate bar at the top currently belongs to 九方 (the 九方 layout)
/// rather than 速成.
fn q9_bar(state: &AppState) -> bool {
    state.ime.mode == Mode::Jiufang
}

/// The overlay is drawn and takes clicks inside its rect.
fn overlay_shown(state: &AppState) -> bool {
    state.ime.overlay_open && state.ime.mode != Mode::Jiufang
}

fn make_int_resource(id: u16) -> PCWSTR {
    PCWSTR(id as usize as *const u16)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hit {
    Cp437(u8),
    Mode(Mode),
    Candidate(u8),
    PagePrev,
    PageNext,
    Letter(char),
    Delete,
    Clear,
    Space,
    SymbolToggle,
    Symbol(char),
    Digit(char),
    Punct(char),
    Nav(NavKey),
    Backspace,
    Latin(char),
    Opacity,
    Brand,
    AsciiSpace,
    /// 九方 pad key 0–9 (on the 九方 layout, or in the overlay).
    Q9(u8),
    /// Overlay action keys and paging (CP437 / 速成 only; on the 九方 layout
    /// the regular 刪碼 / 清除 / 空格 / page keys act on 九方).
    OvDelete,
    OvClear,
    OvSpace,
    OvPrev,
    OvNext,
    /// `繁｜簡` segment.
    Output(OutputScript),
    /// 數墊 capture toggle.
    NumpadToggle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NavKey {
    Home,
    End,
    PageUp,
    PageDown,
}

impl NavKey {
    const ALL: [NavKey; 4] = [NavKey::Home, NavKey::End, NavKey::PageUp, NavKey::PageDown];

    fn label(self) -> &'static str {
        match self {
            NavKey::Home => "Home",
            NavKey::End => "End",
            NavKey::PageUp => "PgUp",
            NavKey::PageDown => "PgDn",
        }
    }

    fn vk(self) -> VIRTUAL_KEY {
        match self {
            NavKey::Home => VK_HOME,
            NavKey::End => VK_END,
            NavKey::PageUp => VK_PRIOR,
            NavKey::PageDown => VK_NEXT,
        }
    }
}

struct Fonts {
    glyph: HFONT,
    caption: HFONT,
    named: HFONT,
    status: HFONT,
    radical: HFONT,
    letter: HFONT,
    english: HFONT,
    cand_jhenghei: HFONT,
    cand_hkscs: HFONT,
    cand_extb: HFONT,
}

impl Fonts {
    fn empty() -> Self {
        Self {
            glyph: HFONT::default(),
            caption: HFONT::default(),
            named: HFONT::default(),
            status: HFONT::default(),
            radical: HFONT::default(),
            letter: HFONT::default(),
            english: HFONT::default(),
            cand_jhenghei: HFONT::default(),
            cand_hkscs: HFONT::default(),
            cand_extb: HFONT::default(),
        }
    }

    unsafe fn delete(&mut self) {
        for font in [
            &mut self.glyph,
            &mut self.caption,
            &mut self.named,
            &mut self.status,
            &mut self.radical,
            &mut self.letter,
            &mut self.english,
            &mut self.cand_jhenghei,
            &mut self.cand_hkscs,
            &mut self.cand_extb,
        ] {
            if handle_nonzero(*font) {
                let _ = DeleteObject(HGDIOBJ(font.0));
                *font = HFONT::default();
            }
        }
    }
}

struct Brushes {
    bg: HBRUSH,
    status: HBRUSH,
    key: HBRUSH,
    hover: HBRUSH,
    pressed: HBRUSH,
    special: HBRUSH,
    english: HBRUSH,
    selected: HBRUSH,
    disabled: HBRUSH,
    border: HPEN,
    /// Outline for controls that act on the window or the target app
    /// (topmost toggle, Backspace, opacity) rather than typing a character.
    control_pen: HPEN,
    /// Outline for keys that act on the 速成 composition (刪碼 / 清除 / 空格 / 符號).
    compose_pen: HPEN,
}

/// Which outline a key gets, so controls never read as plain typing keys.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Accent {
    None,
    Control,
    Compose,
}

const CONTROL_TEXT: COLORREF = rgb(0xF0, 0xC8, 0x78);
const COMPOSE_TEXT: COLORREF = rgb(0x8E, 0xC8, 0xFF);
const HOLD_MARK_TEXT: COLORREF = rgb(0x6E, 0x9E, 0x94);

/// Draw a key body; accented keys get a 2px inside outline instead of the
/// near-invisible default border.
unsafe fn draw_key_body(
    hdc: HDC,
    state: &AppState,
    rc: RECT,
    radius: i32,
    brush: HBRUSH,
    accent: Accent,
) {
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let pen = match accent {
        Accent::None => None,
        Accent::Control => Some(state.brushes.control_pen),
        Accent::Compose => Some(state.brushes.compose_pen),
    };
    let old_pen = pen.map(|p| SelectObject(hdc, HGDIOBJ(p.0)));
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
    if let Some(old) = old_pen {
        SelectObject(hdc, old);
    }
    SelectObject(hdc, old_brush);
}

struct AppState {
    /// Visible layout, 速成 / 九方 compositions, overlay, 繁簡 and 數墊.
    ime: ime::Ime,
    symbols_open: bool,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    last_typed_cp437: Option<u8>,
    last_typed: Option<char>,
    opacity: prefs::Opacity,
    topmost: bool,
    latin_shifted: bool,
    tracking: bool,
    /// A numpad commit waiting for the physical key-up of `digit` before it
    /// is typed: (glyph, digit).
    pending_send: Option<(char, u8)>,
    /// The last commit could not be typed (`SendInput` failed).
    send_failed: bool,
    fonts: Fonts,
    brushes: Brushes,
    font_key: (i32, i32, i32, u8),
    /// 速成 layout for the last (width, height, DPI, symbols_open). Rebuilt
    /// only when one of those changes, so hover / hit-test / paint don't allocate.
    simplex_cache: RefCell<Option<(LayoutKey, Rc<SimplexLayout>)>>,
    /// 九方 layout, cached the same way.
    jiufang_cache: RefCell<Option<(LayoutKey, Rc<JiufangLayout>)>>,
    /// Off-screen buffer kept across paints so only dirty rects are redrawn.
    backbuf: Option<BackBuffer>,
}

/// (client width, client height, window DPI, symbols_open). DPI is part of
/// the key because every row height and gap is scaled with `dp`.
type LayoutKey = (i32, i32, u32, bool);

/// Persistent memory DC + bitmap for double buffering. Recreated on resize.
struct BackBuffer {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    w: i32,
    h: i32,
}

impl BackBuffer {
    unsafe fn release(&mut self) {
        SelectObject(self.dc, self.old);
        let _ = DeleteObject(HGDIOBJ(self.bmp.0));
        let _ = DeleteDC(self.dc);
    }
}

impl AppState {
    fn new() -> WinResult<Self> {
        unsafe {
            Ok(Self {
                ime: ime::Ime::new(Mode::Cp437),
                symbols_open: false,
                hover: None,
                pressed: None,
                last_typed_cp437: None,
                last_typed: None,
                pending_send: None,
                send_failed: false,
                opacity: prefs::Opacity::DEFAULT,
                topmost: true,
                latin_shifted: false,
                tracking: false,
                fonts: Fonts::empty(),
                brushes: Brushes {
                    bg: CreateSolidBrush(rgb(0x12, 0x14, 0x18)),
                    status: CreateSolidBrush(rgb(0x0C, 0x0E, 0x12)),
                    key: CreateSolidBrush(rgb(0x2A, 0x2E, 0x38)),
                    hover: CreateSolidBrush(rgb(0x3A, 0x58, 0x88)),
                    pressed: CreateSolidBrush(rgb(0x1E, 0x6E, 0xBE)),
                    special: CreateSolidBrush(rgb(0x34, 0x2C, 0x3C)),
                    english: CreateSolidBrush(rgb(0x1C, 0x2E, 0x34)),
                    selected: CreateSolidBrush(rgb(0x1E, 0x6E, 0xBE)),
                    disabled: CreateSolidBrush(rgb(0x1A, 0x1C, 0x22)),
                    border: CreatePen(PS_SOLID, 1, rgb(0x18, 0x1A, 0x20)),
                    control_pen: CreatePen(PS_INSIDEFRAME, dp(2).max(1), rgb(0xB0, 0x86, 0x3C)),
                    compose_pen: CreatePen(PS_INSIDEFRAME, dp(2).max(1), rgb(0x3E, 0x6E, 0xA8)),
                },
                font_key: (0, 0, 0, 255),
                simplex_cache: RefCell::new(None),
                jiufang_cache: RefCell::new(None),
                backbuf: None,
            })
        }
    }

    fn min_client(&self) -> (i32, i32) {
        match self.ime.mode {
            Mode::Cp437 => (dp(prefs::MIN_CLIENT_W), dp(prefs::MIN_CLIENT_H)),
            Mode::Simplex => (dp(prefs::MIN_CLIENT_W), simplex_min_client_h()),
            Mode::Jiufang => (dp(prefs::MIN_CLIENT_W), jiufang_min_client_h()),
        }
    }

    fn status_h(client_h: i32) -> i32 {
        (client_h / 18).clamp(dp(28), dp(44))
    }

    /// Height of the whole footer: hint row plus control bar.
    fn footer_h(client_h: i32) -> i32 {
        Self::status_h(client_h) + Self::hint_row_h(client_h)
    }

    /// Tall enough for the status font (about 0.48 × status height).
    fn hint_row_h(client_h: i32) -> i32 {
        let status = Self::status_h(client_h) as f32;
        ((status * 0.62).round() as i32).clamp(dp(HINT_ROW_MIN_H), dp(28))
    }

    fn status_rect(client: RECT) -> RECT {
        let status = Self::status_h(client.bottom - client.top);
        RECT {
            left: 0,
            top: client.bottom - status,
            right: client.right,
            bottom: client.bottom,
        }
    }

    /// Vertical extent shared by every bottom-bar control.
    fn bar_y(client: RECT) -> (i32, i32) {
        let status = Self::status_rect(client);
        (status.top + dp(3), status.bottom - dp(3))
    }

    fn bar_rect(client: RECT, left: i32, w: i32) -> RECT {
        let (top, bottom) = Self::bar_y(client);
        RECT {
            left,
            top,
            right: left + w,
            bottom,
        }
    }

    /// Three-segment `CP437｜速成｜九方`, always in that order.
    fn mode_segment_rect(client: RECT, which: Mode) -> RECT {
        let i = match which {
            Mode::Cp437 => 0,
            Mode::Simplex => 1,
            Mode::Jiufang => 2,
        };
        let seg_w = dp(MODE_SEG_W);
        Self::bar_rect(client, dp(BAR_PAD) + seg_w * i, seg_w)
    }

    fn opacity_rect(client: RECT) -> RECT {
        let x0 = Self::mode_segment_rect(client, Mode::Jiufang).right + dp(BAR_GAP);
        Self::bar_rect(client, x0, dp(OPACITY_BTN_W))
    }

    /// Two-segment `繁｜簡`, after the opacity button.
    fn output_segment_rect(client: RECT, which: OutputScript) -> RECT {
        let x0 = Self::opacity_rect(client).right + dp(BAR_GAP);
        let seg_w = dp(OUTPUT_SEG_W);
        let i = match which {
            OutputScript::Traditional => 0,
            OutputScript::Simplified => 1,
        };
        Self::bar_rect(client, x0 + seg_w * i, seg_w)
    }

    /// 數墊 capture toggle, after `繁｜簡`.
    fn numpad_toggle_rect(client: RECT) -> RECT {
        let x0 = Self::output_segment_rect(client, OutputScript::Simplified).right + dp(BAR_GAP);
        Self::bar_rect(client, x0, dp(NUMPAD_BTN_W))
    }

    /// Wide Space on the right; never narrower than `SPACE_BAR_MIN_W`.
    fn space_bar_rect(client: RECT) -> RECT {
        let status = Self::status_rect(client);
        let x0 = Self::numpad_toggle_rect(client).right + dp(BAR_GAP);
        let avail = (status.right - dp(BAR_PAD) - x0).max(dp(SPACE_BAR_MIN_W));
        let space_w = avail.clamp(dp(SPACE_BAR_MIN_W), dp(SPACE_BAR_MAX_W));
        Self::bar_rect(client, x0, space_w)
    }

    /// Full-width hint row sitting directly above the control bar.
    fn hint_rect(client: RECT) -> RECT {
        let status = Self::status_rect(client);
        RECT {
            left: dp(12),
            top: status.top - Self::hint_row_h(client.bottom - client.top),
            right: (client.right - dp(12)).max(dp(12)),
            bottom: status.top,
        }
    }

    fn footer_rect(client: RECT) -> RECT {
        let status = Self::status_rect(client);
        RECT {
            left: 0,
            top: status.top - Self::hint_row_h(client.bottom - client.top),
            right: client.right,
            bottom: client.bottom,
        }
    }

    fn grid_rect(client: RECT) -> RECT {
        let status = Self::footer_h(client.bottom - client.top);
        let pad = dp(GRID_PAD);
        RECT {
            left: pad,
            top: pad,
            right: (client.right - pad).max(pad + 16),
            bottom: (client.bottom - status - dp(2)).max(pad + 16),
        }
    }

    fn cell_rect(client: RECT, code: u8) -> RECT {
        let grid = Self::grid_rect(client);
        let gw = (grid.right - grid.left).max(COLS);
        let gh = (grid.bottom - grid.top).max(ROWS);
        let col = (code as i32) % COLS;
        let row = (code as i32) / COLS;
        let cw = gw / COLS;
        let ch = gh / ROWS;
        let gap = dp(KEY_GAP);
        let left = grid.left + col * cw + gap / 2;
        let top = grid.top + row * ch + gap / 2;
        RECT {
            left,
            top,
            right: left + cw - gap,
            bottom: top + ch - gap,
        }
    }

    fn candidate_band(client: RECT) -> RECT {
        RECT {
            left: dp(6),
            top: dp(4),
            right: (client.right - dp(6)).max(dp(8)),
            bottom: dp(CANDIDATE_ROW_H - 2),
        }
    }

    fn current_candidates(&self) -> &'static [char] {
        if self.ime.mode != Mode::Simplex || self.symbols_open {
            return &[];
        }
        simplex::candidates(&self.ime.codes)
    }

    fn current_page_items(&self) -> &'static [char] {
        simplex::page_slice(self.current_candidates(), self.ime.page)
    }

    /// Items in the top candidate bar for whichever method owns it.
    fn bar_items(&self) -> &'static [char] {
        if q9_bar(self) {
            self.ime.q9.page_items(q9::active())
        } else {
            self.current_page_items()
        }
    }

    /// The candidate that 空格 would commit, or the hovered one: the
    /// "current pick" for the 繁／簡 no-mapping hint.
    fn current_pick(&self) -> Option<char> {
        let q9_items = || self.ime.q9.page_items(q9::active());
        match self.hover {
            Some(Hit::Candidate(i)) => self.bar_items().get(i as usize).copied(),
            Some(Hit::Q9(k)) if k > 0 && self.ime.q9.code().is_some() => {
                q9_items().get(usize::from(k) - 1).copied()
            }
            _ if self.ime.q9_live() && self.ime.q9.code().is_some() => q9_items().first().copied(),
            _ if self.ime.mode == Mode::Simplex && !self.ime.overlay_open => {
                self.current_page_items().first().copied()
            }
            _ => None,
        }
    }
}

fn candidate_parts(client: RECT) -> (RECT, RECT, [RECT; 9], RECT, RECT) {
    let band = AppState::candidate_band(client);
    let narrow = band.right - band.left < dp(430);
    let page_w = dp(if narrow { 28 } else { 36 });
    let codes_w = dp(if narrow { 78 } else { 100 });
    let brand_w = dp(if narrow { 52 } else { 58 });
    let gap = dp(if narrow { 2 } else { 3 });
    let next = RECT {
        left: band.right - page_w,
        top: band.top,
        right: band.right,
        bottom: band.bottom,
    };
    let prev = RECT {
        left: next.left - gap - page_w,
        top: band.top,
        right: next.left - gap,
        bottom: band.bottom,
    };
    let brand = RECT {
        left: band.left,
        top: band.top,
        right: band.left + brand_w,
        bottom: band.bottom,
    };
    let codes = RECT {
        left: brand.right + gap,
        top: band.top,
        right: brand.right + gap + codes_w,
        bottom: band.bottom,
    };
    let slots_left = codes.right + gap;
    let slots_right = prev.left - gap;
    let avail = (slots_right - slots_left).max(9);
    let slot_span = avail / 9;
    let mut slots = [RECT::default(); 9];
    for i in 0..9 {
        let left = slots_left + i * slot_span;
        slots[i as usize] = RECT {
            left: left + 1,
            top: band.top,
            right: left + slot_span - 1,
            bottom: band.bottom,
        };
    }
    (brand, codes, slots, prev, next)
}

fn centered_keys(row: RECT, count: i32, key_w: i32) -> Vec<RECT> {
    let gap = dp(KEY_GAP);
    let total = count * key_w + (count - 1) * gap;
    let x0 = row.left + ((row.right - row.left) - total).max(0) / 2;
    (0..count)
        .map(|i| RECT {
            left: x0 + i * (key_w + gap),
            top: row.top,
            right: x0 + i * (key_w + gap) + key_w,
            bottom: row.bottom,
        })
        .collect()
}

/// Minimum 速成 client height in physical pixels at the current DPI. Each
/// row is scaled on its own, exactly as `simplex_layout` does, so rounding
/// can never leave the minimum a pixel or two short.
fn simplex_min_client_h() -> i32 {
    let gap = dp(SIMPLEX_GAP);
    let needed = dp(CANDIDATE_ROW_H + 2)
        + dp(AUX_ROW_H)
        + gap
        + dp(AUX_ROW_H)
        + gap
        + dp(LETTER_ROW_MIN_H) * 3
        + gap * 2
        + gap
        + dp(ENGLISH_ROW_MIN_H) * 3
        + gap * 2
        + gap
        + dp(ACTION_ROW_H)
        + dp(2)
        + dp(HINT_ROW_MIN_H)
        + dp(28);
    needed.max(dp(prefs::MIN_CLIENT_H))
}

/// Digit row (`1–9 0` + Bksp) and fullwidth punctuation / nav row, as on the
/// 速成 layout, starting at `top`. Returns the rows and their bottom edge.
type AuxRows = (
    Vec<(char, RECT)>,
    RECT,
    Vec<(char, RECT)>,
    Vec<(NavKey, RECT)>,
    i32,
);

fn aux_rows(left: i32, right: i32, top: i32) -> AuxRows {
    let key_gap = dp(KEY_GAP);
    let row_gap = dp(SIMPLEX_GAP);
    let aux_row_h = dp(AUX_ROW_H);
    let usable = right - left;
    let unit = ((usable - 10 * key_gap) as f32 / 11.6).max(dp(14) as f32);
    let digit_w = unit as i32;
    let bksp_w = ((unit * 1.6) as i32).max(digit_w + dp(8));
    let total = digit_w * 10 + bksp_w + 10 * key_gap;
    let x0 = left + (usable - total).max(0) / 2;
    let row = |x: i32, w: i32, y: i32| RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + aux_row_h,
    };
    let digits: Vec<(char, RECT)> = DIGIT_CHARS
        .iter()
        .copied()
        .enumerate()
        .map(|(i, ch)| (ch, row(x0 + (i as i32) * (digit_w + key_gap), digit_w, top)))
        .collect();
    let backspace = row(x0 + 10 * (digit_w + key_gap), bksp_w, top);
    let y2 = top + aux_row_h + row_gap;
    let aux2 = RECT {
        left,
        top: y2,
        right,
        bottom: y2 + aux_row_h,
    };
    let aux2_w = ((usable - 7 * key_gap) / 8).max(dp(16));
    let keys = centered_keys(aux2, 8, aux2_w);
    let puncts = PUNCT_CHARS
        .iter()
        .copied()
        .zip(keys.iter().copied())
        .collect();
    let navs = NavKey::ALL
        .iter()
        .copied()
        .zip(keys.iter().copied().skip(4))
        .collect();
    (digits, backspace, puncts, navs, aux2.bottom)
}

/// Lowercase English rows filling `top..bottom` (clamped like 速成).
fn english_rows(left: i32, right: i32, top: i32, bottom: i32) -> (Vec<(char, RECT)>, i32) {
    let key_gap = dp(KEY_GAP);
    let row_gap = dp(SIMPLEX_GAP);
    let usable = right - left;
    let avail = (bottom - top).max(1);
    let english_h = ((avail - 2 * row_gap) / 3).clamp(dp(ENGLISH_ROW_MIN_H), dp(ENGLISH_ROW_MAX_H));
    let en_key_w = ((usable - 9 * key_gap) / 10).max(dp(14));
    let unit = en_key_w + key_gap;
    let block_w = en_key_w * 10 + key_gap * 9;
    let x_base = left + (usable - block_w).max(0) / 2;
    let mut english = Vec::new();
    for (i, row_chars) in ENGLISH_ROWS.iter().enumerate() {
        let row_top = top + (i as i32) * (english_h + row_gap);
        let indent = match i {
            1 => unit / 2,
            2 => unit,
            _ => 0,
        };
        for (j, ch) in row_chars.iter().copied().enumerate() {
            let x = x_base + indent + (j as i32) * unit;
            english.push((
                ch,
                RECT {
                    left: x,
                    top: row_top,
                    right: x + en_key_w,
                    bottom: row_top + english_h,
                },
            ));
        }
    }
    (english, english_h)
}

/// Computer-numpad pad: 7 8 9 / 4 5 6 / 1 2 3 and a wide 0 under columns
/// 1–2. `pad[k]` is the rect of key `k`.
fn pad_rects(area: RECT, key_w: i32, row_h: i32) -> [RECT; 10] {
    let gap = dp(KEY_GAP);
    let total_w = key_w * 3 + gap * 2;
    let x0 = area.left + ((area.right - area.left) - total_w).max(0) / 2;
    let mut pad = [RECT::default(); 10];
    for (r, row) in q9::PAD_ROWS.iter().enumerate() {
        for (c, key) in row.iter().enumerate() {
            let left = x0 + (c as i32) * (key_w + gap);
            let top = area.top + (r as i32) * (row_h + gap);
            pad[usize::from(*key)] = RECT {
                left,
                top,
                right: left + key_w,
                bottom: top + row_h,
            };
        }
    }
    let top = area.top + 3 * (row_h + gap);
    pad[0] = RECT {
        left: x0,
        top,
        right: x0 + key_w * 2 + gap,
        bottom: top + row_h,
    };
    pad
}

/// Minimum 九方 client height in physical pixels (rows scaled one by one,
/// as in `jiufang_layout`).
fn jiufang_min_client_h() -> i32 {
    let gap = dp(SIMPLEX_GAP);
    let needed = dp(CANDIDATE_ROW_H + 2)
        + gap
        + dp(Q9_ROW_MIN_H) * 4
        + dp(KEY_GAP) * 3
        + gap
        + dp(AUX_ROW_H) * 2
        + gap
        + gap
        + dp(ENGLISH_ROW_MIN_H) * 3
        + gap * 2
        + gap
        + dp(ACTION_ROW_H)
        + dp(2)
        + dp(HINT_ROW_MIN_H)
        + dp(28);
    needed.max(dp(prefs::MIN_CLIENT_H))
}

/// The 九方 layout: candidate bar on top (shared with 速成), the 3×3 + wide
/// 0 pad, the digit / punctuation / Bksp rows, the compact English rows, and
/// an action row 刪碼 · 清除 · 空格.
struct JiufangLayout {
    pad: [RECT; 10],
    digits: Vec<(char, RECT)>,
    backspace: RECT,
    puncts: Vec<(char, RECT)>,
    navs: Vec<(NavKey, RECT)>,
    english: Vec<(char, RECT)>,
    actions: Vec<(Hit, &'static str, RECT)>,
    pad_h: i32,
    english_h: i32,
}

fn jiufang_layout(client: RECT) -> JiufangLayout {
    let key_gap = dp(KEY_GAP);
    let row_gap = dp(SIMPLEX_GAP);
    let left = dp(GRID_PAD);
    let right = (client.right - dp(GRID_PAD)).max(left + dp(40));
    let usable = right - left;

    let status = AppState::footer_h(client.bottom - client.top);
    let action_bottom = client.bottom - status - dp(2);
    let action_top = action_bottom - dp(ACTION_ROW_H);
    let action_row = RECT {
        left,
        top: action_top,
        right,
        bottom: action_bottom,
    };
    let action_w = ((usable - 2 * key_gap) / 3).clamp(dp(40), dp(72));
    let recs = centered_keys(action_row, 3, action_w);
    let actions = vec![
        (Hit::Delete, "刪碼", recs[0]),
        (Hit::Clear, "清除", recs[1]),
        (Hit::Space, "空格", recs[2]),
    ];

    let pad_top = dp(CANDIDATE_ROW_H + 2) + row_gap;
    let below = dp(AUX_ROW_H) * 2
        + row_gap * 2
        + row_gap
        + dp(ENGLISH_ROW_MIN_H) * 3
        + row_gap * 2
        + row_gap;
    let pad_avail = (action_top - pad_top - below).max(1);
    let pad_h = ((pad_avail - 3 * key_gap) / 4).clamp(dp(Q9_ROW_MIN_H), dp(Q9_ROW_MAX_H));
    let key_w = ((usable - 2 * key_gap) / 3).min(dp(Q9_KEY_MAX_W));
    let pad_area = RECT {
        left,
        top: pad_top,
        right,
        bottom: pad_top + pad_h * 4 + key_gap * 3,
    };
    let pad = pad_rects(pad_area, key_w, pad_h);

    let (digits, backspace, puncts, navs, aux_bottom) =
        aux_rows(left, right, pad_area.bottom + row_gap);
    let (english, english_h) =
        english_rows(left, right, aux_bottom + row_gap, action_top - row_gap);

    JiufangLayout {
        pad,
        digits,
        backspace,
        puncts,
        navs,
        english,
        actions,
        pad_h,
        english_h,
    }
}

/// Focus-free 九方 overlay panel drawn inside the main window over the
/// mid-upper keyboard (CP437 grid or 速成 radicals). It covers the 速成
/// candidate row, and never the hint row or the bottom control bar.
struct OverlayLayout {
    panel: RECT,
    header: RECT,
    pad: [RECT; 10],
    del: RECT,
    clear: RECT,
    space: RECT,
    prev: RECT,
    next: RECT,
}

fn overlay_layout(client: RECT) -> OverlayLayout {
    let pad = dp(OV_PAD);
    let gap = dp(KEY_GAP);
    let footer_top = AppState::footer_rect(client).top;
    let row_h = dp(OV_ROW_H);
    let want_h = pad * 2 + dp(OV_HEADER_H) + gap + row_h * 4 + gap * 3 + gap + dp(OV_ACTION_H);
    let panel = RECT {
        left: dp(4),
        top: dp(4),
        right: (client.right - dp(4)).max(dp(40)),
        bottom: (dp(4) + want_h).min(footer_top - dp(4)),
    };
    let inner_l = panel.left + pad;
    let inner_r = panel.right - pad;
    let header = RECT {
        left: inner_l,
        top: panel.top + pad,
        right: inner_r,
        bottom: panel.top + pad + dp(OV_HEADER_H),
    };
    let action_bottom = panel.bottom - pad;
    let action_top = action_bottom - dp(OV_ACTION_H);
    // Rows shrink if the window is unusually short, never below 20 dip.
    let rows_avail = action_top - gap - (header.bottom + gap) - gap * 3;
    let row_h = (rows_avail / 4).clamp(dp(20), row_h);
    let key_w = ((inner_r - inner_l - 2 * gap) / 3).min(dp(100));
    let pad_area = RECT {
        left: inner_l,
        top: header.bottom + gap,
        right: inner_r,
        bottom: header.bottom + gap + row_h * 4 + gap * 3,
    };
    let keys = pad_rects(pad_area, key_w, row_h);
    let actions = centered_keys(
        RECT {
            left: inner_l,
            top: action_top,
            right: inner_r,
            bottom: action_bottom,
        },
        5,
        ((inner_r - inner_l - 4 * gap) / 5).clamp(dp(30), dp(72)),
    );
    OverlayLayout {
        panel,
        header,
        pad: keys,
        del: actions[0],
        clear: actions[1],
        space: actions[2],
        prev: actions[3],
        next: actions[4],
    }
}

impl OverlayLayout {
    fn hits(&self) -> [(Hit, RECT); 15] {
        let mut out = [(Hit::OvDelete, RECT::default()); 15];
        for k in 0..10u8 {
            out[usize::from(k)] = (Hit::Q9(k), self.pad[usize::from(k)]);
        }
        out[10] = (Hit::OvDelete, self.del);
        out[11] = (Hit::OvClear, self.clear);
        out[12] = (Hit::OvSpace, self.space);
        out[13] = (Hit::OvPrev, self.prev);
        out[14] = (Hit::OvNext, self.next);
        out
    }
}

struct SimplexLayout {
    digits: Vec<(char, RECT)>,
    backspace: RECT,
    puncts: Vec<(char, RECT)>,
    navs: Vec<(NavKey, RECT)>,
    letters: Vec<(char, RECT)>,
    symbols: Vec<(char, RECT, &'static str)>,
    actions: Vec<(Hit, &'static str, RECT)>,
    english: Vec<(char, RECT)>,
    letter_h: i32,
    english_h: i32,
}

fn action_rects(row: RECT) -> Vec<(Hit, &'static str, RECT)> {
    let usable = (row.right - row.left).max(dp(40));
    // Clearly shorter than letter keys (row height is ACTION_ROW_H) and not
    // stretched as wide as the old full-pitch buttons.
    let action_w = ((usable - 3 * dp(KEY_GAP)) / 4).clamp(dp(40), dp(72));
    let recs = centered_keys(row, 4, action_w);
    vec![
        (Hit::Delete, "刪碼", recs[0]),
        (Hit::Clear, "清除", recs[1]),
        (Hit::Space, "空格", recs[2]),
        (Hit::SymbolToggle, "符號", recs[3]),
    ]
}

fn simplex_layout(client: RECT, symbols_open: bool) -> SimplexLayout {
    // Scale every design constant once; the rest of the layout is in
    // physical pixels.
    let key_gap = dp(KEY_GAP);
    let row_gap = dp(SIMPLEX_GAP);
    let aux_row_h = dp(AUX_ROW_H);
    let left = dp(GRID_PAD);
    let right = (client.right - dp(GRID_PAD)).max(left + dp(40));
    let usable = right - left;

    let y = dp(CANDIDATE_ROW_H + 2);
    let num_row = RECT {
        left,
        top: y,
        right,
        bottom: y + aux_row_h,
    };
    // 1–9 0 plus a slightly wider Bksp at the end, like a keyboard.
    let unit = ((usable - 10 * key_gap) as f32 / 11.6).max(dp(14) as f32);
    let digit_w = unit as i32;
    let bksp_w = ((unit * 1.6) as i32).max(digit_w + dp(8));
    let total = digit_w * 10 + bksp_w + 10 * key_gap;
    let x0 = left + (usable - total).max(0) / 2;
    let mut digits = Vec::new();
    for (i, ch) in DIGIT_CHARS.iter().copied().enumerate() {
        let x = x0 + (i as i32) * (digit_w + key_gap);
        digits.push((
            ch,
            RECT {
                left: x,
                top: num_row.top,
                right: x + digit_w,
                bottom: num_row.bottom,
            },
        ));
    }
    let bx = x0 + 10 * (digit_w + key_gap);
    let backspace = RECT {
        left: bx,
        top: num_row.top,
        right: bx + bksp_w,
        bottom: num_row.bottom,
    };

    let y2 = num_row.bottom + row_gap;
    let aux2 = RECT {
        left,
        top: y2,
        right,
        bottom: y2 + aux_row_h,
    };
    let aux2_w = ((usable - 7 * key_gap) / 8).max(dp(16));
    let aux2_keys = centered_keys(aux2, 8, aux2_w);
    let mut puncts = Vec::new();
    for (ch, rc) in PUNCT_CHARS
        .iter()
        .copied()
        .zip(aux2_keys.iter().copied().take(4))
    {
        puncts.push((ch, rc));
    }
    let mut navs = Vec::new();
    for (nav, rc) in NavKey::ALL
        .iter()
        .copied()
        .zip(aux2_keys.iter().copied().skip(4))
    {
        navs.push((nav, rc));
    }

    let status = AppState::footer_h(client.bottom - client.top);
    let action_bottom = client.bottom - status - dp(2);
    let action_top = action_bottom - dp(ACTION_ROW_H);
    let action_row = RECT {
        left,
        top: action_top,
        right,
        bottom: action_bottom,
    };
    let actions = action_rects(action_row);

    let content_top = aux2.bottom + row_gap;
    let content_bottom = action_top - row_gap;
    let avail = (content_bottom - content_top).max(1);

    let mut letters = Vec::new();
    let mut symbols = Vec::new();
    let letter_h;

    if symbols_open {
        let rh = ((avail - 3 * row_gap) / 4).clamp(dp(22), dp(48));
        letter_h = rh;
        let key_w = ((usable - 12 * key_gap) / 13).max(dp(14));
        for (i, row) in simplex::SYMBOL_ROWS.iter().enumerate() {
            let row_rc = RECT {
                left,
                top: content_top + (i as i32) * (rh + row_gap),
                right,
                bottom: content_top + (i as i32) * (rh + row_gap) + rh,
            };
            let recs = centered_keys(row_rc, row.len() as i32, key_w);
            for (sk, rc) in row.iter().zip(recs) {
                symbols.push((sk.glyph, rc, sk.key));
            }
        }
    } else {
        letter_h = ((avail - 2 * row_gap) / 3).clamp(dp(LETTER_ROW_MIN_H), dp(LETTER_ROW_MAX_H));
        let key_w = ((usable - 9 * key_gap) / 10).max(dp(16));
        let rows_chars: [&[char]; 3] = [simplex::ROW_QWERTY, simplex::ROW_ASDF, simplex::ROW_ZXCV];
        for (i, row_chars) in rows_chars.iter().enumerate() {
            let row_rc = RECT {
                left,
                top: content_top + (i as i32) * (letter_h + row_gap),
                right,
                bottom: content_top + (i as i32) * (letter_h + row_gap) + letter_h,
            };
            let recs = centered_keys(row_rc, row_chars.len() as i32, key_w);
            for (ch, rc) in row_chars.iter().copied().zip(recs) {
                letters.push((ch, rc));
            }
        }
    }

    // Occupy the leftover band under the radical / 符號 keyboard; do not
    // move the action row or raise the capped radical rows to close it.
    let last_kb_bottom = if symbols_open {
        symbols
            .iter()
            .map(|(_, rc, _)| rc.bottom)
            .max()
            .unwrap_or(content_top)
    } else {
        letters
            .iter()
            .map(|(_, rc)| rc.bottom)
            .max()
            .unwrap_or(content_top)
    };
    let eng_top = last_kb_bottom + row_gap;
    let eng_bottom = action_top - row_gap;
    let eng_avail = (eng_bottom - eng_top).max(1);
    let english_h =
        ((eng_avail - 2 * row_gap) / 3).clamp(dp(ENGLISH_ROW_MIN_H), dp(ENGLISH_ROW_MAX_H));
    let en_key_w = ((usable - 9 * key_gap) / 10).max(dp(14));
    let unit = en_key_w + key_gap;
    let block_w = en_key_w * 10 + key_gap * 9;
    let x_base = left + (usable - block_w).max(0) / 2;
    let mut english = Vec::new();
    for (i, row_chars) in ENGLISH_ROWS.iter().enumerate() {
        let row_top = eng_top + (i as i32) * (english_h + row_gap);
        let indent = match i {
            1 => unit / 2,
            2 => unit,
            _ => 0,
        };
        for (j, ch) in row_chars.iter().copied().enumerate() {
            let x = x_base + indent + (j as i32) * unit;
            english.push((
                ch,
                RECT {
                    left: x,
                    top: row_top,
                    right: x + en_key_w,
                    bottom: row_top + english_h,
                },
            ));
        }
    }

    SimplexLayout {
        digits,
        backspace,
        puncts,
        navs,
        letters,
        symbols,
        actions,
        english,
        letter_h,
        english_h,
    }
}

impl AppState {
    /// Cached 速成 layout. Keyed by client size and the 符號 toggle, so a
    /// resize or toggle rebuilds it once and every other call is a clone of
    /// an `Rc`.
    fn layout(&self, client: RECT) -> Rc<SimplexLayout> {
        let key = (
            client.right - client.left,
            client.bottom - client.top,
            current_dpi(),
            self.symbols_open,
        );
        let mut cache = self.simplex_cache.borrow_mut();
        if let Some((k, layout)) = cache.as_ref() {
            if *k == key {
                return Rc::clone(layout);
            }
        }
        let layout = Rc::new(simplex_layout(client, self.symbols_open));
        *cache = Some((key, Rc::clone(&layout)));
        layout
    }

    fn jlayout(&self, client: RECT) -> Rc<JiufangLayout> {
        let key = (
            client.right - client.left,
            client.bottom - client.top,
            current_dpi(),
            false,
        );
        let mut cache = self.jiufang_cache.borrow_mut();
        if let Some((k, layout)) = cache.as_ref() {
            if *k == key {
                return Rc::clone(layout);
            }
        }
        let layout = Rc::new(jiufang_layout(client));
        *cache = Some((key, Rc::clone(&layout)));
        layout
    }

    fn invalidate_layout(&self) {
        *self.simplex_cache.borrow_mut() = None;
        *self.jiufang_cache.borrow_mut() = None;
    }

    /// Screen rect a hit target is drawn in, used to repaint just that key.
    fn hit_rect(&self, client: RECT, hit: Hit) -> Option<RECT> {
        let find = |v: &[(char, RECT)], c: char| v.iter().find(|(k, _)| *k == c).map(|(_, r)| *r);
        match hit {
            Hit::Cp437(code) => Some(Self::cell_rect(client, code)),
            Hit::Mode(m) => Some(Self::mode_segment_rect(client, m)),
            Hit::Opacity => Some(Self::opacity_rect(client)),
            Hit::AsciiSpace => Some(Self::space_bar_rect(client)),
            Hit::Output(o) => Some(Self::output_segment_rect(client, o)),
            Hit::NumpadToggle => Some(Self::numpad_toggle_rect(client)),
            Hit::Q9(k) => {
                if overlay_shown(self) {
                    Some(overlay_layout(client).pad[usize::from(k.min(9))])
                } else if self.ime.mode == Mode::Jiufang {
                    Some(self.jlayout(client).pad[usize::from(k.min(9))])
                } else {
                    None
                }
            }
            Hit::OvDelete | Hit::OvClear | Hit::OvSpace | Hit::OvPrev | Hit::OvNext => {
                let ov = overlay_layout(client);
                ov.hits().iter().find(|(h, _)| *h == hit).map(|(_, r)| *r)
            }
            Hit::Brand | Hit::PagePrev | Hit::PageNext | Hit::Candidate(_) => {
                let (brand, _, slots, prev, next) = candidate_parts(client);
                Some(match hit {
                    Hit::Brand => brand,
                    Hit::PagePrev => prev,
                    Hit::PageNext => next,
                    Hit::Candidate(i) => *slots.get(i as usize)?,
                    _ => unreachable!(),
                })
            }
            _ if self.ime.mode == Mode::Jiufang => {
                let l = self.jlayout(client);
                match hit {
                    Hit::Backspace => Some(l.backspace),
                    Hit::Digit(c) => find(&l.digits, c),
                    Hit::Punct(c) => find(&l.puncts, c),
                    Hit::Latin(c) => find(&l.english, c),
                    Hit::Nav(n) => l.navs.iter().find(|(k, _)| *k == n).map(|(_, r)| *r),
                    Hit::Delete | Hit::Clear | Hit::Space => l
                        .actions
                        .iter()
                        .find(|(k, _, _)| *k == hit)
                        .map(|(_, _, r)| *r),
                    _ => None,
                }
            }
            _ if self.ime.mode != Mode::Simplex => None,
            Hit::Backspace => Some(self.layout(client).backspace),
            Hit::Digit(c) => find(&self.layout(client).digits, c),
            Hit::Punct(c) => find(&self.layout(client).puncts, c),
            Hit::Letter(c) => find(&self.layout(client).letters, c),
            Hit::Latin(c) => find(&self.layout(client).english, c),
            Hit::Nav(n) => self
                .layout(client)
                .navs
                .iter()
                .find(|(k, _)| *k == n)
                .map(|(_, r)| *r),
            Hit::Symbol(c) => self
                .layout(client)
                .symbols
                .iter()
                .find(|(k, _, _)| *k == c)
                .map(|(_, r, _)| *r),
            Hit::Delete | Hit::Clear | Hit::Space | Hit::SymbolToggle => self
                .layout(client)
                .actions
                .iter()
                .find(|(k, _, _)| *k == hit)
                .map(|(_, _, r)| *r),
        }
    }

    fn hit_test(&self, client: RECT, x: i32, y: i32) -> Option<Hit> {
        for (m, _) in MODES {
            if pt_in(AppState::mode_segment_rect(client, m), x, y) {
                return Some(Hit::Mode(m));
            }
        }
        if pt_in(AppState::opacity_rect(client), x, y) {
            return Some(Hit::Opacity);
        }
        for (o, _) in OUTPUTS {
            if pt_in(AppState::output_segment_rect(client, o), x, y) {
                return Some(Hit::Output(o));
            }
        }
        if pt_in(AppState::numpad_toggle_rect(client), x, y) {
            return Some(Hit::NumpadToggle);
        }
        if pt_in(AppState::space_bar_rect(client), x, y) {
            return Some(Hit::AsciiSpace);
        }
        if overlay_shown(self) {
            let ov = overlay_layout(client);
            if pt_in(ov.panel, x, y) {
                // Inside the panel only overlay controls answer; the keys
                // underneath are covered.
                return ov
                    .hits()
                    .iter()
                    .find(|(_, r)| pt_in(*r, x, y))
                    .map(|(h, _)| *h);
            }
        }
        if self.ime.mode == Mode::Jiufang {
            let (brand, _, slots, prev, next) = candidate_parts(client);
            if pt_in(brand, x, y) {
                return Some(Hit::Brand);
            }
            if pt_in(prev, x, y) {
                return Some(Hit::PagePrev);
            }
            if pt_in(next, x, y) {
                return Some(Hit::PageNext);
            }
            for (i, rc) in slots.iter().enumerate() {
                if pt_in(*rc, x, y) {
                    return Some(Hit::Candidate(i as u8));
                }
            }
            let l = self.jlayout(client);
            for (k, rc) in l.pad.iter().enumerate() {
                if pt_in(*rc, x, y) {
                    return Some(Hit::Q9(k as u8));
                }
            }
            if pt_in(l.backspace, x, y) {
                return Some(Hit::Backspace);
            }
            let find =
                |v: &[(char, RECT)]| v.iter().find(|(_, r)| pt_in(*r, x, y)).map(|(c, _)| *c);
            if let Some(c) = find(&l.digits) {
                return Some(Hit::Digit(c));
            }
            if let Some(c) = find(&l.puncts) {
                return Some(Hit::Punct(c));
            }
            if let Some((n, _)) = l.navs.iter().find(|(_, r)| pt_in(*r, x, y)) {
                return Some(Hit::Nav(*n));
            }
            if let Some((h, _, _)) = l.actions.iter().find(|(_, _, r)| pt_in(*r, x, y)) {
                return Some(*h);
            }
            if let Some(c) = find(&l.english) {
                return Some(Hit::Latin(c));
            }
            return None;
        }
        if self.ime.mode == Mode::Simplex {
            let (brand, codes, slots, prev, next) = candidate_parts(client);
            let _ = codes;
            if pt_in(brand, x, y) {
                return Some(Hit::Brand);
            }
            if pt_in(prev, x, y) {
                return Some(Hit::PagePrev);
            }
            if pt_in(next, x, y) {
                return Some(Hit::PageNext);
            }
            for (i, rc) in slots.iter().enumerate() {
                if pt_in(*rc, x, y) {
                    return Some(Hit::Candidate(i as u8));
                }
            }
            let layout = self.layout(client);
            if pt_in(layout.backspace, x, y) {
                return Some(Hit::Backspace);
            }
            for (ch, rc) in &layout.digits {
                if pt_in(*rc, x, y) {
                    return Some(Hit::Digit(*ch));
                }
            }
            for (ch, rc) in &layout.puncts {
                if pt_in(*rc, x, y) {
                    return Some(Hit::Punct(*ch));
                }
            }
            for (nav, rc) in &layout.navs {
                if pt_in(*rc, x, y) {
                    return Some(Hit::Nav(*nav));
                }
            }
            for (hit, _, rc) in &layout.actions {
                if pt_in(*rc, x, y) {
                    return Some(*hit);
                }
            }
            if self.symbols_open {
                for (ch, rc, _) in &layout.symbols {
                    if pt_in(*rc, x, y) {
                        let ch = *ch;
                        return Some(Hit::Symbol(ch));
                    }
                }
            } else {
                for (ch, rc) in &layout.letters {
                    if pt_in(*rc, x, y) {
                        return Some(Hit::Letter(*ch));
                    }
                }
            }
            for (ch, rc) in &layout.english {
                if pt_in(*rc, x, y) {
                    return Some(Hit::Latin(*ch));
                }
            }
            return None;
        }
        let grid = Self::grid_rect(client);
        if x < grid.left || y < grid.top || x >= grid.right || y >= grid.bottom {
            return None;
        }
        let gw = (grid.right - grid.left).max(COLS);
        let gh = (grid.bottom - grid.top).max(ROWS);
        let cw = gw / COLS;
        let ch = gh / ROWS;
        if cw <= 0 || ch <= 0 {
            return None;
        }
        let col = (x - grid.left) / cw;
        let row = (y - grid.top) / ch;
        if col < 0 || col >= COLS || row < 0 || row >= ROWS {
            return None;
        }
        Some(Hit::Cp437((row * COLS + col) as u8))
    }

    unsafe fn ensure_fonts(&mut self, client: RECT) {
        let (key_h, english_h) = match self.ime.mode {
            Mode::Cp437 => {
                let grid = Self::grid_rect(client);
                (((grid.bottom - grid.top) / ROWS).max(1), 0)
            }
            Mode::Simplex => {
                let layout = self.layout(client);
                (layout.letter_h.max(1), layout.english_h.max(1))
            }
            Mode::Jiufang => {
                let layout = self.jlayout(client);
                (layout.pad_h.max(1), layout.english_h.max(1))
            }
        };
        let cand_h = dp(CANDIDATE_ROW_H);
        let mode_tag = match (self.ime.mode, self.symbols_open) {
            (Mode::Cp437, _) => 0,
            (Mode::Simplex, false) => 1,
            (Mode::Simplex, true) => 2,
            (Mode::Jiufang, _) => 3,
        };
        // cand_h is DPI-scaled, so a DPI change alone also rebuilds fonts.
        let key = (key_h, cand_h, english_h, mode_tag);
        if key == self.font_key && handle_nonzero(self.fonts.glyph) {
            return;
        }
        self.fonts.delete();
        self.font_key = key;
        let glyph_h = (key_h as f32 * 0.46).round() as i32;
        let caption_h = (key_h as f32 * 0.20).round() as i32;
        let named_h = if self.ime.mode != Mode::Cp437 {
            (dp(ACTION_ROW_H) as f32 * 0.50).round() as i32
        } else {
            (key_h as f32 * 0.26).round() as i32
        };
        let status = Self::status_h(client.bottom - client.top);
        let status_font_h = (status as f32 * 0.48).round() as i32;
        // Keep radicals inside the key: modest ratio and a hard cap, plus a
        // reserved band for the letter so the two never collide.
        let radical_h = (key_h as f32 * 0.34).round() as i32;
        let letter_h = (key_h as f32 * 0.18).round() as i32;
        let cand_font_h = ((cand_h - dp(8)) as f32 * 0.58).round() as i32;
        // Font size clamps are design pixels too.
        let clamp = |v: i32, lo: i32, hi: i32| v.clamp(dp(lo), dp(hi));
        self.fonts.glyph = create_font(
            "Segoe UI Symbol",
            clamp(glyph_h, 11, 72),
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.caption = create_font("Segoe UI", clamp(caption_h, 9, 22), FW_NORMAL.0 as i32);
        self.fonts.named = create_font(
            "Microsoft JhengHei",
            if self.ime.mode != Mode::Cp437 {
                clamp(named_h, 10, 14)
            } else {
                clamp(named_h, 10, 28)
            },
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.status = create_font(
            "Microsoft JhengHei",
            clamp(status_font_h, 12, 20),
            FW_NORMAL.0 as i32,
        );
        self.fonts.radical = create_font(
            "Microsoft JhengHei",
            clamp(radical_h, 11, 22),
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.letter = create_font("Segoe UI", clamp(letter_h, 8, 12), FW_NORMAL.0 as i32);
        let english_font_h = (english_h as f32 * 0.46).round() as i32;
        self.fonts.english = create_font(
            "Segoe UI",
            clamp(english_font_h, 10, 20),
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.cand_jhenghei = create_font(
            "Microsoft JhengHei",
            clamp(cand_font_h, 13, 32),
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.cand_hkscs = create_font(
            "MingLiU_HKSCS",
            clamp(cand_font_h, 13, 32),
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.cand_extb = create_font(
            "MingLiU_HKSCS-ExtB",
            clamp(cand_font_h, 13, 32),
            FW_SEMIBOLD.0 as i32,
        );
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        unsafe {
            if let Some(mut buf) = self.backbuf.take() {
                buf.release();
            }
            self.fonts.delete();
            let objs = [
                HGDIOBJ(self.brushes.bg.0),
                HGDIOBJ(self.brushes.status.0),
                HGDIOBJ(self.brushes.key.0),
                HGDIOBJ(self.brushes.hover.0),
                HGDIOBJ(self.brushes.pressed.0),
                HGDIOBJ(self.brushes.special.0),
                HGDIOBJ(self.brushes.english.0),
                HGDIOBJ(self.brushes.selected.0),
                HGDIOBJ(self.brushes.disabled.0),
                HGDIOBJ(self.brushes.border.0),
                HGDIOBJ(self.brushes.control_pen.0),
                HGDIOBJ(self.brushes.compose_pen.0),
            ];
            for obj in objs {
                if handle_nonzero(obj) {
                    let _ = DeleteObject(obj);
                }
            }
        }
    }
}

unsafe fn create_font(name: &str, height: i32, weight: i32) -> HFONT {
    let face = wide(name);
    CreateFontW(
        -height.max(dp(8)),
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_TT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 as u32) | (FF_DONTCARE.0 as u32),
        PCWSTR(face.as_ptr()),
    )
}

fn client_rect(hwnd: HWND) -> RECT {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    rc
}

fn state_mut(hwnd: HWND) -> Option<&'static mut AppState> {
    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if p == 0 {
        None
    } else {
        Some(unsafe { &mut *(p as *mut AppState) })
    }
}

fn prefs_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("on99KB").join("window.ini"))
}

/// Optional user 九方 table (same format as `data/jiufang-shape.txt`).
fn jiufang_table_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("on99KB").join("jiufang.txt"))
}

fn load_prefs() -> Option<prefs::WindowPrefs> {
    let path = prefs_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    prefs::parse_window_ini(&text)
}

fn save_prefs(hwnd: HWND) {
    if unsafe { IsIconic(hwnd).as_bool() } {
        return;
    }
    // Sizes are stored in 96-DPI design pixels so the window keeps the same
    // apparent size when the scale factor or monitor changes.
    let rc = client_rect(hwnd);
    let w = undp(rc.right - rc.left).max(prefs::MIN_CLIENT_W);
    let h = undp(rc.bottom - rc.top).max(prefs::MIN_CLIENT_H);
    let mut wr = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut wr);
    }
    // `mode` is the visible layout only; the numpad overlay never changes it.
    let (opacity, topmost, mode, output, numpad_capture) = state_mut(hwnd)
        .map(|s| {
            (
                s.opacity,
                s.topmost,
                s.ime.mode,
                s.ime.output,
                s.ime.numpad_capture,
            )
        })
        .unwrap_or((
            prefs::Opacity::DEFAULT,
            true,
            prefs::KeyboardMode::DEFAULT,
            OutputScript::DEFAULT,
            prefs::NUMPAD_CAPTURE_DEFAULT,
        ));
    let Some(mut p) = prefs::sanitize(w, h, Some(wr.left), Some(wr.top), opacity, topmost, mode)
    else {
        return;
    };
    p.output = output;
    p.numpad_capture = numpad_capture;
    let Some(path) = prefs_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, prefs::format_window_ini(&p));
}

fn schedule_save(hwnd: HWND) {
    unsafe {
        let _ = SetTimer(hwnd, SAVE_TIMER_ID, SAVE_DEBOUNCE_MS, None);
    }
}

fn clamp_window_pos(x: i32, y: i32, win_w: i32, win_h: i32) -> (i32, i32) {
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if vw <= 0 || vh <= 0 {
            return (x, y);
        }
        let min_visible = dp(80);
        let nx = x.clamp(vx - win_w + min_visible, vx + vw - min_visible);
        let ny = y.clamp(vy, vy + vh - dp(40));
        let _ = win_h;
        (nx, ny)
    }
}

fn apply_opacity(hwnd: HWND, opacity: prefs::Opacity) {
    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), opacity.alpha(), LWA_ALPHA);
    }
}

fn apply_topmost(hwnd: HWND, topmost: bool) {
    unsafe {
        let insert = if topmost {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
        let _ = SetWindowPos(
            hwnd,
            insert,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

fn topmost_hint(state: &AppState) -> String {
    if state.topmost {
        "置頂：開  ·  點擊取消置頂".to_string()
    } else {
        "置頂：關  ·  點擊重新置頂".to_string()
    }
}

fn kill_latin_timer(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(hwnd, LATIN_HOLD_TIMER_ID);
    }
}

fn cursor_in_client(hwnd: HWND) -> (i32, i32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
        let _ = ScreenToClient(hwnd, &mut pt);
    }
    (pt.x, pt.y)
}

fn wheel_delta(wparam: WPARAM) -> i16 {
    ((wparam.0 as u32) >> 16) as u16 as i16
}

fn simplex_page_prev(state: &mut AppState) {
    if !state.symbols_open && simplex::can_page_prev(state.ime.page) {
        state.ime.page -= 1;
    }
}

fn simplex_page_next(state: &mut AppState) {
    if !state.symbols_open
        && simplex::can_page_next(state.current_candidates().len(), state.ime.page)
    {
        state.ime.page += 1;
    }
}

fn apply_mode_min_size(hwnd: HWND, state: &AppState) {
    let (min_w, min_h) = state.min_client();
    let rc = client_rect(hwnd);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    if w >= min_w && h >= min_h {
        return;
    }
    let (win_w, win_h) = window_size_for_client(w.max(min_w), h.max(min_h), current_dpi());
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND::default(),
            0,
            0,
            win_w,
            win_h,
            SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}

fn set_title(hwnd: HWND, mode: Mode) {
    let title = wide(title_for(mode));
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(title.as_ptr()));
    }
}

/// Carry out what the composition layer asked for.
fn apply_action(state: &mut AppState, action: Action) {
    match action {
        Action::Commit(ch) => {
            // The Ime already reset for the next character (pad back to the
            // feature keys, overlay closed). If the glyph did not reach the
            // target, undo that so the commit is not treated as done.
            if send_unicode(ch).is_ok() {
                state.last_typed = Some(ch);
                state.send_failed = false;
            } else {
                state.ime.restore_failed_commit();
                state.send_failed = true;
            }
        }
        Action::SendBackspace => {
            let _ = send_vk(VK_BACK, false);
        }
        Action::None => {}
    }
}

/// Type a deferred numpad commit now (its key-up arrived, its timer fired,
/// or other input came first and must not overtake it).
fn flush_pending(hwnd: HWND, state: &mut AppState) {
    if let Some((ch, _)) = state.pending_send.take() {
        unsafe {
            let _ = KillTimer(hwnd, SEND_FLUSH_TIMER_ID);
        }
        apply_action(state, Action::Commit(ch));
    }
}

/// Install or remove the keyboard hook to match 數墊, the composition state
/// (physical Backspace 清碼) and the window state.
fn sync_hook(hwnd: HWND, state: &AppState) {
    hook::sync(hwnd, state.ime.numpad_capture, state.ime.bksp_active());
}

/// Full repaint after composition changes, so the pad visibly returns to
/// the feature keys after a commit or 清碼.
fn refresh_after_input(hwnd: HWND, state: &AppState) {
    sync_hook(hwnd, state);
    unsafe {
        let _ = InvalidateRect(hwnd, None, false);
    }
    a11y::notify_reorder(hwnd);
}

fn activate_hit(hwnd: HWND, state: &mut AppState, hit: Hit) {
    state.send_failed = false;
    flush_pending(hwnd, state);
    match hit {
        Hit::Cp437(0) | Hit::Brand => {
            state.topmost = !state.topmost;
            apply_topmost(hwnd, state.topmost);
            schedule_save(hwnd);
        }
        Hit::Cp437(32) => {
            // The Alt+32 cell is labelled BS and sends Backspace, not a
            // space. Same 清碼 policy as Bksp: an open 九方 overlay (or 速成
            // codes underneath) is cleared first; idle it sends VK_BACK.
            let a = state.ime.backspace();
            if a == Action::SendBackspace {
                state.last_typed_cp437 = Some(32);
            }
            apply_action(state, a);
        }
        Hit::Cp437(code) => {
            if let Some(ch) = cp437::glyph(code) {
                let _ = send_unicode(ch);
                state.last_typed_cp437 = Some(code);
                state.last_typed = Some(ch);
            }
        }
        Hit::Mode(mode) => {
            if state.ime.set_mode(mode) {
                state.symbols_open = false;
                state.hover = None;
                state.latin_shifted = false;
                kill_latin_timer(hwnd);
                state.font_key = (0, 0, 0, 255);
                set_title(hwnd, mode);
                apply_mode_min_size(hwnd, state);
                save_prefs(hwnd);
            }
        }
        Hit::Output(o) => {
            if state.ime.output != o {
                // Output script only: no IME / keyboard-layout change, and
                // in-progress codes are untouched.
                state.ime.output = o;
                save_prefs(hwnd);
            }
        }
        Hit::NumpadToggle => {
            state.ime.numpad_capture = !state.ime.numpad_capture;
            sync_hook(hwnd, state);
            save_prefs(hwnd);
        }
        Hit::Letter(ch) => {
            state.ime.simplex_letter(ch);
        }
        Hit::Delete if q9_bar(state) => {
            state.ime.q9_delete();
        }
        Hit::Delete => state.ime.simplex_delete(),
        Hit::Clear if q9_bar(state) => {
            state.ime.q9_clear();
        }
        Hit::Clear => state.ime.simplex_clear(),
        Hit::Space if q9_bar(state) => {
            let a = state.ime.q9_space(q9::active());
            apply_action(state, a);
        }
        Hit::Space => {
            if !state.symbols_open && !state.ime.overlay_open {
                if let Some(ch) = state.current_page_items().first().copied() {
                    let a = state.ime.simplex_commit(ch);
                    apply_action(state, a);
                }
            }
        }
        Hit::Candidate(i) if q9_bar(state) => {
            let a = state.ime.q9_candidate(i as usize, q9::active());
            apply_action(state, a);
        }
        Hit::Candidate(i) => {
            if !state.symbols_open && !state.ime.overlay_open {
                if let Some(ch) = state.current_page_items().get(i as usize).copied() {
                    let a = state.ime.simplex_commit(ch);
                    apply_action(state, a);
                }
            }
        }
        // 九方 上頁／下頁 wrap like key 0 and the wheel; 速成 stops at the ends.
        Hit::PagePrev if q9_bar(state) => state.ime.q9_prev_page(q9::active()),
        Hit::PageNext if q9_bar(state) => state.ime.q9_next_page(q9::active()),
        Hit::PagePrev => simplex_page_prev(state),
        Hit::PageNext => simplex_page_next(state),
        Hit::Q9(k) => {
            let a = state.ime.q9_key(k, q9::active());
            apply_action(state, a);
        }
        Hit::OvDelete => {
            state.ime.q9_delete();
        }
        Hit::OvClear => {
            state.ime.q9_clear();
        }
        Hit::OvSpace => {
            let a = state.ime.q9_space(q9::active());
            apply_action(state, a);
        }
        Hit::OvPrev => state.ime.q9_prev_page(q9::active()),
        Hit::OvNext => state.ime.q9_next_page(q9::active()),
        Hit::SymbolToggle => {
            state.symbols_open = !state.symbols_open;
            state.ime.page = 0;
            state.font_key = (0, 0, 0, 255);
        }
        Hit::Symbol(ch) => {
            let _ = send_unicode(ch);
            state.last_typed = Some(ch);
        }
        Hit::Digit(ch) | Hit::Punct(ch) => {
            let _ = send_unicode(ch);
            state.last_typed = Some(ch);
        }
        Hit::Latin(_) => {
            // English letters are committed from the press / hold timer path
            // so a short press cannot type lowercase and then uppercase.
        }
        Hit::Nav(nav) => {
            let _ = send_vk(nav.vk(), true);
        }
        Hit::Backspace => {
            // 九方 composition / overlay first, then 速成, else VK_BACK.
            let a = state.ime.backspace();
            apply_action(state, a);
        }
        Hit::AsciiSpace => {
            let _ = send_unicode(' ');
            state.last_typed = Some(' ');
        }
        Hit::Opacity => {
            state.opacity = state.opacity.next();
            apply_opacity(hwnd, state.opacity);
            schedule_save(hwnd);
        }
    }
}

pub fn run_or_report() {
    if let Err(err) = run() {
        let text = wide(&format!("on99KB failed to start:\r\n{err}"));
        let title = wide("on99KB");
        unsafe {
            let _ = MessageBoxW(
                None,
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

pub fn run() -> WinResult<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        SetCurrentProcessExplicitAppUserModelID(w!("TimothyLam1017.on99KB"))?;
        // COM (STA) is needed to hand screen readers our IAccessible.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let instance = GetModuleHandleW(None)?;
        let class_name = w!("on99KB.Window");

        let h_icon = LoadImageW(
            instance,
            make_int_resource(ICON_RESOURCE_ID),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE,
        )
        .ok()
        .map(|h| HICON(h.0))
        .unwrap_or_default();
        let sm_x = GetSystemMetrics(SM_CXSMICON);
        let sm_y = GetSystemMetrics(SM_CYSMICON);
        let h_icon_sm = LoadImageW(
            instance,
            make_int_resource(ICON_RESOURCE_ID),
            IMAGE_ICON,
            sm_x,
            sm_y,
            LR_DEFAULTCOLOR,
        )
        .ok()
        .map(|h| HICON(h.0))
        .unwrap_or_default();

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: h_icon,
            hIconSm: h_icon_sm,
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(windows::core::Error::from_win32());
        }

        let saved = load_prefs();
        let topmost = saved.as_ref().map(|p| p.topmost).unwrap_or(true);
        // Create at the 96-DPI size first; the real DPI is only known once
        // the window exists on a monitor, and it is resized below before it
        // is shown.
        let (win_w, win_h) =
            window_size_for_client(prefs::DEFAULT_CLIENT_W, prefs::DEFAULT_CLIENT_H, BASE_DPI);
        let saved_pos = saved.as_ref().and_then(|p| Some((p.x?, p.y?)));
        let (win_x, win_y) = match saved_pos {
            Some((x, y)) => clamp_window_pos(x, y, win_w, win_h),
            None => (CW_USEDEFAULT, CW_USEDEFAULT),
        };

        let start_mode = saved.as_ref().map(|p| p.mode).unwrap_or(Mode::Cp437);
        // 九方 table: a user override in %APPDATA%\on99KB\jiufang.txt
        // replaces the built-in starter table when it has a valid line.
        let override_text = jiufang_table_path().and_then(|p| std::fs::read_to_string(p).ok());
        q9::install(q9::load_table(override_text.as_deref()));
        let title = wide(title_for(start_mode));
        let hwnd = CreateWindowExW(
            app_ex_style(topmost),
            class_name,
            PCWSTR(title.as_ptr()),
            APP_STYLE,
            win_x,
            win_y,
            win_w,
            win_h,
            None,
            None,
            HINSTANCE::from(instance),
            None,
        )?;

        set_current_dpi(GetDpiForWindow(hwnd));
        // First launch (no saved size): the screenshot-measured 480×460
        // client, never the old 900×820 default, scaled to the monitor's DPI.
        // Missing or invalid mode defaults to CP437; otherwise the last mode
        // is restored. Legacy prefs stored physical pixels; convert them.
        let (saved_w, saved_h) = match saved.as_ref() {
            Some(p) if p.dip => (p.width, p.height),
            Some(p) => (undp(p.width), undp(p.height)),
            None => (prefs::DEFAULT_CLIENT_W, prefs::DEFAULT_CLIENT_H),
        };
        let client_w = dp(saved_w.max(prefs::MIN_CLIENT_W));
        let client_h = dp(saved_h.max(prefs::MIN_CLIENT_H));
        let (win_w, win_h) = window_size_for_client(client_w, client_h, current_dpi());
        let mut flags = SWP_NOZORDER | SWP_NOACTIVATE;
        let (mut x, mut y) = (0, 0);
        match saved_pos {
            Some((sx, sy)) => (x, y) = clamp_window_pos(sx, sy, win_w, win_h),
            None => flags |= SWP_NOMOVE,
        }
        let _ = SetWindowPos(hwnd, HWND::default(), x, y, win_w, win_h, flags);

        let mut state = AppState::new()?;
        state.ime.mode = start_mode;
        if let Some(p) = saved.as_ref() {
            state.opacity = p.opacity;
            state.topmost = p.topmost;
            state.ime.output = p.output;
            state.ime.numpad_capture = p.numpad_capture;
        }
        let opacity = state.opacity;
        let topmost = state.topmost;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(state)) as isize);
        apply_opacity(hwnd, opacity);
        apply_topmost(hwnd, topmost);
        if let Some(state) = state_mut(hwnd) {
            apply_mode_min_size(hwnd, state);
        }
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        if let Some(state) = state_mut(hwnd) {
            sync_hook(hwnd, state);
        }

        let mut msg = MSG::default();
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 == 0 || ret.0 == -1 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_GETOBJECT => a11y::on_get_object(hwnd, wparam, lparam)
            .unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam)),
        WM_ERASEBKGND => LRESULT(1),
        WM_GETMINMAXINFO => {
            let info = lparam.0 as *mut MINMAXINFO;
            if !info.is_null() {
                let (min_w, min_h) = state_mut(hwnd)
                    .map(|s| s.min_client())
                    .unwrap_or((dp(prefs::MIN_CLIENT_W), dp(prefs::MIN_CLIENT_H)));
                let (x, y) = window_size_for_client(min_w, min_h, current_dpi());
                (*info).ptMinTrackSize = POINT { x, y };
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // Update the scale before moving: MoveWindow re-enters
            // WM_GETMINMAXINFO / WM_SIZE, which must see the new DPI.
            set_current_dpi((wparam.0 & 0xFFFF) as u32);
            let suggested = lparam.0 as *const RECT;
            if !suggested.is_null() {
                let r = *suggested;
                let _ = MoveWindow(
                    hwnd,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    true,
                );
            }
            if let Some(state) = state_mut(hwnd) {
                state.font_key = (0, 0, 0, 255);
                state.invalidate_layout();
            }
            let _ = InvalidateRect(hwnd, None, false);
            a11y::notify_reorder(hwnd);
            LRESULT(0)
        }
        WM_MOVE => {
            if !IsIconic(hwnd).as_bool() {
                schedule_save(hwnd);
            }
            LRESULT(0)
        }
        WM_SIZE => {
            let minimized = wparam.0 as u32 == SIZE_MINIMIZED;
            if !minimized {
                schedule_save(hwnd);
            }
            if let Some(state) = state_mut(hwnd) {
                if minimized {
                    // Numpad goes back to the target app: type a commit
                    // already made, drop the overlay without committing and
                    // remove the hook.
                    flush_pending(hwnd, state);
                    state.ime.on_hidden();
                }
                sync_hook(hwnd, state);
                state.font_key = (0, 0, 0, 255);
                state.invalidate_layout();
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            save_prefs(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == SAVE_TIMER_ID {
                let _ = KillTimer(hwnd, SAVE_TIMER_ID);
                save_prefs(hwnd);
            } else if wparam.0 == SEND_FLUSH_TIMER_ID {
                let _ = KillTimer(hwnd, SEND_FLUSH_TIMER_ID);
                if let Some(state) = state_mut(hwnd) {
                    flush_pending(hwnd, state);
                    refresh_after_input(hwnd, state);
                }
            } else if wparam.0 == LATIN_HOLD_TIMER_ID {
                kill_latin_timer(hwnd);
                if let Some(state) = state_mut(hwnd) {
                    if let Some(Hit::Latin(ch)) = state.pressed {
                        if !state.latin_shifted {
                            let rc = client_rect(hwnd);
                            let (x, y) = cursor_in_client(hwnd);
                            if state.hit_test(rc, x, y) == Some(Hit::Latin(ch)) {
                                let shifted = latin_shifted_char(ch);
                                let _ = send_unicode(shifted);
                                state.last_typed = Some(shifted);
                                state.latin_shifted = true;
                            }
                        }
                    }
                    invalidate_hits(hwnd, state, &[state.pressed]);
                }
            }
            LRESULT(0)
        }
        WM_APP_NUMPAD => {
            // Posted by the keyboard hook; composition runs here on the UI
            // thread. No focus or activation change.
            if let Some(state) = state_mut(hwnd) {
                // A commit still waiting for its key-up goes first.
                flush_pending(hwnd, state);
                if state.ime.numpad_capture && !IsIconic(hwnd).as_bool() {
                    state.send_failed = false;
                    let d = (wparam.0 as u8).min(9);
                    match state.ime.numpad_digit(d, q9::active()) {
                        // The Ime has already reset (feature keys, overlay
                        // closed) so the next key-down starts a fresh
                        // character. The glyph is typed on this key's
                        // key-up, not while the hook holds the key down.
                        Action::Commit(ch) => {
                            state.pending_send = Some((ch, d));
                            let _ = SetTimer(hwnd, SEND_FLUSH_TIMER_ID, SEND_FLUSH_MS, None);
                        }
                        a => apply_action(state, a),
                    }
                }
                refresh_after_input(hwnd, state);
            }
            LRESULT(0)
        }
        WM_APP_NUMPAD_UP => {
            if let Some(state) = state_mut(hwnd) {
                if matches!(state.pending_send, Some((_, d)) if usize::from(d) == wparam.0) {
                    flush_pending(hwnd, state);
                }
                refresh_after_input(hwnd, state);
            }
            LRESULT(0)
        }
        WM_APP_BACKSPACE => {
            // Physical Backspace taken by the hook because a composition or
            // the overlay was active: 清碼, exactly like on-screen Bksp.
            if let Some(state) = state_mut(hwnd) {
                if wparam.0 == 0 {
                    flush_pending(hwnd, state);
                    state.send_failed = false;
                    // Normally Action::None. If the composition already
                    // ended before this ran, the eaten key becomes a
                    // normal Backspace for the target.
                    let a = state.ime.backspace();
                    apply_action(state, a);
                }
                refresh_after_input(hwnd, state);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if let Some(state) = state_mut(hwnd) {
                // 九方 (layout or overlay) wraps; 速成 does not.
                if state
                    .ime
                    .q9_wheel(i32::from(wheel_delta(wparam)), q9::active())
                {
                    let _ = InvalidateRect(hwnd, None, false);
                    a11y::notify_reorder(hwnd);
                } else if state.ime.mode == Mode::Simplex {
                    let delta = wheel_delta(wparam);
                    if delta > 0 {
                        simplex_page_prev(state);
                    } else if delta < 0 {
                        simplex_page_next(state);
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                    a11y::notify_reorder(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(hwnd, &mut pt);
            let rc = client_rect(hwnd);
            let hit = state_mut(hwnd).and_then(|s| s.hit_test(rc, pt.x, pt.y));
            if hit.is_some() {
                if let Ok(hand) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(hand);
                    return LRESULT(1);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_MOUSEMOVE => {
            let (x, y) = point_from_lparam(lparam);
            let rc = client_rect(hwnd);
            if let Some(state) = state_mut(hwnd) {
                if !state.tracking {
                    state.tracking = true;
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                }
                let hit = state.hit_test(rc, x, y);
                if state.hover != hit {
                    let old = state.hover;
                    state.hover = hit;
                    invalidate_hits(hwnd, state, &[old, hit]);
                    a11y::notify_state(hwnd, state, &[old, hit]);
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if let Some(state) = state_mut(hwnd) {
                state.tracking = false;
                let old = state.hover.take();
                invalidate_hits(hwnd, state, &[old]);
                a11y::notify_state(hwnd, state, &[old]);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point_from_lparam(lparam);
            let rc = client_rect(hwnd);
            if let Some(state) = state_mut(hwnd) {
                kill_latin_timer(hwnd);
                state.latin_shifted = false;
                state.pressed = state.hit_test(rc, x, y);
                if let Some(Hit::Latin(_)) = state.pressed {
                    let _ = SetTimer(hwnd, LATIN_HOLD_TIMER_ID, LATIN_HOLD_MS, None);
                }
                invalidate_hits(hwnd, state, &[state.pressed]);
                a11y::notify_state(hwnd, state, &[state.pressed]);
            }
            let _ = SetCapture(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            kill_latin_timer(hwnd);
            let (x, y) = point_from_lparam(lparam);
            let rc = client_rect(hwnd);
            if let Some(state) = state_mut(hwnd) {
                let pressed = state.pressed.take();
                let already_shifted = state.latin_shifted;
                state.latin_shifted = false;
                let hit = state.hit_test(rc, x, y);
                match pressed {
                    Some(Hit::Latin(_)) if already_shifted => {
                        // Uppercase was sent when the hold timer fired.
                    }
                    Some(Hit::Latin(ch)) if hit == Some(Hit::Latin(ch)) => {
                        let _ = send_unicode(ch);
                        state.last_typed = Some(ch);
                    }
                    Some(Hit::Latin(_)) => {
                        // Released off the key before the hold threshold: type nothing.
                    }
                    Some(p) if hit == Some(p) => {
                        activate_hit(hwnd, state, p);
                    }
                    _ => {}
                }
                // Repaint everything (faces, overlay) and re-sync the hook:
                // a click may have started or ended a composition.
                refresh_after_input(hwnd, state);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            hook::uninstall();
            if let Some(state) = state_mut(hwnd) {
                flush_pending(hwnd, state);
                state.ime.on_hidden();
            }
            let _ = KillTimer(hwnd, SAVE_TIMER_ID);
            kill_latin_timer(hwnd);
            save_prefs(hwnd);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            hook::uninstall();
            let _ = KillTimer(hwnd, SAVE_TIMER_ID);
            kill_latin_timer(hwnd);
            save_prefs(hwnd);
            let p = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if p != 0 {
                drop(Box::from_raw(p as *mut AppState));
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn send_unicode(ch: char) -> WinResult<()> {
    let mut units = [0u16; 2];
    let encoded = ch.encode_utf16(&mut units);
    let mut inputs: Vec<INPUT> = Vec::with_capacity(encoded.len() * 2);
    for &unit in encoded.iter() {
        let ki_down = KEYBDINPUT {
            wVk: VIRTUAL_KEY(0),
            wScan: unit,
            dwFlags: KEYEVENTF_UNICODE,
            time: 0,
            dwExtraInfo: 0,
        };
        let ki_up = KEYBDINPUT {
            wVk: VIRTUAL_KEY(0),
            wScan: unit,
            dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
            time: 0,
            dwExtraInfo: 0,
        };
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: ki_down },
        });
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: ki_up },
        });
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(windows::core::Error::from_win32());
    }
    Ok(())
}

fn send_vk(vk: VIRTUAL_KEY, extended: bool) -> WinResult<()> {
    let scan = unsafe { MapVirtualKeyW(u32::from(vk.0), MAPVK_VK_TO_VSC) } as u16;
    let extra = if extended {
        KEYEVENTF_EXTENDEDKEY
    } else {
        Default::default()
    };
    let ki_down = KEYBDINPUT {
        wVk: vk,
        wScan: scan,
        dwFlags: extra,
        time: 0,
        dwExtraInfo: 0,
    };
    let ki_up = KEYBDINPUT {
        wVk: vk,
        wScan: scan,
        dwFlags: extra | KEYEVENTF_KEYUP,
        time: 0,
        dwExtraInfo: 0,
    };
    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: ki_down },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: ki_up },
        },
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(windows::core::Error::from_win32());
    }
    Ok(())
}

unsafe fn font_has_glyph(hdc: HDC, font: HFONT, ch: char) -> bool {
    if !handle_nonzero(font) || ch > '\u{FFFF}' {
        return false;
    }
    let old = SelectObject(hdc, HGDIOBJ(font.0));
    let buf = [ch as u16, 0];
    let mut idx: u16 = 0xFFFF;
    let ok = GetGlyphIndicesW(
        hdc,
        PCWSTR(buf.as_ptr()),
        1,
        &mut idx,
        GGI_MARK_NONEXISTING_GLYPHS,
    );
    SelectObject(hdc, old);
    ok != u32::MAX && idx != 0xFFFF
}

unsafe fn pick_cand_font(hdc: HDC, fonts: &Fonts, ch: char) -> HFONT {
    if ch > '\u{FFFF}' && handle_nonzero(fonts.cand_extb) {
        return fonts.cand_extb;
    }
    for font in [fonts.cand_jhenghei, fonts.cand_hkscs, fonts.cand_extb] {
        if font_has_glyph(hdc, font, ch) {
            return font;
        }
    }
    fonts.cand_jhenghei
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc.is_invalid() {
        let _ = EndPaint(hwnd, &ps);
        return;
    }

    let rc = client_rect(hwnd);
    let width = (rc.right - rc.left).max(1);
    let height = (rc.bottom - rc.top).max(1);

    let Some(state) = state_mut(hwnd) else {
        FillRect(hdc, &rc, HBRUSH(GetStockObject(BLACK_BRUSH).0));
        let _ = EndPaint(hwnd, &ps);
        return;
    };

    // Reuse the back buffer between paints; only recreate it on resize.
    let mut dirty = ps.rcPaint;
    if !matches!(&state.backbuf, Some(b) if b.w == width && b.h == height) {
        if let Some(mut old) = state.backbuf.take() {
            old.release();
        }
        let dc = CreateCompatibleDC(hdc);
        let bmp = CreateCompatibleBitmap(hdc, width, height);
        let old = SelectObject(dc, HGDIOBJ(bmp.0));
        state.backbuf = Some(BackBuffer {
            dc,
            bmp,
            old,
            w: width,
            h: height,
        });
        // A fresh bitmap has no prior frame to keep.
        dirty = rc;
    }
    let mem = state.backbuf.as_ref().map(|b| b.dc).unwrap_or(hdc);

    state.ensure_fonts(rc);
    let _ = IntersectClipRect(mem, dirty.left, dirty.top, dirty.right, dirty.bottom);
    FillRect(mem, &dirty, state.brushes.bg);

    let footer_rc = AppState::footer_rect(rc);
    FillRect(mem, &footer_rc, state.brushes.status);

    let old_pen = SelectObject(mem, HGDIOBJ(state.brushes.border.0));
    // The back buffer outlives this paint, so put its original font back at
    // the end; otherwise ensure_fonts could not delete a selected font.
    let old_font = SelectObject(mem, HGDIOBJ(state.fonts.status.0));
    let _ = SetBkMode(mem, TRANSPARENT);

    match state.ime.mode {
        Mode::Cp437 => {
            for code in 0u8..=255 {
                if rects_overlap(AppState::cell_rect(rc, code), dirty) {
                    draw_cp437_key(mem, state, rc, code);
                }
            }
        }
        Mode::Simplex => {
            draw_simplex(mem, state, rc, dirty);
        }
        Mode::Jiufang => {
            draw_jiufang(mem, state, rc, dirty);
        }
    }
    if overlay_shown(state) && rects_overlap(overlay_layout(rc).panel, dirty) {
        draw_overlay(mem, state, rc);
    }

    draw_mode_switch(mem, state, rc);
    draw_opacity_btn(mem, state, rc);
    draw_output_switch(mem, state, rc);
    draw_numpad_toggle(mem, state, rc);
    draw_ascii_space_bar(mem, state, rc);
    draw_hint(mem, state, rc);

    SelectObject(mem, old_font);
    SelectObject(mem, old_pen);
    let _ = SelectClipRgn(mem, HRGN::default());

    let _ = BitBlt(
        hdc,
        dirty.left,
        dirty.top,
        dirty.right - dirty.left,
        dirty.bottom - dirty.top,
        mem,
        dirty.left,
        dirty.top,
        SRCCOPY,
    );
    let _ = EndPaint(hwnd, &ps);
}

/// One segment of a segmented control (mode, 繁｜簡): selected segments are
/// filled blue with bright text, the rest are plain keys.
unsafe fn draw_segment(
    hdc: HDC,
    state: &AppState,
    rc: RECT,
    label: &str,
    hit: Hit,
    selected: bool,
) {
    let brush = if selected {
        state.brushes.selected
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else {
        state.brushes.key
    };
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, dp(6), dp(6));
    SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
    SetTextColor(
        hdc,
        if selected {
            rgb(0xF4, 0xF5, 0xF8)
        } else {
            rgb(0xC8, 0xCD, 0xD8)
        },
    );
    let mut text: Vec<u16> = label.encode_utf16().collect();
    let mut text_rc = RECT {
        left: rc.left + 1,
        top: rc.top,
        right: rc.right - 1,
        bottom: rc.bottom,
    };
    DrawTextW(
        hdc,
        &mut text,
        &mut text_rc,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

unsafe fn draw_output_switch(hdc: HDC, state: &AppState, client: RECT) {
    for (o, label) in OUTPUTS {
        let rc = AppState::output_segment_rect(client, o);
        draw_segment(hdc, state, rc, label, Hit::Output(o), state.ime.output == o);
    }
}

/// 數墊 toggle: filled and bright when capture is on, dark and dim when off.
unsafe fn draw_numpad_toggle(hdc: HDC, state: &AppState, client: RECT) {
    let rc = AppState::numpad_toggle_rect(client);
    let hit = Hit::NumpadToggle;
    let on = state.ime.numpad_capture;
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else if on {
        state.brushes.selected
    } else {
        state.brushes.disabled
    };
    draw_key_body(hdc, state, rc, dp(6), brush, Accent::Control);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
    SetTextColor(
        hdc,
        if on {
            rgb(0xF4, 0xF5, 0xF8)
        } else {
            rgb(0x7A, 0x80, 0x8C)
        },
    );
    let mut text: Vec<u16> = "數墊".encode_utf16().collect();
    let mut text_rc = RECT {
        left: rc.left + 1,
        top: rc.top,
        right: rc.right - 1,
        bottom: rc.bottom,
    };
    DrawTextW(
        hdc,
        &mut text,
        &mut text_rc,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

unsafe fn draw_mode_switch(hdc: HDC, state: &AppState, client: RECT) {
    for (mode, label) in MODES {
        let rc = AppState::mode_segment_rect(client, mode);
        let brush = if state.ime.mode == mode {
            state.brushes.selected
        } else if state.hover == Some(Hit::Mode(mode)) {
            state.brushes.hover
        } else {
            state.brushes.key
        };
        let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
        let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, dp(6), dp(6));
        SelectObject(hdc, old_brush);
        let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
        SetTextColor(
            hdc,
            if state.ime.mode == mode {
                rgb(0xF4, 0xF5, 0xF8)
            } else {
                rgb(0xC8, 0xCD, 0xD8)
            },
        );
        let mut text: Vec<u16> = label.encode_utf16().collect();
        let mut text_rc = RECT {
            left: rc.left + 2,
            top: rc.top,
            right: rc.right - 2,
            bottom: rc.bottom,
        };
        DrawTextW(
            hdc,
            &mut text,
            &mut text_rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }
}

unsafe fn draw_opacity_btn(hdc: HDC, state: &AppState, client: RECT) {
    let rc = AppState::opacity_rect(client);
    let hit = Hit::Opacity;
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else {
        state.brushes.special
    };
    draw_key_body(hdc, state, rc, dp(6), brush, Accent::Control);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
    SetTextColor(hdc, CONTROL_TEXT);
    let mut text: Vec<u16> = state.opacity.label().encode_utf16().collect();
    let mut text_rc = RECT {
        left: rc.left + 1,
        top: rc.top,
        right: rc.right - 1,
        bottom: rc.bottom,
    };
    DrawTextW(
        hdc,
        &mut text,
        &mut text_rc,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

unsafe fn draw_ascii_space_bar(hdc: HDC, state: &AppState, client: RECT) {
    let rc = AppState::space_bar_rect(client);
    if rc.right - rc.left < 4 || rc.bottom - rc.top < 4 {
        return;
    }
    let hit = Hit::AsciiSpace;
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else {
        state.brushes.key
    };
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, dp(6), dp(6));
    SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
    SetTextColor(hdc, rgb(0xF4, 0xF5, 0xF8));
    let mut text: Vec<u16> = "Space".encode_utf16().collect();
    let mut text_rc = RECT {
        left: rc.left + 2,
        top: rc.top,
        right: rc.right - 2,
        bottom: rc.bottom,
    };
    DrawTextW(
        hdc,
        &mut text,
        &mut text_rc,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

unsafe fn draw_hint(hdc: HDC, state: &AppState, client: RECT) {
    let hint = hint_text(state);
    let mut hint_rc = AppState::hint_rect(client);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
    SetTextColor(hdc, rgb(0xC8, 0xCD, 0xD8));
    let mut hint_wide: Vec<u16> = hint.encode_utf16().collect();
    DrawTextW(
        hdc,
        &mut hint_wide,
        &mut hint_rc,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
}

/// Keys the 九方 layout shares with 速成 (their hints stay the same).
fn is_shared_key(hit: Option<Hit>) -> bool {
    matches!(
        hit,
        Some(Hit::Latin(_))
            | Some(Hit::Digit(_))
            | Some(Hit::Punct(_))
            | Some(Hit::Nav(_))
            | Some(Hit::Backspace)
            | Some(Hit::Opacity)
            | Some(Hit::AsciiSpace)
            | Some(Hit::Brand)
            | Some(Hit::Mode(_))
    )
}

fn numpad_hint(state: &AppState) -> String {
    if state.ime.numpad_capture {
        "數墊：開  ·  NumLock 開時實體數字鍵盤 0–9 用作九方，唔會輸入數字  ·  點擊關閉".to_string()
    } else {
        "數墊：關  ·  實體數字鍵盤照常輸入數字  ·  點擊開啟九方數字鍵".to_string()
    }
}

/// Hint for the 九方 pad / overlay: current code, 整體／分體, and what 0 means.
fn q9_hint(state: &AppState) -> String {
    let t = q9::active();
    let c = &state.ime.q9;
    let code = c.code_text();
    match c.stage() {
        q9::Stage::Feature => {
            if state.ime.numpad_capture {
                "九方  ·  揀字首左上角特徵 1–9  ·  數墊開：數字鍵盤 0–9 唔會輸入數字".to_string()
            } else {
                "九方  ·  揀字首左上角特徵 1–9".to_string()
            }
        }
        q9::Stage::Component(f) => format!(
            "九方 {code}  ·  揀「{}」下嘅部件 1–9",
            q9::FEATURES[usize::from(f) - 1]
        ),
        q9::Stage::Third(_, _) => {
            format!("九方 {code}  ·  整體字按 0 確定  ·  分體字按右上角特徵 1–9")
        }
        q9::Stage::Candidates(_) => {
            let class = c.class().map(|k| k.label()).unwrap_or("");
            let n = c.page_count(t);
            if n == 0 {
                format!("九方 {code}  ·  {class}  ·  冇候選字  ·  刪碼 返回")
            } else {
                format!(
                    "九方 {code}  ·  {class}  ·  1–9 選字  ·  0 下頁（{}/{n}）",
                    c.page() + 1
                )
            }
        }
    }
}

fn hint_text(state: &AppState) -> String {
    if state.send_failed {
        return SEND_FAILED_HINT.to_string();
    }
    if let Some(Hit::Output(o)) = state.hover {
        let now = match state.ime.output {
            OutputScript::Traditional => "繁體",
            OutputScript::Simplified => "簡體",
        };
        let target = match o {
            OutputScript::Traditional => "繁",
            OutputScript::Simplified => "簡",
        };
        return format!(
            "輸出：{now}  ·  點擊用{target}  ·  只轉速成／九方中文字，唔改 Windows 輸入法"
        );
    }
    if matches!(state.hover, Some(Hit::NumpadToggle)) {
        return numpad_hint(state);
    }
    // 簡 with a pick that has no simplified form (current or just typed).
    let composing = state.ime.q9_live() || state.ime.mode == Mode::Simplex;
    if composing
        && matches!(
            state.hover,
            None | Some(Hit::Candidate(_)) | Some(Hit::Q9(_))
        )
        && state.ime.no_mapping_hint(state.current_pick())
    {
        return script::NO_MAPPING_HINT.to_string();
    }
    if overlay_shown(state) || (state.ime.mode == Mode::Jiufang && !is_shared_key(state.hover)) {
        return match state.hover {
            Some(Hit::OvDelete) | Some(Hit::Delete) => {
                "刪碼  ·  刪除最後一個九方碼（唔會送到目標視窗）".to_string()
            }
            Some(Hit::OvClear) | Some(Hit::Clear) => "清除  ·  清除九方碼同候選字".to_string(),
            Some(Hit::OvSpace) | Some(Hit::Space) => {
                "空格  ·  上本頁第一個候選字（要輸入空白請用底部 Space）".to_string()
            }
            Some(Hit::OvPrev) | Some(Hit::OvNext) | Some(Hit::PagePrev) | Some(Hit::PageNext) => {
                "九方換頁  ·  最後一頁之後返去第一頁（滾輪一樣）".to_string()
            }
            _ => q9_hint(state),
        };
    }
    if matches!(state.hover, Some(Hit::Opacity)) {
        return format!(
            "透明度 {}  ·  點擊切換 100% / 50% / 25%",
            state.opacity.label()
        );
    }
    if matches!(state.hover, Some(Hit::AsciiSpace)) {
        return "Space  ·  輸入空格到先前視窗".to_string();
    }
    if matches!(state.hover, Some(Hit::Brand) | Some(Hit::Cp437(0))) {
        return topmost_hint(state);
    }
    match state.ime.mode {
        Mode::Cp437 => {
            if let Some(code) = match state.hover {
                Some(Hit::Cp437(c)) => Some(c),
                _ => state.last_typed_cp437,
            } {
                if code == 0 {
                    return topmost_hint(state);
                }
                if code == 32 {
                    let text = "BS  ·  Backspace  ·  刪除先前視窗一個字元";
                    if state.last_typed_cp437 == Some(32)
                        && !matches!(state.hover, Some(Hit::Cp437(_)))
                    {
                        return format!("已輸入  ·  {text}");
                    }
                    return text.to_string();
                }
                let text = cp437::hint_text(code);
                if state.last_typed_cp437 == Some(code)
                    && !matches!(state.hover, Some(Hit::Cp437(_)))
                {
                    format!("已輸入  ·  {text}")
                } else if state.last_typed_cp437 == Some(code)
                    && state.hover == Some(Hit::Cp437(code))
                {
                    format!("{text}  ·  點擊輸入")
                } else {
                    text
                }
            } else {
                "點擊按鍵輸入到先前視窗  ·  唔會搶焦點  ·  拖曳邊緣調整大小".to_string()
            }
        }
        // Shared keys on the 九方 layout (English, digits, nav, Bksp) reuse
        // the 速成 wording below.
        Mode::Simplex | Mode::Jiufang => match state.hover {
            Some(Hit::Letter(ch)) => {
                let rad = simplex::radical_for(ch).unwrap_or('?');
                format!(
                    "速成  ·  {rad} {}  ·  最多兩碼（首尾）",
                    ch.to_ascii_uppercase()
                )
            }
            Some(Hit::Latin(ch)) => {
                let shifted = latin_shifted_char(ch);
                if ch == ',' || ch == '.' {
                    format!("英文 {ch}  ·  短按 {ch}，按住出 {shifted}")
                } else {
                    format!("英文 {ch}  ·  短按小寫，按住大寫")
                }
            }
            Some(Hit::Digit(ch)) | Some(Hit::Punct(ch)) => {
                format!("{ch}  ·  點擊輸入到先前視窗")
            }
            Some(Hit::Nav(nav)) => {
                format!("{}  ·  送到先前視窗", nav.label())
            }
            Some(Hit::Backspace) => {
                if state.ime.q9_active() {
                    "Bksp  ·  取消今次九方取碼".to_string()
                } else if has_simplex_composition(state) {
                    "Bksp  ·  取消今次速成取碼".to_string()
                } else {
                    "Bksp  ·  刪除先前視窗一個字元".to_string()
                }
            }
            Some(Hit::Delete) => "刪碼  ·  刪除最後一個速成碼（唔會送到目標視窗）".to_string(),
            Some(Hit::Clear) => "清除  ·  清除全部速成碼（唔會送到目標視窗）".to_string(),
            Some(Hit::Space) => "空格  ·  上第一個候選字（要輸入空白請用底部 Space）".to_string(),
            Some(Hit::SymbolToggle) => "符號  ·  切換符號鍵盤／速成字根".to_string(),
            _ if state.symbols_open => {
                "符號  ·  同 Ctrl+Alt+, 符號鍵盤一樣  ·  點擊輸入".to_string()
            }
            _ if state.last_typed.is_some() => {
                format!("已輸入  ·  {}", state.last_typed.unwrap())
            }
            _ => "速成  ·  最多兩碼（首尾）  ·  點候選或 Space 上字".to_string(),
        },
    }
}

unsafe fn draw_simplex(hdc: HDC, state: &AppState, client: RECT, dirty: RECT) {
    let (brand_rc, codes_rc, slots, prev, next) = candidate_parts(client);
    draw_brand_chip(hdc, state, brand_rc);
    // Codes / mode label in the candidate bar (so 符號 mode is obvious).
    let label = if state.symbols_open {
        "符號".to_string()
    } else if state.ime.codes.is_empty() {
        "碼".to_string()
    } else {
        let rads: String = state
            .ime
            .codes
            .chars()
            .map(|c| simplex::radical_for(c).unwrap_or(c))
            .collect();
        format!("{rads}  {}", state.ime.codes.to_ascii_uppercase())
    };
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
    SetTextColor(hdc, rgb(0x8E, 0xC8, 0xFF));
    let mut label_wide: Vec<u16> = label.encode_utf16().collect();
    let mut label_rc = codes_rc;
    DrawTextW(
        hdc,
        &mut label_wide,
        &mut label_rc,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );

    let items = if state.symbols_open {
        &[] as &[char]
    } else {
        state.current_page_items()
    };
    for (i, slot) in slots.iter().enumerate() {
        // In 簡 the slot shows the glyph that will be typed.
        let ch = items.get(i).copied().map(|c| state.ime.display(c));
        let hit = Hit::Candidate(i as u8);
        let has = ch.is_some();
        let brush = if has && state.pressed == Some(hit) {
            state.brushes.pressed
        } else if has && state.hover == Some(hit) {
            state.brushes.hover
        } else {
            state.brushes.key
        };
        let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
        let _ = RoundRect(
            hdc,
            slot.left,
            slot.top,
            slot.right,
            slot.bottom,
            dp(6),
            dp(6),
        );
        SelectObject(hdc, old_brush);
        if let Some(ch) = ch {
            let font = pick_cand_font(hdc, &state.fonts, ch);
            let _ = SelectObject(hdc, HGDIOBJ(font.0));
            SetTextColor(hdc, rgb(0xF4, 0xF5, 0xF8));
            let mut s: Vec<u16> = utf16_char(ch);
            let mut tr = RECT {
                left: slot.left + 1,
                top: slot.top,
                right: slot.right - 1,
                bottom: slot.bottom,
            };
            DrawTextW(hdc, &mut s, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
    }

    let can_prev = !state.symbols_open && simplex::can_page_prev(state.ime.page);
    let can_next = !state.symbols_open
        && simplex::can_page_next(state.current_candidates().len(), state.ime.page);
    let narrow = client.right - client.left < dp(430);
    let prev_label = if narrow { "◀" } else { "上" };
    let next_label = if narrow { "▶" } else { "下" };
    draw_page_btn(hdc, state, prev, Hit::PagePrev, prev_label, can_prev);
    draw_page_btn(hdc, state, next, Hit::PageNext, next_label, can_next);

    // Skip keys outside the dirty rect: a hover repaint touches 1–2 keys,
    // not the ~70 on the 速成 keyboard.
    let vis = |rc: RECT| rects_overlap(rc, dirty);
    let layout = state.layout(client);
    for (ch, rc) in layout.digits.iter().filter(|(_, rc)| vis(*rc)) {
        draw_labeled_key(hdc, state, *rc, &ch.to_string(), Hit::Digit(*ch), false);
    }
    if vis(layout.backspace) {
        draw_labeled_key(hdc, state, layout.backspace, "Bksp", Hit::Backspace, true);
    }
    for (ch, rc) in layout.puncts.iter().filter(|(_, rc)| vis(*rc)) {
        draw_labeled_key(hdc, state, *rc, &ch.to_string(), Hit::Punct(*ch), false);
    }
    for (nav, rc) in layout.navs.iter().filter(|(_, rc)| vis(*rc)) {
        draw_labeled_key(hdc, state, *rc, nav.label(), Hit::Nav(*nav), false);
    }
    if state.symbols_open {
        for (ch, rc, keycap) in layout.symbols.iter().filter(|(_, rc, _)| vis(*rc)) {
            draw_symbol_key(hdc, state, *rc, *ch, keycap, Hit::Symbol(*ch));
        }
    } else {
        for (ch, rc) in layout.letters.iter().filter(|(_, rc)| vis(*rc)) {
            draw_letter_key(hdc, state, *rc, *ch);
        }
    }
    for (ch, rc) in layout.english.iter().filter(|(_, rc)| vis(*rc)) {
        draw_english_key(hdc, state, *rc, *ch);
    }
    for (hit, label, rc) in layout.actions.iter().filter(|(_, _, rc)| vis(*rc)) {
        draw_action_key(hdc, state, *rc, label, *hit);
    }
}

unsafe fn draw_labeled_key(
    hdc: HDC,
    state: &AppState,
    rc: RECT,
    label: &str,
    hit: Hit,
    special: bool,
) {
    if rc.right - rc.left < 4 || rc.bottom - rc.top < 4 {
        return;
    }
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else if special {
        state.brushes.special
    } else {
        state.brushes.key
    };
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(dp(2), dp(8));
    let accent = if special {
        Accent::Control
    } else {
        Accent::None
    };
    draw_key_body(hdc, state, rc, radius, brush, accent);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
    SetTextColor(
        hdc,
        if special {
            CONTROL_TEXT
        } else {
            rgb(0xF4, 0xF5, 0xF8)
        },
    );
    let mut text: Vec<u16> = label.encode_utf16().collect();
    let mut tr = RECT {
        left: rc.left + 1,
        top: rc.top,
        right: rc.right - 1,
        bottom: rc.bottom,
    };
    DrawTextW(
        hdc,
        &mut text,
        &mut tr,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
}

unsafe fn draw_page_btn(
    hdc: HDC,
    state: &AppState,
    rc: RECT,
    hit: Hit,
    label: &str,
    enabled: bool,
) {
    let brush = if !enabled {
        state.brushes.disabled
    } else if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else {
        state.brushes.key
    };
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, dp(6), dp(6));
    SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
    SetTextColor(
        hdc,
        if enabled {
            rgb(0xE8, 0xE4, 0xC8)
        } else {
            rgb(0x5A, 0x60, 0x70)
        },
    );
    let mut text: Vec<u16> = label.encode_utf16().collect();
    let mut tr = rc;
    DrawTextW(
        hdc,
        &mut text,
        &mut tr,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

unsafe fn draw_brand_chip(hdc: HDC, state: &AppState, rc: RECT) {
    if rc.right - rc.left < 4 || rc.bottom - rc.top < 4 {
        return;
    }
    let hit = Hit::Brand;
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else if state.topmost {
        state.brushes.selected
    } else {
        state.brushes.disabled
    };
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(dp(2), dp(8));
    draw_key_body(hdc, state, rc, radius, brush, Accent::Control);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.named.0));
    SetTextColor(
        hdc,
        if state.topmost {
            rgb(0x8E, 0xC8, 0xFF)
        } else {
            rgb(0x7A, 0x80, 0x8C)
        },
    );
    let mut text: Vec<u16> = "on99KB".encode_utf16().collect();
    let mut text_rc = RECT {
        left: rc.left + 1,
        top: rc.top + 1,
        right: rc.right - 1,
        bottom: rc.bottom - 1,
    };
    DrawTextW(
        hdc,
        &mut text,
        &mut text_rc,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
}

unsafe fn draw_english_key(hdc: HDC, state: &AppState, rc: RECT, letter: char) {
    if rc.right - rc.left < 4 || rc.bottom - rc.top < 4 {
        return;
    }
    let hit = Hit::Latin(letter);
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else {
        state.brushes.english
    };
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(dp(2), dp(8));
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
    SelectObject(hdc, old_brush);
    let shifted = state.latin_shifted && state.pressed == Some(Hit::Latin(letter));
    let glyph = if shifted {
        latin_shifted_char(letter)
    } else {
        letter
    };
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.english.0));
    SetTextColor(
        hdc,
        if shifted {
            rgb(0xFF, 0xF4, 0xC8)
        } else {
            rgb(0xB8, 0xE4, 0xD8)
        },
    );
    let mut text = utf16_char(glyph);
    let mut tr = RECT {
        left: rc.left + 1,
        top: rc.top,
        right: rc.right - 1,
        bottom: rc.bottom,
    };
    DrawTextW(
        hdc,
        &mut text,
        &mut tr,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    // Keycap-style corner mark: shows what a long press types, so
    // hold-for-capitals is discoverable without reading the status bar.
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    if !shifted && w >= dp(24) && h >= dp(20) {
        let _ = SelectObject(hdc, HGDIOBJ(state.fonts.letter.0));
        SetTextColor(hdc, HOLD_MARK_TEXT);
        let mut mark = utf16_char(latin_shifted_char(letter));
        let mut mr = RECT {
            left: rc.left + w / 2,
            top: rc.top + 1,
            right: rc.right - 3,
            bottom: rc.top + h / 2,
        };
        DrawTextW(hdc, &mut mark, &mut mr, DT_RIGHT | DT_TOP | DT_SINGLELINE);
    }
}

unsafe fn draw_letter_key(hdc: HDC, state: &AppState, rc: RECT, letter: char) {
    if rc.right - rc.left < 4 || rc.bottom - rc.top < 4 {
        return;
    }
    let hit = Hit::Letter(letter);
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else {
        state.brushes.key
    };
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(dp(2), dp(10));
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
    SelectObject(hdc, old_brush);

    let rad = simplex::radical_for(letter).unwrap_or(letter);
    let cap = letter.to_ascii_uppercase();
    let h = rc.bottom - rc.top;
    let w = rc.right - rc.left;
    // Reserved letter band + inner padding so the radical never clips the
    // key edge or collides with the Latin letter.
    let pad = dp(3).max((h.min(w) / 16).min(dp(5)));
    let letter_band = (h * 30 / 100).clamp(dp(11), dp(16));
    let mut rad_wide: Vec<u16> = utf16_char(rad);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.radical.0));
    SetTextColor(hdc, rgb(0xF4, 0xF5, 0xF8));
    let mut rad_rc = RECT {
        left: rc.left + pad,
        top: rc.top + pad,
        right: rc.right - pad,
        bottom: (rc.bottom - letter_band).max(rc.top + pad + dp(8)),
    };
    DrawTextW(
        hdc,
        &mut rad_wide,
        &mut rad_rc,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    let mut cap_wide: Vec<u16> = utf16_char(cap);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.letter.0));
    SetTextColor(hdc, rgb(0x9A, 0xA4, 0xB8));
    let mut cap_rc = RECT {
        left: rc.left + 1,
        top: rad_rc.bottom,
        right: rc.right - 1,
        bottom: rc.bottom - 1,
    };
    DrawTextW(
        hdc,
        &mut cap_wide,
        &mut cap_rc,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

unsafe fn draw_symbol_key(hdc: HDC, state: &AppState, rc: RECT, ch: char, keycap: &str, hit: Hit) {
    if rc.right - rc.left < 4 || rc.bottom - rc.top < 4 {
        return;
    }
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else {
        state.brushes.key
    };
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(dp(2), dp(8));
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
    SelectObject(hdc, old_brush);

    let h = rc.bottom - rc.top;
    let show_cap = h >= dp(26);
    let font = pick_cand_font(hdc, &state.fonts, ch);
    let _ = SelectObject(hdc, HGDIOBJ(font.0));
    SetTextColor(hdc, rgb(0xF4, 0xF5, 0xF8));
    let mut s: Vec<u16> = utf16_char(ch);
    let mut glyph_rc = RECT {
        left: rc.left + 1,
        top: rc.top + 1,
        right: rc.right - 1,
        bottom: if show_cap {
            rc.bottom - (h / 3).max(dp(10))
        } else {
            rc.bottom - 1
        },
    };
    DrawTextW(
        hdc,
        &mut s,
        &mut glyph_rc,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    if show_cap {
        let cap = if keycap == "\\" { "\\" } else { keycap };
        let mut cap_wide: Vec<u16> = cap.encode_utf16().collect();
        let _ = SelectObject(hdc, HGDIOBJ(state.fonts.letter.0));
        SetTextColor(hdc, rgb(0x9A, 0xA4, 0xB8));
        let mut cap_rc = RECT {
            left: rc.left + 1,
            top: glyph_rc.bottom - 1,
            right: rc.right - 1,
            bottom: rc.bottom - 1,
        };
        DrawTextW(
            hdc,
            &mut cap_wide,
            &mut cap_rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }
}

unsafe fn draw_action_key(hdc: HDC, state: &AppState, rc: RECT, label: &str, hit: Hit) {
    let selected = hit == Hit::SymbolToggle && state.symbols_open;
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if selected {
        state.brushes.selected
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else {
        state.brushes.special
    };
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(dp(2), dp(10));
    draw_key_body(hdc, state, rc, radius, brush, Accent::Compose);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.named.0));
    SetTextColor(hdc, COMPOSE_TEXT);
    let mut text: Vec<u16> = label.encode_utf16().collect();
    let mut tr = RECT {
        left: rc.left + 2,
        top: rc.top,
        right: rc.right - 2,
        bottom: rc.bottom,
    };
    DrawTextW(
        hdc,
        &mut text,
        &mut tr,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
}

unsafe fn draw_cp437_key(hdc: HDC, state: &AppState, client: RECT, code: u8) {
    let rc = AppState::cell_rect(client, code);
    if rc.right - rc.left < 4 || rc.bottom - rc.top < 4 {
        return;
    }

    let hit = Hit::Cp437(code);
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else if code == 0 {
        if state.topmost {
            state.brushes.selected
        } else {
            state.brushes.disabled
        }
    } else if cp437::is_named_blank(code) {
        state.brushes.special
    } else {
        state.brushes.key
    };

    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(dp(2), dp(10));
    // Cells 0 (topmost toggle) and 32 (Backspace) are controls, not glyphs.
    let accent = if code == 0 || code == 32 {
        Accent::Control
    } else {
        Accent::None
    };
    draw_key_body(hdc, state, rc, radius, brush, accent);

    let h = rc.bottom - rc.top;
    // Full "Alt+n" caption on big keys; at the default window size keys are
    // only ~22 px tall, so fall back to the bare number in a thin band.
    let has_caption = code != 0 && code != 32;
    let full_caption = has_caption && h >= dp(CP437_FULL_CAPTION_H);
    let show_caption = has_caption && h >= dp(CP437_COMPACT_CAPTION_H);
    let named = code == 0 || code == 32 || cp437::is_named_blank(code);
    let label = if code == 32 {
        "BS".to_string()
    } else {
        cp437::display_label(code)
    };
    let mut label_wide: Vec<u16> = label.encode_utf16().collect();

    if named {
        let _ = SelectObject(hdc, HGDIOBJ(state.fonts.named.0));
        SetTextColor(
            hdc,
            if code == 0 {
                if state.topmost {
                    rgb(0x8E, 0xC8, 0xFF)
                } else {
                    rgb(0x7A, 0x80, 0x8C)
                }
            } else if code == 32 {
                CONTROL_TEXT
            } else {
                rgb(0xE8, 0xE4, 0xC8)
            },
        );
    } else {
        let _ = SelectObject(hdc, HGDIOBJ(state.fonts.glyph.0));
        SetTextColor(hdc, rgb(0xF4, 0xF5, 0xF8));
    }

    if show_caption {
        let mut glyph_rc = RECT {
            left: rc.left + 2,
            top: rc.top + 2,
            right: rc.right - 2,
            bottom: if full_caption {
                rc.bottom - (h / 3).max(dp(12))
            } else {
                rc.bottom - dp(CP437_COMPACT_CAPTION_BAND)
            },
        };
        // The compact band leaves the glyph a little less room than its font
        // height, so let it overflow slightly instead of being clipped.
        let glyph_flags = if full_caption {
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS
        } else {
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOCLIP
        };
        DrawTextW(hdc, &mut label_wide, &mut glyph_rc, glyph_flags);

        let cap = if full_caption {
            format!("Alt+{code}")
        } else {
            code.to_string()
        };
        let mut cap_wide: Vec<u16> = cap.encode_utf16().collect();
        let _ = SelectObject(hdc, HGDIOBJ(state.fonts.caption.0));
        SetTextColor(hdc, rgb(0x9A, 0xA4, 0xB8));
        let mut cap_rc = RECT {
            left: rc.left + 1,
            top: if full_caption {
                glyph_rc.bottom - 1
            } else {
                rc.bottom - dp(CP437_COMPACT_CAPTION_BAND) - 1
            },
            right: rc.right - 1,
            bottom: rc.bottom - 1,
        };
        DrawTextW(
            hdc,
            &mut cap_wide,
            &mut cap_rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    } else {
        let mut text_rc = RECT {
            left: rc.left + 1,
            top: rc.top + 1,
            right: rc.right - 1,
            bottom: rc.bottom - 1,
        };
        DrawTextW(
            hdc,
            &mut label_wide,
            &mut text_rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
    }
}

/// Top candidate bar for 九方: same cells as 速成, showing the current page.
/// The first candidate (what 空格 commits) gets a blue outline. 上頁／下頁
/// (‹ › when narrow) wrap.
unsafe fn draw_q9_candidate_bar(hdc: HDC, state: &AppState, client: RECT) {
    let t = q9::active();
    let (brand_rc, codes_rc, slots, prev, next) = candidate_parts(client);
    draw_brand_chip(hdc, state, brand_rc);
    let c = &state.ime.q9;
    let label = if c.is_active() {
        match c.class() {
            Some(k) => format!("{}  {}", c.code_text(), k.label()),
            None => c.code_text(),
        }
    } else {
        "九方".to_string()
    };
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
    SetTextColor(hdc, COMPOSE_TEXT);
    let mut label_wide: Vec<u16> = label.encode_utf16().collect();
    let mut label_rc = codes_rc;
    DrawTextW(
        hdc,
        &mut label_wide,
        &mut label_rc,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
    let items = c.page_items(t);
    for (i, slot) in slots.iter().enumerate() {
        let ch = items.get(i).copied().map(|x| state.ime.display(x));
        let hit = Hit::Candidate(i as u8);
        let has = ch.is_some();
        let brush = if has && state.pressed == Some(hit) {
            state.brushes.pressed
        } else if has && state.hover == Some(hit) {
            state.brushes.hover
        } else {
            state.brushes.key
        };
        let accent = if i == 0 && has {
            Accent::Compose
        } else {
            Accent::None
        };
        draw_key_body(hdc, state, *slot, dp(6), brush, accent);
        if let Some(ch) = ch {
            draw_glyph_centered(hdc, state, *slot, ch, rgb(0xF4, 0xF5, 0xF8));
        }
    }
    let pages = c.page_count(t);
    let narrow = client.right - client.left < dp(430);
    let (pl, nl) = if narrow {
        ("‹", "›")
    } else {
        ("上頁", "下頁")
    };
    draw_page_btn(hdc, state, prev, Hit::PagePrev, pl, pages > 1);
    draw_page_btn(hdc, state, next, Hit::PageNext, nl, pages > 1);
}

unsafe fn draw_glyph_centered(hdc: HDC, state: &AppState, rc: RECT, ch: char, color: COLORREF) {
    let font = pick_cand_font(hdc, &state.fonts, ch);
    let _ = SelectObject(hdc, HGDIOBJ(font.0));
    SetTextColor(hdc, color);
    let mut s: Vec<u16> = utf16_char(ch);
    let mut tr = RECT {
        left: rc.left + 1,
        top: rc.top,
        right: rc.right - 1,
        bottom: rc.bottom,
    };
    DrawTextW(hdc, &mut s, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
}

/// One 九方 pad key. The face follows the composition state (feature,
/// component, or candidate in numpad position); the key number sits in the
/// top-left corner. Key 0 is wide and reads `0`, `0 確定` or `0 下頁`.
unsafe fn draw_q9_key(hdc: HDC, state: &AppState, rc: RECT, key: u8) {
    if rc.right - rc.left < 4 || rc.bottom - rc.top < 4 {
        return;
    }
    let t = q9::active();
    let hit = Hit::Q9(key);
    let face = state.ime.q9.face(key, t);
    let first_candidate = key == 1 && matches!(face, q9::Face::Candidate(_));
    let brush = if state.pressed == Some(hit) {
        state.brushes.pressed
    } else if state.hover == Some(hit) {
        state.brushes.hover
    } else if key == 0 {
        state.brushes.special
    } else {
        state.brushes.key
    };
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(dp(2), dp(10));
    let accent = if key == 0 || first_candidate {
        Accent::Compose
    } else {
        Accent::None
    };
    draw_key_body(hdc, state, rc, radius, brush, accent);
    // Key number, small, top-left.
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.letter.0));
    SetTextColor(hdc, rgb(0x9A, 0xA4, 0xB8));
    let mut num = utf16_char(char::from(b'0' + key));
    let mut nr = RECT {
        left: rc.left + dp(4),
        top: rc.top + dp(2),
        right: rc.right - dp(2),
        bottom: rc.top + dp(16),
    };
    if key != 0 {
        DrawTextW(hdc, &mut num, &mut nr, DT_TOP | DT_SINGLELINE);
    }
    match face {
        q9::Face::Feature(label) | q9::Face::Component(label) => {
            let ch = label.chars().next().unwrap_or(' ');
            let color = if matches!(face, q9::Face::Feature(_)) {
                rgb(0xE8, 0xE4, 0xC8)
            } else {
                rgb(0xF4, 0xF5, 0xF8)
            };
            draw_glyph_centered(hdc, state, rc, ch, color);
        }
        q9::Face::Candidate(ch) => {
            draw_glyph_centered(hdc, state, rc, state.ime.display(ch), rgb(0xF4, 0xF5, 0xF8));
        }
        q9::Face::Empty => {}
        q9::Face::Zero(word) => {
            // Always labelled `0` and `確定` (dim until a code needs it);
            // `0 下頁` once candidates are visible.
            let label = format!("0  {}", if word.is_empty() { "確定" } else { word });
            let _ = SelectObject(hdc, HGDIOBJ(state.fonts.named.0));
            SetTextColor(
                hdc,
                if word.is_empty() {
                    rgb(0x7A, 0x80, 0x8C)
                } else {
                    COMPOSE_TEXT
                },
            );
            let mut text: Vec<u16> = label.encode_utf16().collect();
            let mut tr = rc;
            DrawTextW(
                hdc,
                &mut text,
                &mut tr,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
    }
}

unsafe fn draw_jiufang(hdc: HDC, state: &AppState, client: RECT, dirty: RECT) {
    let band = AppState::candidate_band(client);
    if rects_overlap(band, dirty) {
        draw_q9_candidate_bar(hdc, state, client);
    }
    let vis = |rc: RECT| rects_overlap(rc, dirty);
    let layout = state.jlayout(client);
    for (k, rc) in layout.pad.iter().enumerate() {
        if vis(*rc) {
            draw_q9_key(hdc, state, *rc, k as u8);
        }
    }
    for (ch, rc) in layout.digits.iter().filter(|(_, rc)| vis(*rc)) {
        draw_labeled_key(hdc, state, *rc, &ch.to_string(), Hit::Digit(*ch), false);
    }
    if vis(layout.backspace) {
        draw_labeled_key(hdc, state, layout.backspace, "Bksp", Hit::Backspace, true);
    }
    for (ch, rc) in layout.puncts.iter().filter(|(_, rc)| vis(*rc)) {
        draw_labeled_key(hdc, state, *rc, &ch.to_string(), Hit::Punct(*ch), false);
    }
    for (nav, rc) in layout.navs.iter().filter(|(_, rc)| vis(*rc)) {
        draw_labeled_key(hdc, state, *rc, nav.label(), Hit::Nav(*nav), false);
    }
    for (ch, rc) in layout.english.iter().filter(|(_, rc)| vis(*rc)) {
        draw_english_key(hdc, state, *rc, *ch);
    }
    for (hit, label, rc) in layout.actions.iter().filter(|(_, _, rc)| vis(*rc)) {
        draw_action_key(hdc, state, *rc, label, *hit);
    }
}

/// The focus-free 九方 overlay over CP437 / 速成: code header, live pad,
/// 刪碼 / 清除 / 空格 and ‹ ›. It is part of this window, so it can never
/// take activation.
unsafe fn draw_overlay(hdc: HDC, state: &AppState, client: RECT) {
    let ov = overlay_layout(client);
    let t = q9::active();
    // Panel body with a blue compose outline so it reads as a temporary layer.
    draw_key_body(
        hdc,
        state,
        ov.panel,
        dp(10),
        state.brushes.status,
        Accent::Compose,
    );
    let c = &state.ime.q9;
    let mut header = String::from("九方");
    if c.is_active() {
        header.push_str("  ");
        header.push_str(&c.code_text());
    }
    if let Some(k) = c.class() {
        header.push_str("  ·  ");
        header.push_str(k.label());
        let n = c.page_count(t);
        if n > 0 {
            header.push_str(&format!("  ·  第 {}/{n} 頁", c.page() + 1));
        } else {
            header.push_str("  ·  冇候選字");
        }
    }
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
    SetTextColor(hdc, COMPOSE_TEXT);
    let mut text: Vec<u16> = header.encode_utf16().collect();
    let mut hr = ov.header;
    DrawTextW(
        hdc,
        &mut text,
        &mut hr,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
    for (k, rc) in ov.pad.iter().enumerate() {
        draw_q9_key(hdc, state, *rc, k as u8);
    }
    let pages = c.page_count(t);
    draw_action_key(hdc, state, ov.del, "刪碼", Hit::OvDelete);
    draw_action_key(hdc, state, ov.clear, "清除", Hit::OvClear);
    draw_action_key(hdc, state, ov.space, "空格", Hit::OvSpace);
    draw_page_btn(hdc, state, ov.prev, Hit::OvPrev, "‹", pages > 1);
    draw_page_btn(hdc, state, ov.next, Hit::OvNext, "›", pages > 1);
}
