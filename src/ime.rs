//! Composition state shared by the on-screen layouts and the physical
//! numpad, kept free of Win32 so it can be unit-tested on Linux.
//!
//! - The visible layout (`mode`) changes only when the user clicks a mode
//!   segment. Numpad input never changes it.
//! - On the 九方 layout the numpad and the mouse drive the on-screen pad.
//! - On CP437 / 速成 the first numpad digit opens a focus-free 九方 overlay
//!   inside the existing window and starts composition with that key. A
//!   速成 composition in progress is kept (hidden) and comes back when the
//!   overlay closes. On-screen CP437 / 速成 keys never feed the 九方 code.
//!
//! The capture gate for the `WH_KEYBOARD_LL` hook is [`should_capture`].

use crate::prefs::{KeyboardMode, OutputScript};
use crate::q9::{Composer, KeyResult, Table};
use crate::script;

/// What the UI layer must do after an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// State may have changed; repaint. Nothing to send.
    None,
    /// Type this character (already converted for 繁／簡) with SendInput.
    Commit(char),
    /// Send `VK_BACK` to the target window.
    SendBackspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ime {
    /// Visible layout (and the stored `mode=`).
    pub mode: KeyboardMode,
    /// 速成 codes typed so far (first + last).
    pub codes: String,
    /// 速成 candidate page (no wrap).
    pub page: usize,
    /// 九方 composition (on the 九方 pad or in the overlay).
    pub q9: Composer,
    /// The 九方 overlay is showing over CP437 / 速成.
    pub overlay_open: bool,
    pub output: OutputScript,
    /// 數墊: capture physical numpad 0–9.
    pub numpad_capture: bool,
    /// Last character committed from 速成 / 九方, before conversion.
    pub last_pick: Option<char>,
}

impl Default for Ime {
    fn default() -> Self {
        Self {
            mode: KeyboardMode::DEFAULT,
            codes: String::new(),
            page: 0,
            q9: Composer::new(),
            overlay_open: false,
            output: OutputScript::DEFAULT,
            numpad_capture: crate::prefs::NUMPAD_CAPTURE_DEFAULT,
            last_pick: None,
        }
    }
}

impl Ime {
    pub fn new(mode: KeyboardMode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }

    /// 九方 keys are live: on the 九方 layout, or while the overlay is open.
    pub fn q9_live(&self) -> bool {
        self.mode == KeyboardMode::Jiufang || self.overlay_open
    }

    /// A 九方 composition or the overlay is active (Bksp cancels it).
    pub fn q9_active(&self) -> bool {
        self.q9.is_active() || self.overlay_open
    }

    /// A 速成 code is in progress on the 速成 layout.
    pub fn simplex_active(&self) -> bool {
        self.mode == KeyboardMode::Simplex && !self.codes.is_empty()
    }

    /// The glyph typed for a Chinese candidate under the current output.
    pub fn display(&self, ch: char) -> char {
        script::output_char(self.output, ch)
    }

    fn commit(&mut self, ch: char) -> Action {
        self.last_pick = Some(ch);
        self.q9.clear();
        self.overlay_open = false;
        Action::Commit(self.display(ch))
    }

    /// Close the 九方 composition and overlay without committing.
    fn finish_q9(&mut self) {
        self.q9.clear();
        self.overlay_open = false;
    }

    /// A physical numpad digit that passed the capture gate.
    pub fn numpad_digit(&mut self, d: u8, table: &Table) -> Action {
        if self.mode != KeyboardMode::Jiufang && !self.overlay_open {
            // Keep the visible layout; open the overlay and start with this key.
            self.overlay_open = true;
        }
        self.q9_key(d, table)
    }

    /// A 九方 pad key (on-screen pad, overlay pad, or physical numpad).
    pub fn q9_key(&mut self, d: u8, table: &Table) -> Action {
        if !self.q9_live() {
            return Action::None;
        }
        match self.q9.press(d, table) {
            KeyResult::Commit(ch) => self.commit(ch),
            _ => Action::None,
        }
    }

    /// Click on 九方 candidate `index` of the current page.
    pub fn q9_candidate(&mut self, index: usize, table: &Table) -> Action {
        if !self.q9_live() {
            return Action::None;
        }
        match self.q9.pick(index, table) {
            Some(ch) => self.commit(ch),
            None => Action::None,
        }
    }

    /// 九方 空格: commit the first candidate of the current page.
    pub fn q9_space(&mut self, table: &Table) -> Action {
        self.q9_candidate(0, table)
    }

    /// 九方 刪碼: delete one code only.
    pub fn q9_delete(&mut self) -> Action {
        if self.q9_live() {
            self.q9.delete();
        }
        Action::None
    }

    /// 九方 清除: clear code and candidates, dismiss the overlay.
    pub fn q9_clear(&mut self) -> Action {
        self.finish_q9();
        Action::None
    }

    pub fn q9_next_page(&mut self, table: &Table) {
        self.q9.next_page(table);
    }

    pub fn q9_prev_page(&mut self, table: &Table) {
        self.q9.prev_page(table);
    }

    /// Mouse wheel: 九方 wraps (速成 does not). Positive delta = previous.
    /// Returns `true` if the wheel went to 九方.
    pub fn q9_wheel(&mut self, delta: i32, table: &Table) -> bool {
        if !self.q9_live() {
            return false;
        }
        if delta > 0 {
            self.q9.prev_page(table);
        } else if delta < 0 {
            self.q9.next_page(table);
        }
        true
    }

    /// Bksp: cancel a 九方 composition / overlay, else a 速成 composition,
    /// else send `VK_BACK`.
    pub fn backspace(&mut self) -> Action {
        if self.q9_active() {
            self.finish_q9();
            return Action::None;
        }
        if self.simplex_active() {
            self.codes.clear();
            self.page = 0;
            return Action::None;
        }
        Action::SendBackspace
    }

    /// On-screen 速成 radical. Never feeds the 九方 code.
    pub fn simplex_letter(&mut self, letter: char) -> bool {
        if crate::simplex::push_code(&mut self.codes, letter) {
            self.page = 0;
            true
        } else {
            false
        }
    }

    pub fn simplex_delete(&mut self) {
        if crate::simplex::pop_code(&mut self.codes) {
            self.page = 0;
        }
    }

    pub fn simplex_clear(&mut self) {
        self.codes.clear();
        self.page = 0;
    }

    /// A 速成 candidate was chosen (unconverted).
    pub fn simplex_commit(&mut self, ch: char) -> Action {
        self.last_pick = Some(ch);
        self.codes.clear();
        self.page = 0;
        Action::Commit(self.display(ch))
    }

    /// The user clicked a mode segment. Returns `true` if it changed (and
    /// `mode=` should be written).
    pub fn set_mode(&mut self, mode: KeyboardMode) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        self.codes.clear();
        self.page = 0;
        self.finish_q9();
        true
    }

    /// The window was minimized or is closing: drop the overlay without
    /// committing. A hidden 速成 composition is kept.
    pub fn on_hidden(&mut self) {
        if self.overlay_open {
            self.finish_q9();
        }
    }

    /// Whether the hint row should say `此字無對應簡體，將輸出原字` for this
    /// pick (or, when `pick` is `None`, for the last committed character).
    pub fn no_mapping_hint(&self, pick: Option<char>) -> bool {
        script::needs_no_mapping_hint(self.output, pick.or(self.last_pick))
    }
}

// ---- Capture gate for the WH_KEYBOARD_LL hook ----------------------------

pub const VK_NUMPAD0: u32 = 0x60;
pub const VK_NUMPAD9: u32 = 0x69;
pub const LLKHF_LOWER_IL_INJECTED: u32 = 0x02;
pub const LLKHF_INJECTED: u32 = 0x10;

/// Window / toggle conditions the hook checks on every key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gate {
    /// 數墊 toggle.
    pub capture_on: bool,
    /// The on99KB window exists and is visible.
    pub visible: bool,
    pub minimized: bool,
    /// NumLock toggled on.
    pub numlock: bool,
}

/// `Some(0..=9)` for `VK_NUMPAD0`–`VK_NUMPAD9`, else `None`. The main-row
/// digits, NumLock, numpad `+ - * /` and the NumLock-off navigation keys are
/// different virtual keys and never match.
pub fn numpad_digit(vk: u32) -> Option<u8> {
    if (VK_NUMPAD0..=VK_NUMPAD9).contains(&vk) {
        Some((vk - VK_NUMPAD0) as u8)
    } else {
        None
    }
}

/// Eat this key and post it to the window? Only numpad 0–9, only when 數墊 is
/// on, the window is visible and not minimized, NumLock is on, and the event
/// is not injected (so on99KB's own SendInput is never eaten).
pub fn should_capture(gate: Gate, vk: u32, flags: u32) -> Option<u8> {
    if flags & (LLKHF_INJECTED | LLKHF_LOWER_IL_INJECTED) != 0 {
        return None;
    }
    if !gate.capture_on || !gate.visible || gate.minimized || !gate.numlock {
        return None;
    }
    numpad_digit(vk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::q9;

    fn table() -> Table {
        q9::Table::parse(q9::BUILTIN_TABLE)
    }

    fn open_gate() -> Gate {
        Gate {
            capture_on: true,
            visible: true,
            minimized: false,
            numlock: true,
        }
    }

    #[test]
    fn numpad_composition_never_changes_mode_or_stored_mode() {
        let t = table();
        for mode in [KeyboardMode::Cp437, KeyboardMode::Simplex] {
            let mut ime = Ime::new(mode);
            let stored = crate::prefs::format_window_ini(
                &crate::prefs::sanitize(
                    480,
                    460,
                    None,
                    None,
                    crate::prefs::Opacity::P100,
                    true,
                    ime.mode,
                )
                .unwrap(),
            );
            ime.numpad_digit(6, &t);
            assert!(ime.overlay_open, "overlay opens on {mode:?}");
            assert_eq!(ime.q9.keys(), &[6], "composition starts with that key");
            ime.numpad_digit(9, &t);
            ime.numpad_digit(0, &t);
            assert_eq!(ime.mode, mode, "visible layout unchanged");
            let again = crate::prefs::format_window_ini(
                &crate::prefs::sanitize(
                    480,
                    460,
                    None,
                    None,
                    crate::prefs::Opacity::P100,
                    true,
                    ime.mode,
                )
                .unwrap(),
            );
            assert_eq!(stored, again, "mode= unchanged");
            let a = ime.numpad_digit(1, &t);
            assert!(matches!(a, Action::Commit(_)));
            assert!(!ime.overlay_open, "commit dismisses the overlay");
            assert_eq!(ime.mode, mode);
        }
        // On the 九方 layout numpad drives the pad directly; no overlay.
        let mut ime = Ime::new(KeyboardMode::Jiufang);
        ime.numpad_digit(4, &t);
        assert!(!ime.overlay_open);
        assert_eq!(ime.q9.keys(), &[4]);
        assert_eq!(ime.mode, KeyboardMode::Jiufang);
    }

    #[test]
    fn simplex_composition_survives_overlay() {
        let t = table();
        let mut ime = Ime::new(KeyboardMode::Simplex);
        assert!(ime.simplex_letter('a'));
        assert!(ime.simplex_letter('b'));
        ime.page = 1;
        ime.numpad_digit(6, &t);
        assert!(ime.overlay_open);
        assert_eq!(ime.codes, "ab", "not cleared when the overlay opens");
        assert_eq!(ime.page, 1);
        ime.q9_clear();
        assert!(!ime.overlay_open);
        assert_eq!(ime.codes, "ab", "restored after the overlay closes");
        assert_eq!(ime.page, 1);
        // Same after a commit from the overlay.
        for k in [6, 9, 0] {
            ime.numpad_digit(k, &t);
        }
        assert!(matches!(ime.q9_space(&t), Action::Commit(_)));
        assert_eq!(ime.codes, "ab");
        // Minimize drops the overlay without committing, keeps 速成.
        ime.numpad_digit(6, &t);
        ime.on_hidden();
        assert!(!ime.overlay_open);
        assert!(!ime.q9.is_active());
        assert_eq!(ime.codes, "ab");
    }

    #[test]
    fn onscreen_keys_do_not_mix_into_q9() {
        let t = table();
        let mut ime = Ime::new(KeyboardMode::Simplex);
        ime.numpad_digit(6, &t);
        ime.numpad_digit(9, &t);
        let before = ime.q9.clone();
        // A 速成 radical click while the overlay is open.
        ime.simplex_letter('r');
        ime.simplex_delete();
        ime.simplex_letter('u');
        assert_eq!(ime.q9, before, "九方 code untouched by 速成 keys");
        assert_eq!(ime.codes, "u");
        // CP437 layout: q9 keys only come from the overlay / numpad.
        let mut ime = Ime::new(KeyboardMode::Cp437);
        assert_eq!(ime.q9_key(6, &t), Action::None);
        assert!(!ime.q9.is_active(), "no overlay → pad keys ignored");
    }

    #[test]
    fn clear_dismisses_overlay() {
        let t = table();
        let mut ime = Ime::new(KeyboardMode::Cp437);
        for k in [6, 9, 0] {
            ime.numpad_digit(k, &t);
        }
        assert!(!ime.q9.candidates(&t).is_empty());
        ime.q9_clear();
        assert!(!ime.overlay_open);
        assert!(!ime.q9.is_active());
        assert!(ime.q9.candidates(&t).is_empty());
    }

    #[test]
    fn bksp_cancels_q9_without_vk_back() {
        let t = table();
        // Overlay on CP437.
        let mut ime = Ime::new(KeyboardMode::Cp437);
        ime.numpad_digit(6, &t);
        assert_eq!(ime.backspace(), Action::None);
        assert!(!ime.overlay_open && !ime.q9.is_active());
        // 九方 layout composition.
        let mut ime = Ime::new(KeyboardMode::Jiufang);
        ime.q9_key(6, &t);
        ime.q9_key(9, &t);
        assert_eq!(ime.backspace(), Action::None);
        assert!(!ime.q9.is_active());
        // Overlay over 速成 with a 速成 code: only 九方 is cancelled.
        let mut ime = Ime::new(KeyboardMode::Simplex);
        ime.simplex_letter('a');
        ime.numpad_digit(1, &t);
        assert_eq!(ime.backspace(), Action::None);
        assert_eq!(ime.codes, "a");
    }

    #[test]
    fn bksp_without_composition_sends_vk_back() {
        for mode in [
            KeyboardMode::Cp437,
            KeyboardMode::Simplex,
            KeyboardMode::Jiufang,
        ] {
            let mut ime = Ime::new(mode);
            assert_eq!(ime.backspace(), Action::SendBackspace, "{mode:?}");
        }
        // 速成 composition: cancel it first, then VK_BACK.
        let mut ime = Ime::new(KeyboardMode::Simplex);
        ime.simplex_letter('a');
        assert_eq!(ime.backspace(), Action::None);
        assert_eq!(ime.backspace(), Action::SendBackspace);
    }

    #[test]
    fn output_toggle_keeps_codes_and_converts_only_output() {
        let t = table();
        let mut ime = Ime::new(KeyboardMode::Jiufang);
        for k in [5, 2] {
            ime.q9_key(k, &t);
        }
        ime.simplex_letter('a');
        let (q9_before, codes_before) = (ime.q9.clone(), ime.codes.clone());
        ime.output = OutputScript::Simplified;
        assert_eq!(ime.q9, q9_before);
        assert_eq!(ime.codes, codes_before);
        ime.output = OutputScript::Traditional;
        assert_eq!(ime.q9, q9_before);
        // Lookup stays Traditional: 國 is found under 520 in both scripts.
        ime.output = OutputScript::Simplified;
        ime.q9_key(0, &t);
        let items = ime.q9.page_items(&t);
        assert!(items.contains(&'國'));
        let pos = items.iter().position(|c| *c == '國').unwrap();
        assert_eq!(ime.display('國'), '国');
        assert_eq!(ime.q9_candidate(pos, &t), Action::Commit('国'));
        assert!(!ime.no_mapping_hint(None));
        // Unmapped pick stays original and triggers the hint.
        assert_eq!(ime.simplex_commit('嘅'), Action::Commit('嘅'));
        assert!(ime.no_mapping_hint(None));
        assert!(ime.no_mapping_hint(Some('嘅')));
        assert!(!ime.no_mapping_hint(Some('國')));
    }

    #[test]
    fn gate_numlock_off_never_captures() {
        let g = Gate {
            numlock: false,
            ..open_gate()
        };
        for vk in VK_NUMPAD0..=VK_NUMPAD9 {
            assert_eq!(should_capture(g, vk, 0), None);
        }
    }

    #[test]
    fn gate_capture_toggle_off_never_captures() {
        let g = Gate {
            capture_on: false,
            ..open_gate()
        };
        for vk in VK_NUMPAD0..=VK_NUMPAD9 {
            assert_eq!(should_capture(g, vk, 0), None);
        }
        // The stored numpad_capture=0 maps to capture_on = false.
        let p =
            crate::prefs::parse_window_ini("width=480\nheight=460\nnumpad_capture=0\n").unwrap();
        let g = Gate {
            capture_on: p.numpad_capture,
            ..open_gate()
        };
        assert_eq!(should_capture(g, VK_NUMPAD5, 0), None);
    }

    #[test]
    fn gate_minimized_or_hidden_never_captures() {
        let min = Gate {
            minimized: true,
            ..open_gate()
        };
        let hidden = Gate {
            visible: false,
            ..open_gate()
        };
        for vk in VK_NUMPAD0..=VK_NUMPAD9 {
            assert_eq!(should_capture(min, vk, 0), None);
            assert_eq!(should_capture(hidden, vk, 0), None);
        }
    }

    #[test]
    fn gate_captures_only_numpad_digits_and_ignores_injected() {
        let g = open_gate();
        assert_eq!(should_capture(g, VK_NUMPAD0, 0), Some(0));
        assert_eq!(should_capture(g, VK_NUMPAD9, 0), Some(9));
        assert_eq!(should_capture(g, VK_NUMPAD7, LLKHF_INJECTED), None);
        assert_eq!(should_capture(g, VK_NUMPAD7, LLKHF_LOWER_IL_INJECTED), None);
        // Main-row digits, NumLock, + - * / . and NumLock-off nav keys.
        for vk in [
            0x30, 0x39, 0x90, 0x6A, 0x6B, 0x6D, 0x6E, 0x6F, 0x20, 0x24, 0x23, 0x21, 0x22, 0x25,
            0x26, 0x27, 0x28, 0x2D, 0x2E, 0x0D, 0x0C,
        ] {
            assert_eq!(should_capture(g, vk, 0), None, "vk {vk:#x}");
        }
    }

    const VK_NUMPAD5: u32 = 0x65;
    const VK_NUMPAD7: u32 = 0x67;
}
