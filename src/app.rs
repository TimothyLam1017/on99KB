//! Native Win32 on-screen keyboard.
//!
//! The window is always-on-top, resizable, and created with `WS_EX_NOACTIVATE`
//! so clicks type into the previously focused application instead of stealing
//! focus. Characters are injected with `SendInput` + `KEYEVENTF_UNICODE`.
//!
//! First launch (no saved size) opens at the measured CP437 client size
//! 480×460 — not the old 900×820 default. 速成 reserves extra client height
//! only for the top candidate row. Last window size is remembered; mode is not.

#![cfg(windows)]

use on99kb::{cp437, prefs, simplex};
use std::path::PathBuf;
use windows::core::{w, Result as WinResult, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetGlyphIndicesW,
    GetStockObject, InvalidateRect, RoundRect, ScreenToClient, SelectObject, SetBkMode,
    SetTextColor, BLACK_BRUSH, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL,
    FW_SEMIBOLD, GGI_MARK_NONEXISTING_GLYPHS, HBRUSH, HDC, HFONT, HGDIOBJ, HPEN, OUT_TT_PRECIS,
    PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SendInput, SetCapture, TrackMouseEvent, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, TME_LEAVE, TRACKMOUSEEVENT, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClientRect, GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect,
    IsIconic, KillTimer, LoadCursorW, LoadImageW, MessageBoxW, MoveWindow, PostQuitMessage,
    RegisterClassExW, SetCursor, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
    ShowWindow, TranslateMessage, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA,
    HICON, HWND_TOPMOST, IDC_ARROW, IDC_HAND, IMAGE_ICON, LR_DEFAULTCOLOR, LR_DEFAULTSIZE,
    MA_NOACTIVATE, MB_ICONERROR, MB_OK, MINMAXINFO, MSG, SM_CXSMICON, SM_CXVIRTUALSCREEN,
    SM_CYSMICON, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_DESTROY,
    WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_CAPTION,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
    WS_THICKFRAME,
};

const WM_MOUSELEAVE: u32 = 0x02A3;
const SIZE_MINIMIZED: u32 = 1;
const SAVE_TIMER_ID: usize = 1;
const SAVE_DEBOUNCE_MS: u32 = 400;

const COLS: i32 = 16;
const ROWS: i32 = 16;
const GRID_PAD: i32 = 8;
const KEY_GAP: i32 = 3;
const CANDIDATE_ROW_H: i32 = 52;
const MODE_SEG_W: i32 = 62;
const ICON_RESOURCE_ID: u16 = 1;

const APP_STYLE: WINDOW_STYLE =
    WINDOW_STYLE(WS_POPUP.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0);
const APP_EX_STYLE: WINDOW_EX_STYLE =
    WINDOW_EX_STYLE(WS_EX_NOACTIVATE.0 | WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0);

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16))
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

fn title_for(mode: Mode) -> &'static str {
    match mode {
        Mode::Cp437 => "on99KB  —  CP437 螢幕鍵盤",
        Mode::Simplex => "on99KB  —  速成 螢幕鍵盤",
    }
}

fn make_int_resource(id: u16) -> PCWSTR {
    PCWSTR(id as usize as *const u16)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Cp437,
    Simplex,
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
}

struct Fonts {
    glyph: HFONT,
    caption: HFONT,
    named: HFONT,
    status: HFONT,
    radical: HFONT,
    letter: HFONT,
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
    selected: HBRUSH,
    disabled: HBRUSH,
    border: HPEN,
}

struct AppState {
    mode: Mode,
    symbols_open: bool,
    codes: String,
    page: usize,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    last_typed_cp437: Option<u8>,
    last_typed: Option<char>,
    tracking: bool,
    fonts: Fonts,
    brushes: Brushes,
    font_key: (i32, i32, u8),
}

impl AppState {
    fn new() -> WinResult<Self> {
        unsafe {
            Ok(Self {
                mode: Mode::Cp437,
                symbols_open: false,
                codes: String::new(),
                page: 0,
                hover: None,
                pressed: None,
                last_typed_cp437: None,
                last_typed: None,
                tracking: false,
                fonts: Fonts::empty(),
                brushes: Brushes {
                    bg: CreateSolidBrush(rgb(0x12, 0x14, 0x18)),
                    status: CreateSolidBrush(rgb(0x0C, 0x0E, 0x12)),
                    key: CreateSolidBrush(rgb(0x2A, 0x2E, 0x38)),
                    hover: CreateSolidBrush(rgb(0x3A, 0x58, 0x88)),
                    pressed: CreateSolidBrush(rgb(0x1E, 0x6E, 0xBE)),
                    special: CreateSolidBrush(rgb(0x34, 0x2C, 0x3C)),
                    selected: CreateSolidBrush(rgb(0x1E, 0x6E, 0xBE)),
                    disabled: CreateSolidBrush(rgb(0x1A, 0x1C, 0x22)),
                    border: CreatePen(PS_SOLID, 1, rgb(0x18, 0x1A, 0x20)),
                },
                font_key: (0, 0, 255),
            })
        }
    }

    fn min_client(&self) -> (i32, i32) {
        match self.mode {
            Mode::Cp437 => (prefs::MIN_CLIENT_W, prefs::MIN_CLIENT_H),
            Mode::Simplex => (prefs::MIN_CLIENT_W, prefs::MIN_CLIENT_H + CANDIDATE_ROW_H),
        }
    }

    fn status_h(client_h: i32) -> i32 {
        (client_h / 18).clamp(28, 44)
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

    fn mode_segment_rect(client: RECT, which: Mode) -> RECT {
        let status = Self::status_rect(client);
        let y0 = status.top + 3;
        let y1 = status.bottom - 3;
        let x0 = status.left + 8;
        match which {
            Mode::Cp437 => RECT {
                left: x0,
                top: y0,
                right: x0 + MODE_SEG_W,
                bottom: y1,
            },
            Mode::Simplex => RECT {
                left: x0 + MODE_SEG_W,
                top: y0,
                right: x0 + MODE_SEG_W * 2,
                bottom: y1,
            },
        }
    }

    fn hint_rect(client: RECT) -> RECT {
        let status = Self::status_rect(client);
        RECT {
            left: status.left + 8 + MODE_SEG_W * 2 + 10,
            top: status.top,
            right: status.right - 12,
            bottom: status.bottom,
        }
    }

    fn grid_rect(client: RECT) -> RECT {
        let status = Self::status_h(client.bottom - client.top);
        RECT {
            left: GRID_PAD,
            top: GRID_PAD,
            right: (client.right - GRID_PAD).max(GRID_PAD + 16),
            bottom: (client.bottom - status - 2).max(GRID_PAD + 16),
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
        let left = grid.left + col * cw + KEY_GAP / 2;
        let top = grid.top + row * ch + KEY_GAP / 2;
        RECT {
            left,
            top,
            right: left + cw - KEY_GAP,
            bottom: top + ch - KEY_GAP,
        }
    }

    fn candidate_band(client: RECT) -> RECT {
        RECT {
            left: 6,
            top: 4,
            right: (client.right - 6).max(8),
            bottom: CANDIDATE_ROW_H - 2,
        }
    }

    fn keyboard_rect(client: RECT) -> RECT {
        let status = Self::status_h(client.bottom - client.top);
        RECT {
            left: GRID_PAD,
            top: CANDIDATE_ROW_H + 2,
            right: (client.right - GRID_PAD).max(GRID_PAD + 16),
            bottom: (client.bottom - status - 2).max(CANDIDATE_ROW_H + 18),
        }
    }

    fn current_candidates(&self) -> &'static [char] {
        if self.mode != Mode::Simplex || self.symbols_open {
            return &[];
        }
        simplex::candidates(&self.codes)
    }

    fn current_page_items(&self) -> &'static [char] {
        simplex::page_slice(self.current_candidates(), self.page)
    }
}

fn candidate_parts(client: RECT) -> (RECT, [RECT; 9], RECT, RECT) {
    let band = AppState::candidate_band(client);
    let narrow = band.right - band.left < 430;
    let page_w = if narrow { 28 } else { 36 };
    let codes_w = if narrow { 78 } else { 100 };
    let gap = if narrow { 2 } else { 3 };
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
    let codes = RECT {
        left: band.left,
        top: band.top,
        right: band.left + codes_w,
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
    (codes, slots, prev, next)
}

fn row_rects(area: RECT, rows: i32) -> Vec<RECT> {
    let h = (area.bottom - area.top).max(rows);
    let rh = h / rows;
    (0..rows)
        .map(|i| RECT {
            left: area.left,
            top: area.top + i * rh,
            right: area.right,
            bottom: area.top + (i + 1) * rh - KEY_GAP,
        })
        .collect()
}

fn centered_keys(row: RECT, count: i32, key_w: i32) -> Vec<RECT> {
    let total = count * key_w + (count - 1) * KEY_GAP;
    let x0 = row.left + ((row.right - row.left) - total).max(0) / 2;
    (0..count)
        .map(|i| RECT {
            left: x0 + i * (key_w + KEY_GAP),
            top: row.top,
            right: x0 + i * (key_w + KEY_GAP) + key_w,
            bottom: row.bottom,
        })
        .collect()
}

fn letter_key_rects(kb: RECT) -> (Vec<(char, RECT)>, Vec<(Hit, &'static str, RECT)>) {
    let rows = row_rects(kb, 4);
    let usable_w = (kb.right - kb.left).max(40);
    let key_w = ((usable_w - 9 * KEY_GAP) / 10).max(16);
    let q = centered_keys(rows[0], 10, key_w);
    let a = centered_keys(rows[1], 9, key_w);
    let z = centered_keys(rows[2], 7, key_w);
    let mut letters = Vec::new();
    for (ch, rc) in simplex::ROW_QWERTY.iter().copied().zip(q) {
        letters.push((ch, rc));
    }
    for (ch, rc) in simplex::ROW_ASDF.iter().copied().zip(a) {
        letters.push((ch, rc));
    }
    for (ch, rc) in simplex::ROW_ZXCV.iter().copied().zip(z) {
        letters.push((ch, rc));
    }
    let action_w = ((usable_w - 3 * KEY_GAP) / 4).min(key_w * 2).max(key_w);
    let actions_rc = centered_keys(rows[3], 4, action_w);
    let actions = vec![
        (Hit::Delete, "刪", actions_rc[0]),
        (Hit::Clear, "清除", actions_rc[1]),
        (Hit::Space, "空格", actions_rc[2]),
        (Hit::SymbolToggle, "符號", actions_rc[3]),
    ];
    (letters, actions)
}

fn symbol_key_rects(
    kb: RECT,
) -> (
    Vec<(char, RECT, &'static str)>,
    Vec<(Hit, &'static str, RECT)>,
) {
    let rows = row_rects(kb, 5);
    let usable_w = (kb.right - kb.left).max(40);
    let key_w = ((usable_w - 12 * KEY_GAP) / 13).max(14);
    let mut keys = Vec::new();
    for (i, row) in simplex::SYMBOL_ROWS.iter().enumerate() {
        let recs = centered_keys(rows[i], row.len() as i32, key_w);
        for (sk, rc) in row.iter().zip(recs) {
            keys.push((sk.glyph, rc, sk.key));
        }
    }
    let action_w = ((usable_w - 3 * KEY_GAP) / 4).min(key_w * 3).max(key_w);
    let actions_rc = centered_keys(rows[4], 4, action_w);
    let actions = vec![
        (Hit::Delete, "刪", actions_rc[0]),
        (Hit::Clear, "清除", actions_rc[1]),
        (Hit::Space, "空格", actions_rc[2]),
        (Hit::SymbolToggle, "符號", actions_rc[3]),
    ];
    (keys, actions)
}

impl AppState {
    fn hit_test(&self, client: RECT, x: i32, y: i32) -> Option<Hit> {
        for m in [Mode::Cp437, Mode::Simplex] {
            if pt_in(AppState::mode_segment_rect(client, m), x, y) {
                return Some(Hit::Mode(m));
            }
        }
        if self.mode == Mode::Simplex {
            let (codes, slots, prev, next) = candidate_parts(client);
            let _ = codes;
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
            let kb = Self::keyboard_rect(client);
            if self.symbols_open {
                let (keys, actions) = symbol_key_rects(kb);
                for (hit, _, rc) in &actions {
                    if pt_in(*rc, x, y) {
                        return Some(*hit);
                    }
                }
                for (ch, rc, _) in keys {
                    if pt_in(rc, x, y) {
                        return Some(Hit::Symbol(ch));
                    }
                }
            } else {
                let (letters, actions) = letter_key_rects(kb);
                for (hit, _, rc) in &actions {
                    if pt_in(*rc, x, y) {
                        return Some(*hit);
                    }
                }
                for (ch, rc) in letters {
                    if pt_in(rc, x, y) {
                        return Some(Hit::Letter(ch));
                    }
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
        let key_h = if self.mode == Mode::Cp437 {
            let grid = Self::grid_rect(client);
            ((grid.bottom - grid.top) / ROWS).max(1)
        } else {
            let kb = Self::keyboard_rect(client);
            let rows = if self.symbols_open { 5 } else { 4 };
            ((kb.bottom - kb.top) / rows).max(1)
        };
        let cand_h = CANDIDATE_ROW_H;
        let mode_tag = match (self.mode, self.symbols_open) {
            (Mode::Cp437, _) => 0,
            (Mode::Simplex, false) => 1,
            (Mode::Simplex, true) => 2,
        };
        let key = (key_h, cand_h, mode_tag);
        if key == self.font_key && handle_nonzero(self.fonts.glyph) {
            return;
        }
        self.fonts.delete();
        self.font_key = key;
        let glyph_h = (key_h as f32 * 0.46).round() as i32;
        let caption_h = (key_h as f32 * 0.20).round() as i32;
        let named_h = (key_h as f32 * 0.26).round() as i32;
        let status = Self::status_h(client.bottom - client.top);
        let status_font_h = (status as f32 * 0.48).round() as i32;
        let radical_h = (key_h as f32 * 0.42).round() as i32;
        let letter_h = (key_h as f32 * 0.22).round() as i32;
        let cand_font_h = ((cand_h - 8) as f32 * 0.62).round() as i32;
        self.fonts.glyph = create_font(
            "Segoe UI Symbol",
            glyph_h.clamp(11, 72),
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.caption = create_font("Segoe UI", caption_h.clamp(9, 22), FW_NORMAL.0 as i32);
        self.fonts.named = create_font(
            "Microsoft JhengHei",
            named_h.clamp(10, 28),
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.status = create_font(
            "Microsoft JhengHei",
            status_font_h.clamp(12, 20),
            FW_NORMAL.0 as i32,
        );
        self.fonts.radical = create_font(
            "Microsoft JhengHei",
            radical_h.clamp(11, 48),
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.letter = create_font("Segoe UI", letter_h.clamp(8, 22), FW_NORMAL.0 as i32);
        self.fonts.cand_jhenghei = create_font(
            "Microsoft JhengHei",
            cand_font_h.clamp(14, 40),
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.cand_hkscs = create_font(
            "MingLiU_HKSCS",
            cand_font_h.clamp(14, 40),
            FW_SEMIBOLD.0 as i32,
        );
        self.fonts.cand_extb = create_font(
            "MingLiU_HKSCS-ExtB",
            cand_font_h.clamp(14, 40),
            FW_SEMIBOLD.0 as i32,
        );
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        unsafe {
            self.fonts.delete();
            let objs = [
                HGDIOBJ(self.brushes.bg.0),
                HGDIOBJ(self.brushes.status.0),
                HGDIOBJ(self.brushes.key.0),
                HGDIOBJ(self.brushes.hover.0),
                HGDIOBJ(self.brushes.pressed.0),
                HGDIOBJ(self.brushes.special.0),
                HGDIOBJ(self.brushes.selected.0),
                HGDIOBJ(self.brushes.disabled.0),
                HGDIOBJ(self.brushes.border.0),
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
        -height.max(8),
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

fn load_prefs() -> Option<prefs::WindowPrefs> {
    let path = prefs_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    prefs::parse_window_ini(&text)
}

fn save_prefs(hwnd: HWND) {
    if unsafe { IsIconic(hwnd).as_bool() } {
        return;
    }
    let rc = client_rect(hwnd);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let mut wr = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut wr);
    }
    let Some(p) = prefs::sanitize(w, h, Some(wr.left), Some(wr.top)) else {
        return;
    };
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
        let min_visible = 80;
        let nx = x.clamp(vx - win_w + min_visible, vx + vw - min_visible);
        let ny = y.clamp(vy, vy + vh - 40);
        let _ = win_h;
        (nx, ny)
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
    let new_w = w.max(min_w);
    let new_h = h.max(min_h);
    let mut wr = RECT {
        left: 0,
        top: 0,
        right: new_w,
        bottom: new_h,
    };
    unsafe {
        let _ = AdjustWindowRectEx(&mut wr, APP_STYLE, false, APP_EX_STYLE);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            wr.right - wr.left,
            wr.bottom - wr.top,
            SWP_NOMOVE | SWP_NOACTIVATE,
        );
    }
}

fn set_title(hwnd: HWND, mode: Mode) {
    let title = wide(title_for(mode));
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(title.as_ptr()));
    }
}

fn commit_char(state: &mut AppState, ch: char) {
    let _ = send_unicode(ch);
    state.last_typed = Some(ch);
    state.codes.clear();
    state.page = 0;
}

fn activate_hit(hwnd: HWND, state: &mut AppState, hit: Hit) {
    match hit {
        Hit::Cp437(0) => {}
        Hit::Cp437(code) => {
            if let Some(ch) = cp437::glyph(code) {
                let _ = send_unicode(ch);
                state.last_typed_cp437 = Some(code);
                state.last_typed = Some(ch);
            }
        }
        Hit::Mode(mode) => {
            if state.mode != mode {
                state.mode = mode;
                state.symbols_open = false;
                state.codes.clear();
                state.page = 0;
                state.hover = None;
                state.font_key = (0, 0, 255);
                set_title(hwnd, mode);
                apply_mode_min_size(hwnd, state);
            }
        }
        Hit::Letter(ch) => {
            if simplex::push_code(&mut state.codes, ch) {
                state.page = 0;
            }
        }
        Hit::Delete => {
            if simplex::pop_code(&mut state.codes) {
                state.page = 0;
            }
        }
        Hit::Clear => {
            state.codes.clear();
            state.page = 0;
        }
        Hit::Space => {
            if !state.symbols_open {
                if let Some(ch) = state.current_page_items().first().copied() {
                    commit_char(state, ch);
                }
            }
        }
        Hit::Candidate(i) => {
            if !state.symbols_open {
                if let Some(ch) = state.current_page_items().get(i as usize).copied() {
                    commit_char(state, ch);
                }
            }
        }
        Hit::PagePrev => {
            if !state.symbols_open && simplex::can_page_prev(state.page) {
                state.page -= 1;
            }
        }
        Hit::PageNext => {
            if !state.symbols_open
                && simplex::can_page_next(state.current_candidates().len(), state.page)
            {
                state.page += 1;
            }
        }
        Hit::SymbolToggle => {
            state.symbols_open = !state.symbols_open;
            state.page = 0;
            state.font_key = (0, 0, 255);
        }
        Hit::Symbol(ch) => {
            let _ = send_unicode(ch);
            state.last_typed = Some(ch);
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
        // First launch (no saved size): the screenshot-measured 480×460 client,
        // never the old 900×820 default. Mode always starts as CP437.
        let client_w = saved
            .as_ref()
            .map(|p| p.width)
            .unwrap_or(prefs::DEFAULT_CLIENT_W)
            .max(prefs::MIN_CLIENT_W);
        let client_h = saved
            .as_ref()
            .map(|p| p.height)
            .unwrap_or(prefs::DEFAULT_CLIENT_H)
            .max(prefs::MIN_CLIENT_H);

        let mut wr = RECT {
            left: 0,
            top: 0,
            right: client_w,
            bottom: client_h,
        };
        AdjustWindowRectEx(&mut wr, APP_STYLE, false, APP_EX_STYLE)?;
        let win_w = wr.right - wr.left;
        let win_h = wr.bottom - wr.top;

        let (win_x, win_y) = match saved.as_ref().and_then(|p| Some((p.x?, p.y?))) {
            Some((x, y)) => clamp_window_pos(x, y, win_w, win_h),
            None => (CW_USEDEFAULT, CW_USEDEFAULT),
        };

        let hwnd = CreateWindowExW(
            APP_EX_STYLE,
            class_name,
            w!("on99KB  —  CP437 螢幕鍵盤"),
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

        let boxed = Box::new(AppState::new()?);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(boxed) as isize);

        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

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
        WM_ERASEBKGND => LRESULT(1),
        WM_GETMINMAXINFO => {
            let info = lparam.0 as *mut MINMAXINFO;
            if !info.is_null() {
                let (min_w, min_h) = state_mut(hwnd)
                    .map(|s| s.min_client())
                    .unwrap_or((prefs::MIN_CLIENT_W, prefs::MIN_CLIENT_H));
                let mut wr = RECT {
                    left: 0,
                    top: 0,
                    right: min_w,
                    bottom: min_h,
                };
                let _ = AdjustWindowRectEx(&mut wr, APP_STYLE, false, APP_EX_STYLE);
                (*info).ptMinTrackSize = POINT {
                    x: wr.right - wr.left,
                    y: wr.bottom - wr.top,
                };
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
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
                state.font_key = (0, 0, 255);
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_SIZE => {
            if wparam.0 as u32 != SIZE_MINIMIZED {
                schedule_save(hwnd);
            }
            if let Some(state) = state_mut(hwnd) {
                state.font_key = (0, 0, 255);
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == SAVE_TIMER_ID {
                let _ = KillTimer(hwnd, SAVE_TIMER_ID);
                save_prefs(hwnd);
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
                    state.hover = hit;
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if let Some(state) = state_mut(hwnd) {
                state.tracking = false;
                state.hover = None;
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point_from_lparam(lparam);
            let rc = client_rect(hwnd);
            if let Some(state) = state_mut(hwnd) {
                state.pressed = state.hit_test(rc, x, y);
                let _ = InvalidateRect(hwnd, None, false);
            }
            let _ = SetCapture(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            let (x, y) = point_from_lparam(lparam);
            let rc = client_rect(hwnd);
            if let Some(state) = state_mut(hwnd) {
                let pressed = state.pressed.take();
                let hit = state.hit_test(rc, x, y);
                if let (Some(p), Some(c)) = (pressed, hit) {
                    if p == c {
                        activate_hit(hwnd, state, c);
                    }
                }
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = KillTimer(hwnd, SAVE_TIMER_ID);
            save_prefs(hwnd);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(hwnd, SAVE_TIMER_ID);
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

    let mem = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, width, height);
    let old_bmp = SelectObject(mem, HGDIOBJ(bmp.0));

    if let Some(state) = state_mut(hwnd) {
        state.ensure_fonts(rc);
        FillRect(mem, &rc, state.brushes.bg);

        let status_rc = AppState::status_rect(rc);
        FillRect(mem, &status_rc, state.brushes.status);

        let old_pen = SelectObject(mem, HGDIOBJ(state.brushes.border.0));
        let _ = SetBkMode(mem, TRANSPARENT);

        match state.mode {
            Mode::Cp437 => {
                for code in 0u8..=255 {
                    draw_cp437_key(mem, state, rc, code);
                }
            }
            Mode::Simplex => {
                draw_simplex(mem, state, rc);
            }
        }

        draw_mode_switch(mem, state, rc);
        draw_hint(mem, state, rc);

        SelectObject(mem, old_pen);
    } else {
        FillRect(mem, &rc, HBRUSH(GetStockObject(BLACK_BRUSH).0));
    }

    let _ = BitBlt(hdc, 0, 0, width, height, mem, 0, 0, SRCCOPY);
    SelectObject(mem, old_bmp);
    let _ = DeleteObject(HGDIOBJ(bmp.0));
    let _ = DeleteDC(mem);
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn draw_mode_switch(hdc: HDC, state: &AppState, client: RECT) {
    for (mode, label) in [(Mode::Cp437, "CP437"), (Mode::Simplex, "速成")] {
        let rc = AppState::mode_segment_rect(client, mode);
        let brush = if state.mode == mode {
            state.brushes.selected
        } else if state.hover == Some(Hit::Mode(mode)) {
            state.brushes.hover
        } else {
            state.brushes.key
        };
        let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
        let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, 6, 6);
        SelectObject(hdc, old_brush);
        let _ = SelectObject(hdc, HGDIOBJ(state.fonts.status.0));
        SetTextColor(
            hdc,
            if state.mode == mode {
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

fn hint_text(state: &AppState) -> String {
    match state.mode {
        Mode::Cp437 => {
            if let Some(code) = match state.hover {
                Some(Hit::Cp437(c)) => Some(c),
                _ => state.last_typed_cp437,
            } {
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
                "置頂 · 唔會搶焦點 · 點擊按鍵輸入到先前聚焦嘅視窗 · 拖曳邊緣可調整大小".to_string()
            }
        }
        Mode::Simplex if state.symbols_open => {
            "符號 · 對齊微軟倉頡／速成 Ctrl+Alt+, 符號鍵盤 · 點符號輸入到先前聚焦視窗".to_string()
        }
        Mode::Simplex => {
            if let Some(Hit::Letter(ch)) = state.hover {
                let rad = simplex::radical_for(ch).unwrap_or('?');
                format!(
                    "速成  ·  {rad} {}  ·  最多兩碼（首尾）",
                    ch.to_ascii_uppercase()
                )
            } else if let Some(ch) = state.last_typed {
                format!("已輸入  ·  {ch}")
            } else {
                "速成 · 最多兩碼（首尾）· 點候選或空格上字到先前聚焦視窗".to_string()
            }
        }
    }
}

unsafe fn draw_simplex(hdc: HDC, state: &AppState, client: RECT) {
    let (codes_rc, slots, prev, next) = candidate_parts(client);
    // Codes / mode label in the candidate bar (so 符號 mode is obvious).
    let label = if state.symbols_open {
        "符號".to_string()
    } else if state.codes.is_empty() {
        "碼".to_string()
    } else {
        let rads: String = state
            .codes
            .chars()
            .map(|c| simplex::radical_for(c).unwrap_or(c))
            .collect();
        format!("{rads}  {}", state.codes.to_ascii_uppercase())
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
        let ch = items.get(i).copied();
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
        let _ = RoundRect(hdc, slot.left, slot.top, slot.right, slot.bottom, 6, 6);
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

    let can_prev = !state.symbols_open && simplex::can_page_prev(state.page);
    let can_next =
        !state.symbols_open && simplex::can_page_next(state.current_candidates().len(), state.page);
    let narrow = client.right - client.left < 430;
    let prev_label = if narrow { "◀" } else { "上" };
    let next_label = if narrow { "▶" } else { "下" };
    draw_page_btn(hdc, state, prev, Hit::PagePrev, prev_label, can_prev);
    draw_page_btn(hdc, state, next, Hit::PageNext, next_label, can_next);

    let kb = AppState::keyboard_rect(client);
    if state.symbols_open {
        let (keys, actions) = symbol_key_rects(kb);
        for (ch, rc, keycap) in keys {
            draw_symbol_key(hdc, state, rc, ch, keycap, Hit::Symbol(ch));
        }
        for (hit, label, rc) in actions {
            draw_action_key(hdc, state, rc, label, hit);
        }
    } else {
        let (letters, actions) = letter_key_rects(kb);
        for (ch, rc) in letters {
            draw_letter_key(hdc, state, rc, ch);
        }
        for (hit, label, rc) in actions {
            draw_action_key(hdc, state, rc, label, hit);
        }
    }
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
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, 6, 6);
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
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(2, 10);
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
    SelectObject(hdc, old_brush);

    let rad = simplex::radical_for(letter).unwrap_or(letter);
    let cap = letter.to_ascii_uppercase();
    let h = rc.bottom - rc.top;
    // Always show both radical and letter, even on a tight window.
    let split = if h >= 28 {
        (h as f32 * 0.62) as i32
    } else {
        (h as f32 * 0.58) as i32
    };
    let mut rad_wide: Vec<u16> = utf16_char(rad);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.radical.0));
    SetTextColor(hdc, rgb(0xF4, 0xF5, 0xF8));
    let mut rad_rc = RECT {
        left: rc.left + 1,
        top: rc.top + 1,
        right: rc.right - 1,
        bottom: rc.top + split,
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
        top: rad_rc.bottom - 1,
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
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(2, 8);
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
    SelectObject(hdc, old_brush);

    let h = rc.bottom - rc.top;
    let show_cap = h >= 26;
    let font = pick_cand_font(hdc, &state.fonts, ch);
    let _ = SelectObject(hdc, HGDIOBJ(font.0));
    SetTextColor(hdc, rgb(0xF4, 0xF5, 0xF8));
    let mut s: Vec<u16> = utf16_char(ch);
    let mut glyph_rc = RECT {
        left: rc.left + 1,
        top: rc.top + 1,
        right: rc.right - 1,
        bottom: if show_cap {
            rc.bottom - (h / 3).max(10)
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
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(2, 10);
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
    SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, HGDIOBJ(state.fonts.named.0));
    SetTextColor(hdc, rgb(0xE8, 0xE4, 0xC8));
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
    } else if code == 0 || cp437::is_named_blank(code) {
        state.brushes.special
    } else {
        state.brushes.key
    };

    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let radius = ((rc.bottom - rc.top).min(rc.right - rc.left) / 6).clamp(2, 10);
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
    SelectObject(hdc, old_brush);

    let h = rc.bottom - rc.top;
    let show_caption = h >= 30 && code != 0;
    let named = code == 0 || cp437::is_named_blank(code);
    let label = cp437::display_label(code);
    let mut label_wide: Vec<u16> = label.encode_utf16().collect();

    if named {
        let _ = SelectObject(hdc, HGDIOBJ(state.fonts.named.0));
        SetTextColor(
            hdc,
            if code == 0 {
                rgb(0x8E, 0xC8, 0xFF)
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
            bottom: rc.bottom - (h / 3).max(12),
        };
        DrawTextW(
            hdc,
            &mut label_wide,
            &mut glyph_rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );

        let cap = format!("Alt+{code}");
        let mut cap_wide: Vec<u16> = cap.encode_utf16().collect();
        let _ = SelectObject(hdc, HGDIOBJ(state.fonts.caption.0));
        SetTextColor(hdc, rgb(0x9A, 0xA4, 0xB8));
        let mut cap_rc = RECT {
            left: rc.left + 2,
            top: glyph_rc.bottom - 1,
            right: rc.right - 2,
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
