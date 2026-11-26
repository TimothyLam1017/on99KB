# 九方 字型 typography rules (locked for on99KB 1.3.0)

Status: **locked** (gate G1). Written and committed before any 九方 feature
code. The engine (`src/q9.rs`), its unit tests, and the golden set
(`data/jiufang-golden.tsv`, gate G2) must agree with this document. If code
and this document disagree, the code is wrong.

「九方」 is used on screen as the **common, unofficial** name for this
shape-input method (九方 (shape input; computer numpad)). on99KB is **not
affiliated with, not licensed by, and not supported by 九方科技**. Nothing in
this document is copied from 九方科技 software, tables, artwork, fonts or
screenshots, and nothing here is legal clearance.

Scope: the **computer** 字型 / 字形 (shape) method only. Out of scope: 筆劃,
拼音 / 注音 / 粵拼, 姓氏 shortcuts, association / 聯想, the official
punctuation / emoji pad, the official `/` mode switch, laptop letter-cluster
mapping (789 / UIO / JKL), and any unofficial extras that may still be
patented. Only the basic shape input described in public tutorials and in the
expired patent HK1035043 (US 6,922,811) is implemented.

## 1. Keypad geometry (computer, not telephone)

```
┌─────┬─────┬─────┐
│  7  │  8  │  9  │
├─────┼─────┼─────┤
│  4  │  5  │  6  │
├─────┼─────┼─────┤
│  1  │  2  │  3  │
├─────┴─────┼─────┘
│  0  確定  │
└───────────┘
```

- Top row is **7 8 9** (a computer numpad), never the telephone 1 2 3.
- Key **0** is a wide key spanning columns 1–2, labelled `0` and `確定`.
  Once candidates are visible it is labelled `0` and `下頁`.
- Key 0 is **not** a feature key and **not** a tenth shape class.

Source: Clement Yui-Wah Lee, *How to Use the Q9 Input Method (九方輸入法)*,
CC BY 3.0, http://bit.ly/Q9Tutorial (computer vs telephone keypad; 0 = 確定
for 整體字 and next page).

## 2. The split test (整體字 vs 分體字)

Inspect **only the upper / top half of the character (字首)**.

- If that upper part **can** be split left–right → **分體字**.
- If it **cannot** → **整體字**.
- Never classify from the whole glyph or from the lower half.

Official pages call an unsplittable character a **整體字**. Some teaching
materials say **獨體字** for the same idea; on99KB treats 獨體字 as a synonym in
docs and comments but always shows **整體字** on screen and in the README.

Public teaching examples (course slides “九方中文輸入法” attributed to the
inventors 梁立人 and 劉文建):

| Character | Upper part | Class | Why |
|---|---|---|---|
| 晶 | 日 | 整體 | upper 日 does not split, even though the lower 日日 would |
| 壘 | 田 | 整體 | upper 田 does not split, even though the lower part would |
| 禁 | 林 | 分體 | upper 林 splits 木｜木, even though the lower 示 would not |
| 響 | 鄉 | 分體 | upper 鄉 splits 乡｜…｜阝, even though the lower 音 would not |

Official tutorials (九方科技, https://qcode.com/web10/page.php?c=130&newLang=tc)
use 香 as a 整體字 example and 港 as a 分體字 example.

## 3. Key sequences

**整體字:** upper / upper-left feature (1–9) → component under that feature
(1–9) → **0 (確定)** → candidates appear → pick with 1–9.

**分體字:** upper-left feature (1–9) → component of the left part (1–9) →
**upper-right feature (1–9, never 0)** → candidates appear → pick with 1–9.

A complete code is therefore always three keys `F C X`, where `X = 0` means
整體 and `X = 1…9` is the upper-right feature of a 分體字. The procedure
follows 九方科技’s public input steps (電腦版), which say: key 1 = 字首（左上角）
特徵; key 2 = 該特徵下的部件; 整體字 press 確定「0」 then pick 「1,2…9」; 分體字
key 3 = 右上角特徵, then pick.

Public teaching examples of whole-character codes (computer keypad), cited as
**teaching examples, not an official extracted table** (Clement Yui-Wah Lee,
CC BY 3.0): 前 = 470, 月 = 390, 光 = 850, 是 = 690, 上 = 880. on99KB’s own
component chart (§5) is arranged so that these five examples hold.

## 4. The 0-key state machine

States (the composition is empty in `Feature` with no keys typed):

| State | Pad shows (keys 1–9) | Key 0 shows | 1–9 does | 0 does |
|---|---|---|---|---|
| `Feature` | the nine feature labels | `0` (inactive) | choose upper-left feature → `Component(F)` | nothing |
| `Component(F)` | the nine components under F | `0` (inactive) | choose component → `Third(F,C)` | nothing |
| `Third(F,C)` | the nine features again (upper-right) | `0 確定` | 分體: code `F C X` → `Candidates` | 整體: code `F C 0` → `Candidates` |
| `Candidates(code, page)` | the current page of nine candidates, candidate *n* on key *n* | `0 下頁` | commit candidate *n* of the page (if present) | **next page**; on the last page **wrap to the first page** |

Rules:

1. Once candidates are visible, **0 pages forward**. It never re-confirms and
   never clears the composition.
2. Paging **wraps**: last page → first page. A one-page list stays on page 1.
3. The mouse wheel in 九方 also pages and **also wraps** (forward: last →
   first; back: first → last). This **differs from 速成**, whose wheel and
   上／下 buttons stop at the ends. The 九方 上頁／下頁 buttons wrap like the
   wheel.
4. **刪碼** removes exactly one key and restores the previous state and key
   faces: `Candidates → Third → Component → Feature`. It never sends
   Backspace.
5. **清除** clears the whole code and the candidates, and dismisses the
   overlay if one is open.
6. **空格** (action row) commits the **first candidate of the current page**
   (that candidate is highlighted). It never types U+0020. The bottom-bar
   **Space** still types U+0020.
7. **Bksp**: if a 九方 composition (any key typed) or the overlay is active,
   clear that composition, dismiss the overlay, and do **not** send
   `VK_BACK`. Otherwise send `VK_BACK`.
8. Committing a character sends it with Unicode `SendInput`, then clears the
   composition and dismisses the overlay.
9. An empty or unknown code shows **no candidates** (an explicit empty state).
   on99KB never invents a character. 1–9 and 0 do nothing in that state;
   刪碼 / 清除 / Bksp still work.
10. Lookup is always against Traditional characters. The 繁｜簡 output setting
    never changes codes or lookup.

Out of v1 required scope (possible later work): numpad `.` as cancel and
numpad Enter as 空格.

## 5. Feature labels and component charts

### 5.1 The nine feature categories

Publicly taught labels for the computer keypad (course slides “九方中文輸入法”
attributed to 梁立人 and 劉文建, as widely circulated in teaching copies):

```
7 撇    8 橫    9 豎
4 折    5 圓    6 點
1 十    2 交    3 缺
```

These labels are **not** a quotation from qcode.com (the official help pages
describe the procedure but do not print a labelled 3×3 chart in their HTML
text). on99KB draws its own plain key faces from these labels; no official
artwork, font or screenshot is used.

### 5.2 Component charts (on99KB’s own, independently constructed)

The nine components under each feature are on99KB’s **own** grouping, made
for this project from the public feature labels above and ordinary knowledge
of character shapes. They are **not** the official 九方 component charts and
are not claimed to match them. Each component is shown on the key by a
representative character. Where a public teaching example fixes a position,
the chart follows it (marked †).

For every feature, component *n* sits on key *n*, so components 7 8 9 are
the top row of the pad and 1 2 3 the bottom row.

| Feature | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
|---|---|---|---|---|---|---|---|---|---|
| 1 十 | 十 | 木 | 艹 | 廿 | 寸 | 左 (𠂇 head of 左右有在) | 耂 | 丰 | 車 |
| 2 交 | 又 | 文 | 大 | 父 | 夂 | 九 | 乂 | 支 | 戈 |
| 3 缺 | 冖 | 厂 | 尸 | 戶 | 匚 | 凵 | 門 | 几 | 月 † (月 = 390) |
| 4 折 | 乙 | 阝 | 幺 (also 糸, 乡) | 弓 | 女 | 子 | 丷 † (前 = 470) | 己 | 力 |
| 5 圓 | 田 | 囗 | 罒 | 白 | 自 | 貝 | 見 | 西 | 目 |
| 6 點 | 氵 | 宀 | 亠 | 言 | 广 | 冫 | 忄 | 礻 (also 衤) | 日 † (是 = 690) |
| 7 撇 | 亻 | 彳 | 人 | 禾 | 竹 | 金 | 牛 | 夕 | 犭 |
| 8 橫 | 一 | 扌 | 王 | 土 (also 士) | 小 † (光 = 850) | 工 | 石 | 卜 † (上 = 880) | 雨 |
| 9 豎 | 口 | 山 | 中 | 冂 | 足 | 虫 | 止 | 由 | 火 |

The upper-right feature of a 分體字 (third key) is the feature whose row in
this table contains the head of the right part. For example the right part of
林 is 木 (row 1 十) so 林 = `1 2 1`; the right part of 鄉 / 響 is 阝 (row 4 折)
so 響 = `4 3 4`.

### 5.3 Key faces (part of the method, not decoration)

After every key the 3×3 faces update:

1. `Feature`: the nine feature labels (撇 橫 豎 / 折 圓 點 / 十 交 缺).
2. `Component(F)`: the nine components of feature F from §5.2.
3. `Third(F,C)`: the nine feature labels again, read as upper-right
   features; 0 reads `確定`.
4. `Candidates`: the current page of nine candidates in numpad positions
   (candidate 1 on key 1 at the bottom-left … candidate 9 on key 9 at the
   top-right). Keys without a candidate are blank. 0 reads `下頁`.

## 6. Table and override

The starter table is `data/jiufang-shape.txt` (format in
`data/SOURCES.md` and the README). It is independently constructed from §2–§5
and is deliberately incomplete. Users may install their own table in the same
format at `%APPDATA%\on99KB\jiufang.txt`; when that file exists and parses,
it replaces the built-in table. Candidate order is on99KB’s own and is not
claimed to match official Q9 software.

## Citations

- 九方科技, input steps (電腦版): https://qcode.com/web10/page.php?c=130&newLang=tc
- 九方科技, 簡易說明 (字型輸入法): https://www.qcode.com/web10/page.php?c=132&newLang=tc
  (also https://www.q9tech.com/web10/page.php?c=132)
- Course slides “九方中文輸入法”, attributed to 梁立人、劉文建 (widely circulated
  teaching copies): computer keypad feature labels 7撇 8橫 9豎 / 4折 5圓 6點 /
  1十 2交 3缺; split test on the upper part only; examples 晶、壘 vs 禁、響.
- Clement Yui-Wah Lee, “How to Use the Q9 Input Method (九方輸入法)”, CC BY 3.0,
  http://bit.ly/Q9Tutorial: computer vs telephone keypad; 0 = 確定 / next
  page; examples 前=470, 月=390, 光=850, 是=690, 上=880.
- Wikipedia (zh), 九方輸入法 (background only):
  https://zh.wikipedia.org/zh-hant/九方輸入法
- HK patent HK1035043 / US 6,922,811 (filed 1 November 1999), treated as
  expired; basic shape input only. Patent expiry is not a licence to copy
  official software or tables, and this document is not legal advice.
