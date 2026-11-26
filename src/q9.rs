//! 九方 (shape input; computer numpad) engine: table loading and the
//! three-key composition state machine.
//!
//! 「九方」 is the common, unofficial name for this method. on99KB is not
//! affiliated with, licensed by, or supported by 九方科技. The rules are
//! locked in `docs/JIUFANG-TYPOGRAPHY.md`; the starter table
//! `data/jiufang-shape.txt` is independently constructed (see
//! `data/SOURCES.md`). Candidate order is on99KB's own and is not claimed to
//! match official Q9 software.
//!
//! A complete code is `F C X`: upper-left feature, component under that
//! feature, then `0` (整體字, 確定) or the upper-right feature `1..=9`
//! (分體字). Once candidates are visible, `0` pages forward and wraps from the
//! last page to the first; it never re-confirms or clears. (Numpad `.` as
//! cancel and numpad Enter as 空格 are possible later work, not part of v1.)

use std::collections::HashMap;
use std::sync::OnceLock;

/// Candidates per page; candidate *n* sits on key *n*.
pub const PAGE_SIZE: usize = 9;

/// Publicly taught feature labels, indexed by key − 1 (key 1 = 十 … key 9 =
/// 豎). On the pad they sit in computer-numpad positions (7 8 9 on top).
pub const FEATURES: [&str; 9] = ["十", "交", "缺", "折", "圓", "點", "撇", "橫", "豎"];

/// on99KB's own component chart (docs/JIUFANG-TYPOGRAPHY.md §5.2):
/// `COMPONENTS[feature − 1][component − 1]`, each shown by a representative
/// character.
pub const COMPONENTS: [[&str; 9]; 9] = [
    ["十", "木", "艹", "廿", "寸", "左", "耂", "丰", "車"],
    ["又", "文", "大", "父", "夂", "九", "乂", "支", "戈"],
    ["冖", "厂", "尸", "戶", "匚", "凵", "門", "几", "月"],
    ["乙", "阝", "幺", "弓", "女", "子", "丷", "己", "力"],
    ["田", "囗", "罒", "白", "自", "貝", "見", "西", "目"],
    ["氵", "宀", "亠", "言", "广", "冫", "忄", "礻", "日"],
    ["亻", "彳", "人", "禾", "竹", "金", "牛", "夕", "犭"],
    ["一", "扌", "王", "土", "小", "工", "石", "卜", "雨"],
    ["口", "山", "中", "冂", "足", "虫", "止", "由", "火"],
];

/// Keys laid out as on a computer numpad, top row first.
pub const PAD_ROWS: [[u8; 3]; 3] = [[7, 8, 9], [4, 5, 6], [1, 2, 3]];

/// The built-in starter table, compiled into the exe.
pub const BUILTIN_TABLE: &str = include_str!("../data/jiufang-shape.txt");

/// 整體字 (upper half does not split) or 分體字 (upper half splits).
/// Teaching materials also say 獨體字 for 整體字; the UI always says 整體.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Whole,
    Split,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Class::Whole => "整體",
            Class::Split => "分體",
        }
    }
}

/// Class implied by a complete code's third key.
pub fn class_of(code: [u8; 3]) -> Class {
    if code[2] == 0 {
        Class::Whole
    } else {
        Class::Split
    }
}

fn valid_code(code: [u8; 3]) -> bool {
    (1..=9).contains(&code[0]) && (1..=9).contains(&code[1]) && code[2] <= 9
}

/// A 九方 code table: code → candidates in order.
#[derive(Debug, Default, Clone)]
pub struct Table {
    map: HashMap<[u8; 3], Vec<char>>,
}

impl Table {
    /// Parse the documented text format (data/SOURCES.md). Invalid lines
    /// are skipped; nothing is invented.
    pub fn parse(text: &str) -> Table {
        let mut map: HashMap<[u8; 3], Vec<char>> = HashMap::new();
        for raw in text.trim_start_matches('\u{feff}').lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (code_s, rest) = match line.find(char::is_whitespace) {
                Some(i) => (&line[..i], &line[i..]),
                None => continue,
            };
            let digits: Vec<u8> = code_s
                .chars()
                .filter_map(|c| c.to_digit(10).map(|d| d as u8))
                .collect();
            if digits.len() != 3 || code_s.chars().count() != 3 {
                continue;
            }
            let code = [digits[0], digits[1], digits[2]];
            if !valid_code(code) {
                continue;
            }
            let list = map.entry(code).or_default();
            for ch in rest.chars().filter(|c| !c.is_whitespace()) {
                if !list.contains(&ch) {
                    list.push(ch);
                }
            }
        }
        map.retain(|_, v| !v.is_empty());
        Table { map }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn code_count(&self) -> usize {
        self.map.len()
    }

    pub fn lookup(&self, code: [u8; 3]) -> &[char] {
        self.map.get(&code).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Every code that lists `ch` (used by tests and the golden set).
    pub fn codes_for(&self, ch: char) -> Vec<[u8; 3]> {
        let mut v: Vec<[u8; 3]> = self
            .map
            .iter()
            .filter(|(_, l)| l.contains(&ch))
            .map(|(k, _)| *k)
            .collect();
        v.sort();
        v
    }
}

/// Built-in table, or a user override when its text yields at least one code.
pub fn load_table(override_text: Option<&str>) -> Table {
    if let Some(text) = override_text {
        let t = Table::parse(text);
        if !t.is_empty() {
            return t;
        }
    }
    Table::parse(BUILTIN_TABLE)
}

static ACTIVE: OnceLock<Table> = OnceLock::new();

/// Install the table used by the app (call once at start-up). Later calls
/// are ignored.
pub fn install(table: Table) {
    let _ = ACTIVE.set(table);
}

/// The installed table, or the built-in one if none was installed.
pub fn active() -> &'static Table {
    ACTIVE.get_or_init(|| Table::parse(BUILTIN_TABLE))
}

/// Where the composition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Nothing typed; keys show the nine feature labels.
    Feature,
    /// Upper-left feature chosen; keys show its components.
    Component(u8),
    /// Feature and component chosen; keys show upper-right features and
    /// 0 is 確定.
    Third(u8, u8),
    /// Code complete; keys show candidates and 0 is 下頁.
    Candidates([u8; 3]),
}

/// Result of pressing a 九方 key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResult {
    /// The key means nothing in this state (for example 0 before a code,
    /// or a candidate key with no candidate).
    Ignored,
    /// A code key was accepted.
    Advanced,
    /// 0 moved to another candidate page.
    Paged,
    /// A candidate was chosen. The caller commits it and clears.
    Commit(char),
}

/// What a pad key shows right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Face {
    Feature(&'static str),
    Component(&'static str),
    Candidate(char),
    Empty,
    /// Key 0: `0` alone (inactive), `0 確定`, or `0 下頁`.
    Zero(&'static str),
}

/// Three-key composition with wrap-around paging.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Composer {
    keys: Vec<u8>,
    page: usize,
}

impl Composer {
    pub fn new() -> Self {
        Self::default()
    }

    /// The code keys typed so far (0–3 digits).
    pub fn keys(&self) -> &[u8] {
        &self.keys
    }

    pub fn is_active(&self) -> bool {
        !self.keys.is_empty()
    }

    pub fn page(&self) -> usize {
        self.page
    }

    pub fn stage(&self) -> Stage {
        match self.keys.as_slice() {
            [] => Stage::Feature,
            [f] => Stage::Component(*f),
            [f, c] => Stage::Third(*f, *c),
            [f, c, x, ..] => Stage::Candidates([*f, *c, *x]),
        }
    }

    pub fn code(&self) -> Option<[u8; 3]> {
        match self.stage() {
            Stage::Candidates(code) => Some(code),
            _ => None,
        }
    }

    pub fn class(&self) -> Option<Class> {
        self.code().map(class_of)
    }

    /// Code as shown in the hint row, e.g. `69 0` or `12 1`.
    pub fn code_text(&self) -> String {
        let d = |k: &u8| char::from(b'0' + *k);
        match self.keys.as_slice() {
            [] => String::new(),
            [f] => d(f).to_string(),
            [f, c] => format!("{}{}", d(f), d(c)),
            [f, c, x, ..] => format!("{}{} {}", d(f), d(c), d(x)),
        }
    }

    pub fn candidates<'t>(&self, table: &'t Table) -> &'t [char] {
        match self.code() {
            Some(code) => table.lookup(code),
            None => &[],
        }
    }

    pub fn page_count(&self, table: &Table) -> usize {
        self.candidates(table).len().div_ceil(PAGE_SIZE)
    }

    pub fn page_items<'t>(&self, table: &'t Table) -> &'t [char] {
        let all = self.candidates(table);
        let start = self.page * PAGE_SIZE;
        if start >= all.len() {
            return &[];
        }
        &all[start..(start + PAGE_SIZE).min(all.len())]
    }

    /// First candidate of the current page: what 空格 commits.
    pub fn first_on_page(&self, table: &Table) -> Option<char> {
        self.page_items(table).first().copied()
    }

    /// Next page, wrapping last → first. Returns whether the page changed.
    pub fn next_page(&mut self, table: &Table) -> bool {
        let n = self.page_count(table);
        if n <= 1 {
            return false;
        }
        self.page = (self.page + 1) % n;
        true
    }

    /// Previous page, wrapping first → last.
    pub fn prev_page(&mut self, table: &Table) -> bool {
        let n = self.page_count(table);
        if n <= 1 {
            return false;
        }
        self.page = (self.page + n - 1) % n;
        true
    }

    /// Press key `d` (0–9) on the pad or the physical numpad.
    pub fn press(&mut self, d: u8, table: &Table) -> KeyResult {
        if d > 9 {
            return KeyResult::Ignored;
        }
        match self.stage() {
            Stage::Feature | Stage::Component(_) => {
                if d == 0 {
                    KeyResult::Ignored
                } else {
                    self.keys.push(d);
                    KeyResult::Advanced
                }
            }
            Stage::Third(_, _) => {
                // 0 = 確定 (整體字); 1–9 = upper-right feature (分體字).
                self.keys.push(d);
                self.page = 0;
                KeyResult::Advanced
            }
            Stage::Candidates(_) => {
                if d == 0 {
                    if self.next_page(table) {
                        KeyResult::Paged
                    } else {
                        KeyResult::Ignored
                    }
                } else {
                    match self.page_items(table).get(usize::from(d) - 1) {
                        Some(ch) => KeyResult::Commit(*ch),
                        None => KeyResult::Ignored,
                    }
                }
            }
        }
    }

    /// Choose candidate `index` (0-based) on the current page.
    pub fn pick(&self, index: usize, table: &Table) -> Option<char> {
        self.page_items(table).get(index).copied()
    }

    /// 刪碼: remove exactly one key and return to the previous state.
    pub fn delete(&mut self) -> bool {
        self.page = 0;
        self.keys.pop().is_some()
    }

    /// 清除: drop the whole code and the candidates.
    pub fn clear(&mut self) {
        self.keys.clear();
        self.page = 0;
    }

    /// What pad key `key` (0–9) shows in the current state.
    pub fn face(&self, key: u8, table: &Table) -> Face {
        if key == 0 {
            return Face::Zero(match self.stage() {
                Stage::Third(_, _) => "確定",
                Stage::Candidates(_) => "下頁",
                _ => "",
            });
        }
        let i = usize::from(key.clamp(1, 9)) - 1;
        match self.stage() {
            Stage::Feature | Stage::Third(_, _) => Face::Feature(FEATURES[i]),
            Stage::Component(f) => Face::Component(COMPONENTS[usize::from(f) - 1][i]),
            Stage::Candidates(_) => match self.page_items(table).get(i) {
                Some(ch) => Face::Candidate(*ch),
                None => Face::Empty,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin() -> Table {
        Table::parse(BUILTIN_TABLE)
    }

    fn type_code(c: &mut Composer, t: &Table, keys: &[u8]) {
        for &k in keys {
            assert_eq!(c.press(k, t), KeyResult::Advanced, "key {k}");
        }
    }

    struct Golden {
        ch: char,
        class: Class,
        code: [u8; 3],
        page: usize,
    }

    /// The locked golden set (gate G2). Parsed from the committed file, not
    /// rewritten to match the code.
    fn golden() -> Vec<Golden> {
        let text = include_str!("../data/jiufang-golden.tsv");
        let mut out = Vec::new();
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let ch = cols[0].chars().next().unwrap();
            let class = match cols[1] {
                "整體" => Class::Whole,
                "分體" => Class::Split,
                other => panic!("bad class {other}"),
            };
            let k: Vec<u8> = cols[2]
                .split_whitespace()
                .map(|s| s.parse().unwrap())
                .collect();
            let page: usize = cols[3].parse().unwrap();
            out.push(Golden {
                ch,
                class,
                code: [k[0], k[1], k[2]],
                page,
            });
        }
        assert!(out.len() >= 20);
        out
    }

    fn golden_row(ch: char) -> Golden {
        golden().into_iter().find(|g| g.ch == ch).unwrap()
    }

    /// Drive the state machine like a user: type the code, page with 0 as
    /// needed, and report (class, page) where `ch` showed up.
    fn find_by_typing(t: &Table, code: [u8; 3], ch: char) -> Option<(Class, usize)> {
        let mut c = Composer::new();
        type_code(&mut c, t, &code);
        let class = c.class()?;
        for _ in 0..c.page_count(t).max(1) {
            if c.page_items(t).contains(&ch) {
                return Some((class, c.page() + 1));
            }
            c.press(0, t);
        }
        None
    }

    #[test]
    fn golden_set_agrees_with_engine() {
        let t = builtin();
        for g in golden() {
            assert_eq!(class_of(g.code), g.class, "{} class from code", g.ch);
            let found = find_by_typing(&t, g.code, g.ch);
            assert_eq!(found, Some((g.class, g.page)), "{} via {:?}", g.ch, g.code);
        }
    }

    #[test]
    fn jin_is_split() {
        let g = golden_row('禁');
        assert_eq!(g.class, Class::Split);
        assert_ne!(g.code[2], 0);
        assert_eq!(find_by_typing(&builtin(), g.code, '禁').unwrap().0, Class::Split);
        // Never filed as a whole character under the same head.
        assert!(!builtin().lookup([1, 2, 0]).contains(&'禁'));
    }

    #[test]
    fn xiang_is_split() {
        let g = golden_row('響');
        assert_eq!(g.class, Class::Split);
        assert_eq!(find_by_typing(&builtin(), g.code, '響').unwrap().0, Class::Split);
        assert!(builtin()
            .codes_for('響')
            .iter()
            .all(|c| class_of(*c) == Class::Split));
    }

    #[test]
    fn jing_is_whole() {
        let g = golden_row('晶');
        assert_eq!(g.class, Class::Whole);
        assert_eq!(find_by_typing(&builtin(), g.code, '晶').unwrap().0, Class::Whole);
        assert!(builtin()
            .codes_for('晶')
            .iter()
            .all(|c| class_of(*c) == Class::Whole));
    }

    #[test]
    fn lei_is_whole() {
        let g = golden_row('壘');
        assert_eq!(g.class, Class::Whole);
        assert_eq!(find_by_typing(&builtin(), g.code, '壘').unwrap().0, Class::Whole);
    }

    #[test]
    fn whole_sequence_ends_with_zero_before_candidates() {
        let t = builtin();
        let mut c = Composer::new();
        // 前 = 4 7 0 (teaching example).
        c.press(4, &t);
        c.press(7, &t);
        assert_eq!(c.stage(), Stage::Third(4, 7));
        assert!(c.candidates(&t).is_empty(), "no candidates before 0");
        assert_eq!(c.face(0, &t), Face::Zero("確定"));
        assert_eq!(c.press(0, &t), KeyResult::Advanced);
        assert_eq!(c.stage(), Stage::Candidates([4, 7, 0]));
        assert_eq!(c.class(), Some(Class::Whole));
        let pos = c.page_items(&t).iter().position(|x| *x == '前').unwrap();
        assert_eq!(c.press(pos as u8 + 1, &t), KeyResult::Commit('前'));
        for (ch, code) in [('月', [3, 9, 0]), ('光', [8, 5, 0]), ('是', [6, 9, 0]), ('上', [8, 8, 0])] {
            assert_eq!(find_by_typing(&t, code, ch).map(|f| f.0), Some(Class::Whole), "{ch}");
        }
    }

    #[test]
    fn split_sequence_third_key_is_a_feature() {
        let t = builtin();
        let mut c = Composer::new();
        // 明 = 6 9 3: third key is the upper-right feature 3 (缺, for 月).
        type_code(&mut c, &t, &[6, 9]);
        assert_eq!(c.face(3, &t), Face::Feature("缺"));
        assert_eq!(c.press(3, &t), KeyResult::Advanced);
        assert_eq!(c.class(), Some(Class::Split));
        assert!(c.page_items(&t).contains(&'明'));
        for ch in ['禁', '響', '港', '林', '你', '好'] {
            let g = golden_row(ch);
            assert!((1..=9).contains(&g.code[2]), "{ch}");
        }
    }

    #[test]
    fn zero_pages_without_reconfirming_or_clearing() {
        let t = builtin();
        let mut c = Composer::new();
        type_code(&mut c, &t, &[6, 9, 0]);
        assert!(c.page_count(&t) >= 2);
        let first = c.page_items(&t).to_vec();
        assert_eq!(c.face(0, &t), Face::Zero("下頁"));
        assert_eq!(c.press(0, &t), KeyResult::Paged);
        assert_eq!(c.page(), 1);
        assert_eq!(c.keys(), &[6, 9, 0], "code unchanged by paging");
        assert_ne!(c.page_items(&t), first.as_slice());
        assert!(c.is_active());
    }

    #[test]
    fn zero_on_last_page_wraps_to_first() {
        let t = builtin();
        let mut c = Composer::new();
        type_code(&mut c, &t, &[6, 9, 0]);
        let first = c.page_items(&t).to_vec();
        let n = c.page_count(&t);
        for _ in 0..n - 1 {
            assert_eq!(c.press(0, &t), KeyResult::Paged);
        }
        assert_eq!(c.page(), n - 1);
        assert_eq!(c.press(0, &t), KeyResult::Paged);
        assert_eq!(c.page(), 0);
        assert_eq!(c.page_items(&t), first.as_slice());
    }

    #[test]
    fn wheel_paging_wraps_both_ways() {
        // The wheel uses next_page / prev_page; unlike 速成 it wraps.
        let t = builtin();
        let mut c = Composer::new();
        type_code(&mut c, &t, &[1, 3, 0]);
        let n = c.page_count(&t);
        assert!(n >= 2);
        assert!(c.prev_page(&t));
        assert_eq!(c.page(), n - 1, "wheel back from first wraps to last");
        assert!(c.next_page(&t));
        assert_eq!(c.page(), 0, "wheel forward from last wraps to first");
        // 速成 still stops at the ends.
        assert!(!crate::simplex::can_page_prev(0));
    }

    #[test]
    fn delete_removes_one_code_and_restores_faces() {
        let t = builtin();
        let mut c = Composer::new();
        assert_eq!(c.face(7, &t), Face::Feature("撇"));
        c.press(6, &t);
        assert_eq!(c.face(9, &t), Face::Component("日"));
        c.press(9, &t);
        let third_faces: Vec<Face> = (0..=9).map(|k| c.face(k, &t)).collect();
        c.press(0, &t);
        c.press(0, &t); // page 2
        assert!(matches!(c.face(1, &t), Face::Candidate(_)));
        assert!(c.delete());
        assert_eq!(c.stage(), Stage::Third(6, 9));
        assert_eq!(c.page(), 0);
        let restored: Vec<Face> = (0..=9).map(|k| c.face(k, &t)).collect();
        assert_eq!(restored, third_faces);
        assert!(c.delete());
        assert_eq!(c.stage(), Stage::Component(6));
        assert_eq!(c.face(9, &t), Face::Component("日"));
        assert!(c.delete());
        assert_eq!(c.stage(), Stage::Feature);
        assert_eq!(c.face(7, &t), Face::Feature("撇"));
        assert!(!c.delete());
    }

    #[test]
    fn empty_or_unknown_codes_invent_nothing() {
        let t = builtin();
        let mut c = Composer::new();
        assert_eq!(c.press(0, &t), KeyResult::Ignored, "0 is not a feature");
        // 2 1 9 is a valid shape code that the starter table does not list.
        type_code(&mut c, &t, &[2, 1, 9]);
        assert!(t.lookup([2, 1, 9]).is_empty());
        assert!(c.candidates(&t).is_empty());
        assert!(c.page_items(&t).is_empty());
        assert_eq!(c.first_on_page(&t), None);
        for k in 1..=9 {
            assert_eq!(c.press(k, &t), KeyResult::Ignored);
            assert_eq!(c.face(k, &t), Face::Empty);
        }
        assert_eq!(c.press(0, &t), KeyResult::Ignored);
        assert_eq!(c.keys(), &[2, 1, 9]);
        // An empty table has no candidates at all.
        let empty = Table::parse("");
        assert!(empty.lookup([6, 9, 0]).is_empty());
    }

    #[test]
    fn table_parses_and_user_override_replaces_builtin() {
        let t = builtin();
        assert!(t.code_count() > 150);
        let text = "\u{feff}# my table\r\n690 日 是\r\n690 早是\r\n999x 不要\r\n012 無效\r\n12 短\r\n𠀀\r\n693 明𠀾\r\n";
        let user = load_table(Some(text));
        assert_eq!(user.lookup([6, 9, 0]), &['日', '是', '早']);
        assert_eq!(user.lookup([6, 9, 3]), &['明', '𠀾'], "non-BMP allowed");
        assert_eq!(user.code_count(), 2, "invalid lines skipped");
        assert!(user.lookup([1, 3, 0]).is_empty(), "override replaces built-in");
        // An override with no valid line falls back to the built-in table.
        let fallback = load_table(Some("# nothing\nabc\n"));
        assert_eq!(fallback.code_count(), t.code_count());
        assert_eq!(load_table(None).code_count(), t.code_count());
    }

    #[test]
    fn faces_follow_computer_numpad() {
        assert_eq!(PAD_ROWS[0], [7, 8, 9], "computer keypad, not telephone");
        let t = builtin();
        let c = Composer::new();
        let top: Vec<Face> = PAD_ROWS[0].iter().map(|k| c.face(*k, &t)).collect();
        assert_eq!(
            top,
            vec![Face::Feature("撇"), Face::Feature("橫"), Face::Feature("豎")]
        );
        assert_eq!(c.face(1, &t), Face::Feature("十"));
        assert_eq!(c.face(5, &t), Face::Feature("圓"));
        assert_eq!(c.face(3, &t), Face::Feature("缺"));
    }
}
