//! Native Win32 on-screen keyboard.
//!
//! The window is always-on-top, resizable, and created with `WS_EX_NOACTIVATE`
//! so clicks type into the previously focused application instead of stealing
//! focus. Characters are injected with `SendInput` + `KEYEVENTF_UNICODE`.

#![cfg(windows)]

use on99kb::cp437;
use windows::core::{w, Result as WinResult, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetStockObject,
    InvalidateRect, RoundRect, ScreenToClient, SelectObject, SetBkMode, SetTextColor, BLACK_BRUSH,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER,
    DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, HBRUSH, HDC,
    HFONT, HGDIOBJ, HPEN, OUT_TT_PRECIS, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
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
    GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, LoadCursorW, MessageBoxW,
    MoveWindow, PostQuitMessage, RegisterClassExW, SetCursor, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA,
    HWND_TOPMOST, IDC_ARROW, IDC_HAND, MA_NOACTIVATE, MB_ICONERROR, MB_OK, MINMAXINFO, MSG,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_SIZE, WNDCLASSEXW,
    WS_CAPTION, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_MINIMIZEBOX, WS_POPUP,
    WS_SYSMENU, WS_THICKFRAME,
};

// Defined in commctrl / UI::Controls; use the documented value to avoid extra features.
const WM_MOUSELEAVE: u32 = 0x02A3;

const COLS: i32 = 16;
const ROWS: i32 = 16;
const GRID_PAD: i32 = 8;
const KEY_GAP: i32 = 3;
const DEFAULT_CLIENT_W: i32 = 900;
const DEFAULT_CLIENT_H: i32 = 820;
const MIN_CLIENT_W: i32 = 480;
const MIN_CLIENT_H: i32 = 460;

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

fn handle_nonzero<T: Copy + PartialEq + Default>(h: T) -> bool {
    h != T::default()
}

struct Fonts {
    glyph: HFONT,
    caption: HFONT,
    named: HFONT,
    status: HFONT,
}

impl Fonts {
    fn empty() -> Self {
        Self {
            glyph: HFONT::default(),
            caption: HFONT::default(),
            named: HFONT::default(),
            status: HFONT::default(),
        }
    }

    unsafe fn delete(&mut self) {
        for font in [
            &mut self.glyph,
            &mut self.caption,
            &mut self.named,
            &mut self.status,
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
    border: HPEN,
}

struct AppState {
    hover: Option<u8>,
    pressed: Option<u8>,
    last_typed: Option<u8>,
    tracking: bool,
    fonts: Fonts,
    brushes: Brushes,
    cell_h: i32,
}

impl AppState {
    fn new() -> WinResult<Self> {
        unsafe {
            Ok(Self {
                hover: None,
                pressed: None,
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
                    border: CreatePen(PS_SOLID, 1, rgb(0x18, 0x1A, 0x20)),
                },
                cell_h: 0,
            })
        }
    }

    fn status_h(client_h: i32) -> i32 {
        (client_h / 18).clamp(28, 44)
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

    fn hit_test(client: RECT, x: i32, y: i32) -> Option<u8> {
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
        Some((row * COLS + col) as u8)
    }

    unsafe fn ensure_fonts(&mut self, client: RECT) {
        let grid = Self::grid_rect(client);
        let ch = ((grid.bottom - grid.top) / ROWS).max(1);
        if ch == self.cell_h && handle_nonzero(self.fonts.glyph) {
            return;
        }
        self.fonts.delete();
        self.cell_h = ch;
        let glyph_h = (ch as f32 * 0.46).round() as i32;
        let caption_h = (ch as f32 * 0.20).round() as i32;
        let named_h = (ch as f32 * 0.26).round() as i32;
        let status = Self::status_h(client.bottom - client.top);
        let status_font_h = (status as f32 * 0.48).round() as i32;
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

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(windows::core::Error::from_win32());
        }

        let mut wr = RECT {
            left: 0,
            top: 0,
            right: DEFAULT_CLIENT_W,
            bottom: DEFAULT_CLIENT_H,
        };
        AdjustWindowRectEx(&mut wr, APP_STYLE, false, APP_EX_STYLE)?;
        let win_w = wr.right - wr.left;
        let win_h = wr.bottom - wr.top;

        let hwnd = CreateWindowExW(
            APP_EX_STYLE,
            class_name,
            w!("on99KB  —  CP437 螢幕鍵盤"),
            APP_STYLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
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
                let mut wr = RECT {
                    left: 0,
                    top: 0,
                    right: MIN_CLIENT_W,
                    bottom: MIN_CLIENT_H,
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
                state.cell_h = 0;
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_SIZE => {
            if let Some(state) = state_mut(hwnd) {
                state.cell_h = 0;
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(hwnd, &mut pt);
            let rc = client_rect(hwnd);
            if AppState::hit_test(rc, pt.x, pt.y).is_some() {
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
            let code = AppState::hit_test(rc, x, y);
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
                if state.hover != code {
                    state.hover = code;
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
            let code = AppState::hit_test(rc, x, y);
            if let Some(state) = state_mut(hwnd) {
                state.pressed = code;
                let _ = InvalidateRect(hwnd, None, false);
            }
            let _ = SetCapture(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            let (x, y) = point_from_lparam(lparam);
            let rc = client_rect(hwnd);
            let code = AppState::hit_test(rc, x, y);
            if let Some(state) = state_mut(hwnd) {
                let pressed = state.pressed.take();
                if let (Some(p), Some(c)) = (pressed, code) {
                    if p == c && c != 0 {
                        if let Some(ch) = cp437::glyph(c) {
                            let _ = send_unicode(ch);
                            state.last_typed = Some(c);
                        }
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
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
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
    // All CP437 glyphs are in the BMP; still encode generally.
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

        let status_h = AppState::status_h(height);
        let status_rc = RECT {
            left: 0,
            top: height - status_h,
            right: width,
            bottom: height,
        };
        FillRect(mem, &status_rc, state.brushes.status);

        let old_pen = SelectObject(mem, HGDIOBJ(state.brushes.border.0));
        let _ = SetBkMode(mem, TRANSPARENT);

        for code in 0u8..=255 {
            draw_key(mem, state, rc, code);
        }

        let hint = if let Some(code) = state.hover.or(state.last_typed) {
            let text = cp437::hint_text(code);
            if state.last_typed == Some(code) && state.hover.is_none() {
                format!("已輸入  ·  {text}")
            } else if state.last_typed.is_some() && state.hover == state.last_typed {
                format!("{text}  ·  點擊輸入")
            } else {
                text
            }
        } else {
            "置頂 · 唔會搶焦點 · 點擊按鍵輸入到先前聚焦嘅視窗 · 拖曳邊緣可調整大小".to_string()
        };

        let mut hint_rc = RECT {
            left: status_rc.left + 12,
            top: status_rc.top,
            right: status_rc.right - 12,
            bottom: status_rc.bottom,
        };
        let _ = SelectObject(mem, HGDIOBJ(state.fonts.status.0));
        SetTextColor(mem, rgb(0xC8, 0xCD, 0xD8));
        let mut hint_wide: Vec<u16> = hint.encode_utf16().collect();
        DrawTextW(
            mem,
            &mut hint_wide,
            &mut hint_rc,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );

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

unsafe fn draw_key(hdc: HDC, state: &AppState, client: RECT, code: u8) {
    let rc = AppState::cell_rect(client, code);
    if rc.right - rc.left < 4 || rc.bottom - rc.top < 4 {
        return;
    }

    let brush = if state.pressed == Some(code) {
        state.brushes.pressed
    } else if state.hover == Some(code) {
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
