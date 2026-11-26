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
    /// State just before the last commit, so a commit whose `SendInput`
    /// failed can be undone ([`Ime::restore_failed_commit`]).
    undo: Option<Box<Snapshot>>,
}

/// The composition as it was just before a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    q9: Composer,
    overlay_open: bool,
    codes: String,
    page: usize,
    last_pick: Option<char>,
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
            undo: None,
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

    /// Physical Backspace must be captured as 清碼: a 九方 composition, the
    /// overlay, or a 速成 composition is active. Independent of 數墊.
    pub fn bksp_active(&self) -> bool {
        self.q9_active() || self.simplex_active()
    }

    /// The stage the 九方 pad shows (Feature when idle).
    pub fn q9_stage(&self) -> crate::q9::Stage {
        self.q9.stage()
    }

    fn snapshot(&mut self) {
        self.undo = Some(Box::new(Snapshot {
            q9: self.q9.clone(),
            overlay_open: self.overlay_open,
            codes: self.codes.clone(),
            page: self.page,
            last_pick: self.last_pick,
        }));
    }

    /// The last `Action::Commit` could not be typed (`SendInput` failed):
    /// put the composition (and an open overlay) back as it was, so the
    /// commit is not treated as successful. Returns whether anything was
    /// restored.
    pub fn restore_failed_commit(&mut self) -> bool {
        match self.undo.take() {
            Some(s) => {
                self.q9 = s.q9;
                self.overlay_open = s.overlay_open;
                self.codes = s.codes;
                self.page = s.page;
                self.last_pick = s.last_pick;
                true
            }
            None => false,
        }
    }

    /// The glyph typed for a Chinese candidate under the current output.
    pub fn display(&self, ch: char) -> char {
        script::output_char(self.output, ch)
    }

    /// Successful-commit reset: the pad goes back to the first-key
    /// (Feature) state and the overlay closes, ready for the next
    /// character. If the UI then fails to type the glyph it calls
    /// [`Ime::restore_failed_commit`].
    fn commit(&mut self, ch: char) -> Action {
        self.snapshot();
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
        self.snapshot();
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

/// What the hook does with a physical numpad event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumpadVerdict {
    /// Not ours: `CallNextHookEx`.
    Pass,
    /// Eat it and tell the UI thread (first key-down of a press).
    Down(u8),
    /// Eat it without telling anyone (auto-repeat of a press we took).
    Eat,
    /// Eat the key-up of a press we took and tell the UI thread, which
    /// types a deferred commit only now (no `SendInput` while the physical
    /// key is still held by the hook).
    Up(u8),
}

/// Numpad presses the hook has taken and whose key-up has not arrived:
/// `scans[d]` is the hardware scan code of digit `d`'s key, or 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NumpadHeld {
    pub scans: [u16; 10],
}

impl NumpadHeld {
    pub fn any(&self) -> bool {
        self.scans.iter().any(|s| *s != 0)
    }
}

pub const LLKHF_EXTENDED: u32 = 0x01;

/// Numpad 0–9 through the hook; the returned [`NumpadHeld`] replaces `held`.
///
/// - A key-down that passes [`should_capture`] is posted once; its
///   auto-repeats are eaten silently, so holding a key cannot start a
///   stray new composition after a commit.
/// - The key-up of a taken press is always eaten and posted (even if 數墊 or
///   the window changed meanwhile), so the target never sees a lone key-up.
///   It is matched by virtual key **or** by the key's scan code: if NumLock
///   or Shift changed while the key was down, Windows reports the key-up as
///   a navigation key (Home, End, …) with the same non-extended scan code.
///   Without that, the press would stay "held" and every later press of
///   that digit would be eaten as a repeat, so the next character could
///   not be typed.
/// - A key-up whose key-down was not taken passes through.
pub fn numpad_event(
    gate: Gate,
    held: NumpadHeld,
    vk: u32,
    scan: u32,
    flags: u32,
    down: bool,
) -> (NumpadVerdict, NumpadHeld) {
    if flags & (LLKHF_INJECTED | LLKHF_LOWER_IL_INJECTED) != 0 {
        return (NumpadVerdict::Pass, held);
    }
    let mut next = held;
    if down {
        let Some(d) = numpad_digit(vk) else {
            return (NumpadVerdict::Pass, held);
        };
        let i = usize::from(d);
        if held.scans[i] != 0 {
            (NumpadVerdict::Eat, held)
        } else if should_capture(gate, vk, flags).is_some() {
            // 0 means "not held", so a missing scan code is stored as 0xFFFF.
            next.scans[i] = if scan == 0 { 0xFFFF } else { scan as u16 };
            (NumpadVerdict::Down(d), next)
        } else {
            (NumpadVerdict::Pass, held)
        }
    } else {
        let same_key = |i: usize| {
            numpad_digit(vk) == Some(i as u8)
                || (scan != 0 && flags & LLKHF_EXTENDED == 0 && u32::from(held.scans[i]) == scan)
        };
        match (0..10).find(|&i| held.scans[i] != 0 && same_key(i)) {
            Some(i) => {
                next.scans[i] = 0;
                (NumpadVerdict::Up(i as u8), next)
            }
            None => (NumpadVerdict::Pass, held),
        }
    }
}

pub const VK_BACK: u32 = 0x08;

/// Conditions for physical Backspace 清碼.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BkspGate {
    /// [`Ime::bksp_active`]: a 九方 composition, the overlay, or a 速成
    /// composition. Not tied to 數墊.
    pub active: bool,
    pub visible: bool,
    pub minimized: bool,
}

/// What the hook does with a physical Backspace event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BkspVerdict {
    /// Normal Backspace for the target: `CallNextHookEx`.
    Pass,
    /// Eat it and post 清碼 to the UI thread.
    Clear,
    /// Eat an auto-repeat of a press that already did 清碼.
    Eat,
    /// Eat the key-up of that press (the UI may then re-sync the hook).
    Released,
}

/// Physical `VK_BACK` through the hook. `eating` is true from a captured
/// key-down until its key-up; the returned flag replaces it.
///
/// Only `VK_BACK`, only non-injected, only while visible, not minimized
/// and a composition / overlay is active. Once a key-down was taken, its
/// repeats and key-up are eaten even though 清碼 has already ended the
/// composition, so nothing leaks into the target. Idle presses always
/// pass through (no eat-and-re-inject).
pub fn backspace_event(
    gate: BkspGate,
    eating: bool,
    vk: u32,
    flags: u32,
    down: bool,
) -> (BkspVerdict, bool) {
    if vk != VK_BACK || flags & (LLKHF_INJECTED | LLKHF_LOWER_IL_INJECTED) != 0 {
        return (BkspVerdict::Pass, eating);
    }
    if down {
        if eating {
            (BkspVerdict::Eat, true)
        } else if gate.active && gate.visible && !gate.minimized {
            (BkspVerdict::Clear, true)
        } else {
            (BkspVerdict::Pass, false)
        }
    } else if eating {
        (BkspVerdict::Released, false)
    } else {
        (BkspVerdict::Pass, false)
    }
}

/// Whether the hook should be installed at all: the window is visible and
/// not minimized and 數墊 is on or Backspace may need capturing, or a
/// press we took is still waiting for its key-up.
pub fn hook_wanted(
    capture_on: bool,
    bksp_active: bool,
    visible: bool,
    minimized: bool,
    pending_up: bool,
) -> bool {
    pending_up || (visible && !minimized && (capture_on || bksp_active))
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

    // ---- 1.3.1: second character, go-back, physical Backspace ----------

    fn type_code(ime: &mut Ime, t: &Table, keys: &[u8]) {
        for &k in keys {
            assert_eq!(ime.q9_key(k, t), Action::None, "key {k} of {keys:?}");
        }
    }

    #[test]
    fn after_commit_ime_is_idle_and_next_key_starts_fresh() {
        let t = table();
        for mode in [
            KeyboardMode::Jiufang,
            KeyboardMode::Cp437,
            KeyboardMode::Simplex,
        ] {
            let mut ime = Ime::new(mode);
            for k in [4, 7, 0] {
                ime.numpad_digit(k, &t);
            }
            assert_eq!(ime.numpad_digit(1, &t), Action::Commit('前'), "{mode:?}");
            assert!(!ime.q9.is_active(), "no 九方 keys left");
            assert!(!ime.overlay_open, "overlay closed");
            assert!(!ime.bksp_active(), "nothing left for Backspace to clear");
            assert_eq!(
                ime.q9_stage(),
                q9::Stage::Feature,
                "pad on the feature keys"
            );
            assert_eq!(ime.q9.face(0, &t), q9::Face::Zero(""), "0 idle, not 下頁");
            assert_eq!(ime.mode, mode, "layout unchanged");
            // The next first key starts a fresh composition.
            ime.numpad_digit(6, &t);
            assert_eq!(ime.q9.keys(), &[6]);
            assert_eq!(ime.q9_stage(), q9::Stage::Component(6));
            assert_eq!(ime.overlay_open, mode != KeyboardMode::Jiufang);
        }
    }

    #[test]
    fn two_commits_in_a_row_in_both_scripts() {
        let t = table();
        for (output, first, second) in [
            (OutputScript::Traditional, '前', '國'),
            (OutputScript::Simplified, '前', '国'),
        ] {
            for mode in [KeyboardMode::Jiufang, KeyboardMode::Simplex] {
                let mut ime = Ime::new(mode);
                ime.output = output;
                for k in [4, 7, 0] {
                    ime.numpad_digit(k, &t);
                }
                assert_eq!(ime.numpad_digit(1, &t), Action::Commit(first));
                for k in [5, 2, 0] {
                    ime.numpad_digit(k, &t);
                }
                assert_eq!(
                    ime.numpad_digit(1, &t),
                    Action::Commit(second),
                    "{output:?} {mode:?}"
                );
                assert_eq!(ime.last_pick, Some('國'), "lookup stays Traditional");
                assert!(!ime.q9_active());
            }
        }
        // Mouse paths on the 九方 layout, twice each.
        let mut ime = Ime::new(KeyboardMode::Jiufang);
        type_code(&mut ime, &t, &[6, 9, 0]);
        assert_eq!(ime.q9_space(&t), Action::Commit('日'));
        type_code(&mut ime, &t, &[6, 9, 0]);
        assert_eq!(ime.q9_candidate(1, &t), Action::Commit('是'));
        // Unmapped stays original in 簡.
        ime.output = OutputScript::Simplified;
        type_code(&mut ime, &t, &[9, 1, 5]);
        assert_eq!(ime.q9_space(&t), Action::Commit('嘅'));
        type_code(&mut ime, &t, &[1, 2, 1]);
        assert_eq!(ime.q9_key(1, &t), Action::Commit('林'));
    }

    #[test]
    fn delete_from_candidates_goes_back_one_stage() {
        let t = table();
        // 整體: Candidates → Third (0 is 確定 again).
        let mut ime = Ime::new(KeyboardMode::Jiufang);
        type_code(&mut ime, &t, &[4, 7, 0]);
        assert_eq!(ime.q9.face(0, &t), q9::Face::Zero("下頁"));
        assert_eq!(ime.q9_delete(), Action::None);
        assert_eq!(ime.q9_stage(), q9::Stage::Third(4, 7));
        assert_eq!(ime.q9.face(0, &t), q9::Face::Zero("確定"));
        assert!(ime.q9.candidates(&t).is_empty());
        // 確定 again works.
        assert_eq!(ime.q9_key(0, &t), Action::None);
        assert_eq!(ime.q9_key(1, &t), Action::Commit('前'));
        // 分體 on the overlay: Candidates → Third, then on back to Feature.
        let mut ime = Ime::new(KeyboardMode::Cp437);
        for k in [4, 3, 4] {
            ime.numpad_digit(k, &t);
        }
        ime.q9_delete();
        assert_eq!(ime.q9_stage(), q9::Stage::Third(4, 3));
        assert!(ime.overlay_open, "刪碼 keeps the overlay");
        ime.q9_delete();
        assert_eq!(ime.q9_stage(), q9::Stage::Component(4));
        assert_eq!(
            ime.q9.face(3, &t),
            q9::Face::Component(q9::COMPONENTS[3][2])
        );
        ime.q9_delete();
        assert_eq!(ime.q9_stage(), q9::Stage::Feature);
        // 刪碼 never commits or sends anything.
        assert_eq!(ime.q9_delete(), Action::None);
    }

    #[test]
    fn backspace_clear_then_new_code_and_hidden_simplex() {
        let t = table();
        let mut ime = Ime::new(KeyboardMode::Simplex);
        ime.simplex_letter('a');
        ime.simplex_letter('b');
        for k in [4, 7, 0] {
            ime.numpad_digit(k, &t);
        }
        assert!(ime.bksp_active());
        assert_eq!(ime.backspace(), Action::None, "清碼, no VK_BACK");
        assert!(!ime.overlay_open && !ime.q9.is_active());
        assert_eq!(ime.codes, "ab", "hidden 速成 kept");
        assert!(ime.bksp_active(), "速成 still there for a second Backspace");
        // A new 九方 code starts at once.
        ime.numpad_digit(5, &t);
        assert!(ime.overlay_open);
        assert_eq!(ime.q9.keys(), &[5]);
        ime.backspace();
        assert_eq!(ime.codes, "ab");
        // Second Backspace clears 速成, third goes to the target.
        assert_eq!(ime.backspace(), Action::None);
        assert!(ime.codes.is_empty());
        assert!(!ime.bksp_active());
        assert_eq!(ime.backspace(), Action::SendBackspace);
    }

    #[test]
    fn commit_helpers_and_empty_slot() {
        let t = table();
        for (output, want) in [
            (OutputScript::Traditional, ['國', '回', '因']),
            (OutputScript::Simplified, ['国', '回', '因']),
        ] {
            let mut ime = Ime::new(KeyboardMode::Jiufang);
            ime.output = output;
            type_code(&mut ime, &t, &[5, 2, 0]);
            assert_eq!(ime.q9_space(&t), Action::Commit(want[0]));
            type_code(&mut ime, &t, &[5, 2, 0]);
            assert_eq!(ime.q9_key(2, &t), Action::Commit(want[1]));
            type_code(&mut ime, &t, &[5, 2, 0]);
            assert_eq!(ime.q9_candidate(2, &t), Action::Commit(want[2]));
        }
        // 空格 takes only the first of the current page, never U+0020.
        let mut ime = Ime::new(KeyboardMode::Jiufang);
        type_code(&mut ime, &t, &[6, 9, 0]);
        ime.q9_key(0, &t); // 下頁
        let first = ime.q9.page_items(&t)[0];
        assert_eq!(ime.q9_space(&t), Action::Commit(first));
        assert_ne!(first, ' ');
        // Empty slot: 前 has 8 candidates on 470, so key 9 is empty.
        type_code(&mut ime, &t, &[4, 7, 0]);
        let before = ime.clone();
        assert_eq!(ime.q9_key(9, &t), Action::None);
        assert_eq!(ime.q9_candidate(8, &t), Action::None);
        assert_eq!(ime, before, "composition left as it was");
        // 空格 with no candidates types nothing.
        let mut ime = Ime::new(KeyboardMode::Jiufang);
        assert_eq!(ime.q9_space(&t), Action::None);
    }

    #[test]
    fn failed_send_restores_the_composition() {
        let t = table();
        let mut ime = Ime::new(KeyboardMode::Cp437);
        for k in [4, 7, 0] {
            ime.numpad_digit(k, &t);
        }
        ime.q9_key(0, &t); // one page only: stays on page 1
        let before = (ime.q9.clone(), ime.overlay_open, ime.last_pick);
        assert_eq!(ime.numpad_digit(2, &t), Action::Commit('首'));
        assert!(ime.restore_failed_commit());
        assert_eq!((ime.q9.clone(), ime.overlay_open, ime.last_pick), before);
        assert!(ime.q9_active(), "overlay and code kept");
        assert!(!ime.restore_failed_commit(), "only once");
        // 速成 too.
        let mut ime = Ime::new(KeyboardMode::Simplex);
        ime.simplex_letter('a');
        ime.simplex_commit('日');
        assert!(ime.codes.is_empty());
        ime.restore_failed_commit();
        assert_eq!(ime.codes, "a");
    }

    #[test]
    fn delete_and_backspace_regression() {
        let t = table();
        let mut ime = Ime::new(KeyboardMode::Jiufang);
        type_code(&mut ime, &t, &[1, 2, 1]);
        ime.q9_delete();
        assert_eq!(ime.q9.keys(), &[1, 2], "刪碼 pops exactly one");
        assert_eq!(ime.backspace(), Action::None);
        assert!(!ime.q9.is_active(), "Bksp clears the whole code");
        let mut ime = Ime::new(KeyboardMode::Simplex);
        ime.simplex_letter('a');
        ime.simplex_letter('b');
        ime.simplex_delete();
        assert_eq!(ime.codes, "a");
        ime.simplex_letter('b');
        ime.backspace();
        assert!(ime.codes.is_empty());
    }

    const VK_SPACE: u32 = 0x20;

    fn bgate(active: bool) -> BkspGate {
        BkspGate {
            active,
            visible: true,
            minimized: false,
        }
    }

    #[test]
    fn bksp_gate_captures_only_when_composing_and_visible() {
        // Idle in every mode, 數墊 on or off: pass through.
        for mode in [
            KeyboardMode::Cp437,
            KeyboardMode::Simplex,
            KeyboardMode::Jiufang,
        ] {
            for capture in [true, false] {
                let mut ime = Ime::new(mode);
                ime.numpad_capture = capture;
                let g = bgate(ime.bksp_active());
                assert_eq!(
                    backspace_event(g, false, VK_BACK, 0, true),
                    (BkspVerdict::Pass, false)
                );
                assert_eq!(
                    backspace_event(g, false, VK_BACK, 0, false),
                    (BkspVerdict::Pass, false)
                );
            }
        }
        // 數墊 off + a composition started on screen: still captured.
        let t = table();
        let mut ime = Ime::new(KeyboardMode::Jiufang);
        ime.numpad_capture = false;
        ime.q9_key(6, &t);
        assert!(ime.bksp_active());
        assert!(hook_wanted(
            ime.numpad_capture,
            ime.bksp_active(),
            true,
            false,
            false
        ));
        assert_eq!(
            backspace_event(bgate(true), false, VK_BACK, 0, true),
            (BkspVerdict::Clear, true)
        );
        let mut ime = Ime::new(KeyboardMode::Simplex);
        ime.numpad_capture = false;
        ime.simplex_letter('a');
        assert!(ime.bksp_active());
        // Hidden or minimized: never captured.
        for g in [
            BkspGate {
                visible: false,
                ..bgate(true)
            },
            BkspGate {
                minimized: true,
                ..bgate(true)
            },
        ] {
            assert_eq!(
                backspace_event(g, false, VK_BACK, 0, true).0,
                BkspVerdict::Pass
            );
        }
        // Injected (including our own VK_BACK) is never captured.
        for f in [LLKHF_INJECTED, LLKHF_LOWER_IL_INJECTED] {
            assert_eq!(
                backspace_event(bgate(true), false, VK_BACK, f, true).0,
                BkspVerdict::Pass
            );
        }
        // Only VK_BACK: Delete, digits, Space, numpad ops, NumLock-off nav.
        for vk in [
            0x2E, 0x30, 0x39, VK_SPACE, 0x6A, 0x6B, 0x6D, 0x6E, 0x6F, 0x2D, 0x23, 0x24, 0x60,
        ] {
            assert_eq!(
                backspace_event(bgate(true), false, vk, 0, true).0,
                BkspVerdict::Pass,
                "vk {vk:#x}"
            );
        }
    }

    #[test]
    fn bksp_gate_eats_repeats_and_keyup_after_clear() {
        let t = table();
        let mut ime = Ime::new(KeyboardMode::Cp437);
        ime.numpad_digit(4, &t);
        // Key-down: captured, 清碼 runs on the UI thread.
        let (v, eating) = backspace_event(bgate(ime.bksp_active()), false, VK_BACK, 0, true);
        assert_eq!(v, BkspVerdict::Clear);
        assert_eq!(ime.backspace(), Action::None);
        assert!(!ime.bksp_active());
        // Auto-repeats of the same press are eaten although idle now.
        let (v, eating) = backspace_event(bgate(ime.bksp_active()), eating, VK_BACK, 0, true);
        assert_eq!(v, BkspVerdict::Eat);
        let (v, eating) = backspace_event(bgate(ime.bksp_active()), eating, VK_BACK, 0, true);
        assert_eq!(v, BkspVerdict::Eat);
        // The hook stays while the key-up is pending.
        assert!(hook_wanted(false, false, true, false, eating));
        // Key-up eaten, then idle.
        let (v, eating) = backspace_event(bgate(false), eating, VK_BACK, 0, false);
        assert_eq!(v, BkspVerdict::Released);
        assert!(!eating);
        assert!(!hook_wanted(false, false, true, false, eating));
        // A new press with nothing composing reaches the target.
        assert_eq!(
            backspace_event(bgate(false), eating, VK_BACK, 0, true).0,
            BkspVerdict::Pass
        );
        assert_eq!(ime.backspace(), Action::SendBackspace);
    }

    #[test]
    fn numpad_event_posts_once_eats_repeats_and_keyup() {
        const SC5: u32 = 0x4C;
        const SC7: u32 = 0x47;
        let g = open_gate();
        let none = NumpadHeld::default();
        let (v, held) = numpad_event(g, none, VK_NUMPAD5, SC5, 0, true);
        assert_eq!(v, NumpadVerdict::Down(5));
        assert!(held.any());
        // Auto-repeat: eaten, not posted again.
        let (v, held) = numpad_event(g, held, VK_NUMPAD5, SC5, 0, true);
        assert_eq!(v, NumpadVerdict::Eat);
        // Another digit while 5 is held (rollover) is its own press.
        let (v, held) = numpad_event(g, held, VK_NUMPAD7, SC7, 0, true);
        assert_eq!(v, NumpadVerdict::Down(7));
        // Key-up of a taken press is eaten and posted even if 數墊 went off.
        let off = Gate {
            capture_on: false,
            ..g
        };
        let (v, held) = numpad_event(off, held, VK_NUMPAD5, SC5, 0, false);
        assert_eq!(v, NumpadVerdict::Up(5));
        // NumLock / Shift changed while 7 was down: the key-up arrives as
        // VK_HOME with the same scan code. It still ends the press...
        let (v, held) = numpad_event(g, held, 0x24, SC7, 0, false);
        assert_eq!(v, NumpadVerdict::Up(7));
        assert_eq!(held, none);
        // ...so the next press of 7 starts a new key, not a "repeat".
        let (v, held) = numpad_event(g, held, VK_NUMPAD7, SC7, 0, true);
        assert_eq!(v, NumpadVerdict::Down(7));
        // The extended Home key (nav cluster, same scan code) is not it.
        let (v, held) = numpad_event(g, held, 0x24, SC7, LLKHF_EXTENDED, false);
        assert_eq!(v, NumpadVerdict::Pass);
        let (_, held) = numpad_event(g, held, VK_NUMPAD7, SC7, 0, false);
        assert_eq!(held, none);
        // A key-up whose key-down we did not take passes through.
        assert_eq!(
            numpad_event(g, none, VK_NUMPAD5, SC5, 0, false).0,
            NumpadVerdict::Pass
        );
        // 數墊 off: digits go to the target; injected always passes.
        assert_eq!(
            numpad_event(off, none, VK_NUMPAD5, SC5, 0, true).0,
            NumpadVerdict::Pass
        );
        assert_eq!(
            numpad_event(g, none, VK_NUMPAD5, SC5, LLKHF_INJECTED, true).0,
            NumpadVerdict::Pass
        );
        // Backspace and other keys are not numpad business.
        assert_eq!(
            numpad_event(g, none, VK_BACK, 0x0E, 0, true).0,
            NumpadVerdict::Pass
        );
        assert_eq!(
            numpad_event(g, none, 0x35, 0x06, 0, true).0,
            NumpadVerdict::Pass
        );
    }

    #[test]
    fn hook_wanted_follows_toggle_composition_and_window() {
        assert!(hook_wanted(true, false, true, false, false), "數墊 on");
        assert!(
            !hook_wanted(false, false, true, false, false),
            "idle, 數墊 off"
        );
        assert!(
            hook_wanted(false, true, true, false, false),
            "composing, 數墊 off"
        );
        assert!(!hook_wanted(true, true, true, true, false), "minimized");
        assert!(!hook_wanted(true, true, false, false, false), "hidden");
    }

    const VK_NUMPAD5: u32 = 0x65;
    const VK_NUMPAD7: u32 = 0x67;
}
