# on99KB

A resizable, always-on-top Windows on-screen keyboard for the classic IBM **CP437** / DOS Alt+numpad characters (Alt+1 through Alt+255) and Hong Kong **速成** (Simplex / Quick, first and last Cangjie codes), including HKSCS-2016 characters as completely as practical.

Clicking a key types that character into the **previously focused** window. The keyboard itself does not steal focus, so you can keep Notepad, a browser, or a terminal focused and use this tool to enter symbols such as ☺ ♥ ⌂ █ α, or 速成 characters such as 嘅 唔 係.

點擊按鍵會把該字元輸入到**先前聚焦**的視窗。鍵盤本身不會搶走焦點。

## Features

- Native Win32 GUI (Rust + [`windows`](https://crates.io/crates/windows) crate). No webview, no admin rights, no telemetry, no hooks.
- Two modes, switched from a segmented control on the **bottom bar** (`CP437｜速成`). The window title shows the current mode. The last mode (**CP437** or **速成**) is remembered in `%APPDATA%\on99KB\window.ini` as `mode=cp437` or `mode=simplex`, and is restored when you open the app again. The mode is written when you switch it, not only when you close the window. A missing or invalid `mode` (or no ini) still defaults to CP437. The 符號 pad is not remembered.
- All **255** CP437 glyphs are in the table (codes 1..=255). Code 255 is a non-breaking space U+00A0 (`不換行空格`). The former Alt+32 space *cell* is labelled **BS** and sends Backspace (`VK_BACK`) to the previously focused window instead of a space character (the U+0020 mapping itself is unchanged in the library).
- **速成** mode: a standard Cangjie letter-to-radical keyboard (every key shows the radical and the letter), a top candidate bar of nine characters with paging, and 刪碼 / 清除 / 空格. A compact row of digits `1–9 0` plus **Bksp**, and a row of fullwidth `，` `。` `：` `；` plus Home / End / PgUp / PgDn, sit under the candidate bar (they stay usable while the 符號 pad is open). Digits and punctuation are typed with Unicode `SendInput`; Bksp cancels the current 速成 composition (clears the codes and candidates) when a code or candidate is in progress, and otherwise sends `VK_BACK`; the nav keys send `VK_HOME` / `VK_END` / `VK_PRIOR` / `VK_NEXT` (extended) into the previously focused window. **刪碼** only deletes one 速成 composition code — it is not Backspace. Below the radical keyboard (and below the 符號 pad when that is open), a lowercase English QWERTY keyboard (`qwertyuiop` / `asdfghjkl` / `zxcvbnm,.`) occupies the remaining band above the action keys. A short press types the lowercase letter (or `,` / `.`) with Unicode `SendInput`; holding the same key for about 350 ms types the uppercase letter instead, or `<` / `>` for the comma and period keys (the key label switches to the shifted character; there is no Caps Lock or sticky shift, and other keys are unaffected). English keys also show the long-press result (`A`, `<`, `>`) in the top-right corner, like a keycap. They are visually distinct from the 速成 radical keys, which still add composition codes. Only first + last codes (not full Cangjie). Hong Kong characters share the same candidate bar. Common spoken-Cantonese characters are promoted earlier in their lists. Candidate order is **not** claimed to match Microsoft 速成.
- **符號** button (速成 layout only): opens the publicly documented Microsoft 倉頡 / 速成 Ctrl+Alt+, symbol keyboard (from 「┌」 through 「※」, plus the rest of that same official keyboard). Clicking a symbol uses `SendInput`. The physical Ctrl+Alt+, shortcut is not implemented. See [Sources](#sources).
- In 速成 mode, the mouse wheel over the keyboard window pages the candidate list (wheel away / positive delta = previous page, wheel toward / negative = next page), including when the window is inactive and has not been clicked first. There is no wrap at the ends, matching the on-screen 上／下 buttons. CP437 ignores the wheel. No mouse hook is installed.
- Window opacity can be cycled with a `100%` / `50%` / `25%` button on the bottom bar, immediately to the right of `CP437｜速成`, in both modes. `100%` is fully opaque and is the default; `50%` and `25%` use layered-window alpha 128 and 64. The choice is remembered in `%APPDATA%\on99KB\window.ini` with the window size, position, always-on-top setting, and mode.
- A **Space** bar sits on the bottom status bar in both modes, to the right of `CP437｜速成` and the opacity button, and types a normal space (U+0020) with Unicode `SendInput`. It is separate from the 速成 action-row **空格**, which still commits the first candidate and does not type U+0020. The CP437 Alt+32 cell remains **BS**.
- Mouse / on-screen keyboard only. No focus steal (`WS_EX_NOACTIVATE` + `WM_MOUSEACTIVATE` / `MA_NOACTIVATE`) and no hooks. Physical 1–9 / Space are not handled.
- First launch (no saved size) opens at the compact CP437 client size **480×460**, matching the measured reference screenshot (the PNG is 495×510 including a teal desktop margin; the window itself is about 482×492 with a 31 px title bar; the client area is exactly 480×460). The bottom status bar stays visible (~28 px). This is the actual default open size, not only a minimum the user has to drag down to. After you resize or move the window, or change opacity or always-on-top, those settings are remembered in `%APPDATA%\on99KB\window.ini`: client width and height, window position (`x` / `y`), `opacity` (25 / 50 / 100), `topmost` (1 = on, 0 = off), and `mode` (`cp437` / `simplex`). An older ini that lacks the new keys keeps a valid size and position; missing opacity is treated as 100%, missing topmost as on, and missing mode as CP437. Saving also happens when you finish dragging the window (`WM_EXITSIZEMOVE`) and when you close it. Switching into 速成 grows the window just enough (`SWP_NOACTIVATE`) if the current height would clip the candidate bar, the digit / punctuation / nav rows, a readable radical keyboard, the lowercase English QWERTY band, the compact action row, or the bottom bar. The CP437 first-launch / minimum client size remains **480×460**.
- Always-on-top is on by default, and can be toggled with the `on99KB` brand control (the first CP437 grid cell, and the same chip at the left of the 速成 candidate bar). When on, the chip is bright / selected; when off, it is darker and flatter. The label stays `on99KB` (it may ellipsize to `on...` on a tight window). The window does not steal focus when you toggle it.
- The window can be pinned to the Windows taskbar. It uses `WS_EX_APPWINDOW` (not `WS_EX_TOOLWINDOW`) and the stable AppUserModelID `TimothyLam1017.on99KB`, so a pinned shortcut keeps working across versions. A side effect is that the window may also appear in Alt+Tab.

- Types the actual Unicode glyph with `SendInput` and `KEYEVENTF_UNICODE` (including non-BMP characters; not a simulated Alt+numpad sequence).
- In CP437, each key shows the glyph and the `Alt+n` code when there is room. At the compact default size, keys are only about 22 design-pixels tall, so the caption shrinks to the bare Alt number in a thin band instead of hiding. The full `Alt+n` caption appears on taller keys. Tight truncation of long labels (for example `on...`, `不...`) is expected at that size.
- Control keys are visually distinct from typing keys: an amber outline and label for window / target-app controls (the `on99KB` topmost chip and CP437 cell 0, CP437 cell 32 **BS**, **Bksp**, and the opacity button), and a blue outline and label for 速成 composition keys (**刪碼**, **清除**, **空格**, **符號**).
- The status hint has its own full-width row immediately above the bottom control bar, so it is never squeezed into the leftover space beside the **Space** bar.
- The layout is specified in 96-DPI design pixels and scaled to the window's current DPI (`150%`, `200%`, and so on), including the new hint row, key sizes, paddings, fonts, and minimum window sizes. `%APPDATA%\on99KB\window.ini` stores the client size in design pixels with a `units=dip` marker. An older ini without that marker is treated as physical pixels and converted.
- Fonts: Segoe UI Symbol for CP437 glyphs, Microsoft JhengHei for Chinese labels, Segoe UI for Latin captions. 速成 candidates use Microsoft JhengHei, then MingLiU_HKSCS, then MingLiU_HKSCS-ExtB, so rare HKSCS / Ext-B glyphs are less likely to tofu.
- Application icon (from `assets/icon.png`) is embedded in the exe and used for the window.
- Screen readers: the keyboard window exposes each on-screen key to MSAA / UI Automation clients (Narrator, NVDA, JAWS) with a name such as `♥, Alt+3, U+2665` or `日, code A`, a push-button role, hover / pressed / on-off state, and its location. A screen reader's “click” / default action presses the key just like the mouse.
- Redraws are incremental: hovering or pressing a key repaints only that key (with a small DPI-scaled margin) and the status hint. The 速成 layout is cached by window size and the current window DPI; after a DPI change the cache is rebuilt and the screen-reader tree is refreshed.

## Run the exe

Download `on99KB.exe` from the [Releases](https://github.com/TimothyLam1017/on99KB/releases) page, or build it yourself.

1. Double-click `on99KB.exe`. A compact window opens in the last used mode (CP437 if you have not chosen one yet) and stays on top by default (you can unpin it with the `on99KB` brand key).
2. Click the window you want to type into (Notepad, a browser, a terminal, …) so it has focus.
3. Click a key on on99KB. The character is inserted into that focused window.
4. Use `CP437｜速成` on the bottom bar to switch mode. In 速成, click radicals (at most two codes), then a candidate or the action-row 空格 to commit. The bottom-bar **Space** key types a space character in both modes. 符號 opens the Ctrl+Alt+, symbol keyboard. Digits, fullwidth punctuation, Home/End/PgUp/PgDn, Bksp (or, if a 速成 composition is in progress, Bksp cancels that composition instead), and the English letter rows type into the previously focused window (short press lowercase, hold about 350 ms for uppercase; `,` / `.` on the bottom English row similarly type a comma or period, or `<` / `>` when held; the English keys are not 速成 codes). Roll the mouse wheel over the 速成 window to page candidates. Use the `100%` / `50%` / `25%` button beside the mode switch to change window opacity. Click the `on99KB` brand key to toggle always-on-top. In CP437, the **BS** key (the old Alt+32 space cell) also sends Backspace.
5. Drag any window edge or corner to resize; the keys scale automatically. The last size is restored the next time you open the app.
6. Close it with the window’s close button.

The first CP437 grid cell (`on99KB`) and the matching 速成 brand chip are the always-on-top toggle and do not type anything. The nine candidate slots, paging controls, and current-code display remain.

## Build on Windows

Requirements: [Rust](https://rustup.rs/) (stable) and the MSVC or GNU toolchain.

```bat
git clone https://github.com/TimothyLam1017/on99KB.git
cd on99KB
cargo test --lib
cargo build --release
```

The exe is `target\release\on99KB.exe`.

## Cross-compile from Linux

Requirements: Rust stable, the `x86_64-pc-windows-gnu` target, and MinGW-w64 (including `x86_64-w64-mingw32-windres` for the icon resource).

```bash
rustup default stable
rustup target add x86_64-pc-windows-gnu
# Debian / Ubuntu:
sudo apt-get install -y mingw-w64

git clone https://github.com/TimothyLam1017/on99KB.git
cd on99KB
cargo test --lib
cargo build --release --target x86_64-pc-windows-gnu
```

The exe is `target/x86_64-pc-windows-gnu/release/on99KB.exe`.

This repository’s `.cargo/config.toml` points the GNU target at `x86_64-w64-mingw32-gcc` and passes `-static` so the exe does not depend on MinGW runtime DLLs (`libgcc_s_seh-1.dll`, `libwinpthread-1.dll`, and so on). `build.rs` invokes `x86_64-w64-mingw32-windres` to embed `assets/icon.ico`.

## Character set (CP437)

The CP437 layout is a 16×16 grid (256 cells). Cell 0 is a title cell. Cells 1–255 are CP437:

| Code | Character | Notes |
|------|-----------|--------|
| 1 | ☺ | |
| 3 | ♥ | |
| 32 | space | cell labelled BS (sends Backspace, not a space) |
| 65 | A | |
| 127 | ⌂ | |
| 219 | █ | |
| 224 | α | |
| 255 | U+00A0 | labelled 不換行空格 |

The mapping is covered by unit tests against the full requested list (`cargo test --lib`).

## Sources

The 速成 codebook is compiled into the exe. You do not need to keep any data file beside `on99KB.exe`. Full URLs, revisions, checksums, and merge rules are in [`data/SOURCES.md`](data/SOURCES.md).

- **OpenVanilla `simplex-ext.cin`** (簡易／大字集, MIT DataTables / OpenVanilla MIT Licence), last changed in commit `69aa216a2429d1bffa8533666bd8d2ce07a34d6f` (2012-08-03):  
  https://github.com/openvanilla/openvanilla/blob/69aa216a2429d1bffa8533666bd8d2ce07a34d6f/DataTables/simplex-ext.cin  
  Licence text: [`data/LICENSE-OpenVanilla.txt`](data/LICENSE-OpenVanilla.txt).
- **HKSCS-2016** public JSON from the Digital Policy Office:  
  https://www.digitalpolicy.gov.hk/open_data/ccli/HKSCS2016.json  
  Catalogue: https://data.gov.hk/en-data/dataset/hk-dpo-dpo_hp-hong-kong-supplementary-character-set-related-information  
  Terms: https://data.gov.hk/en/terms-and-conditions  

  This project cites that source and acknowledges the intellectual property of the Government of the Hong Kong Special Administrative Region in the dataset. See [`data/HKSCS-ATTRIBUTION.md`](data/HKSCS-ATTRIBUTION.md). This is attribution, not legal advice.

Merge rule: start from `simplex-ext.cin` order; add any HKSCS-2016 character that has a Cangjie code and is still missing, under the 速成 key of that official code (first + last letters). Codes are not invented. Common spoken-Cantonese characters already in the table are moved earlier in their lists; the documented list is `PROMOTED_CANTONESE` in `src/simplex.rs`.

The 符號 keyboard follows Microsoft’s documented Ctrl+Alt+comma shortcut for 微軟倉頡 / 微軟速成 (a 符號鍵盤 / 輸入法整合器, not a private IME dictionary):

- https://support.microsoft.com/zh-tw/windows/hardware/input-devices/microsoft-traditional-chinese-ime
- Key maps: https://rakutentw.pixnet.net/blog/posts/11404319206 (2019, primary) and https://water0622.pixnet.net/blog/posts/4043845348 (2017). Key 7 is ’ (the 2017 ˙ looks like a typo); key D is ┘ (the 2017 table skips D). See `data/SOURCES.md`.

## Known limitations

- Mouse only: physical 1–9, Space, and Ctrl+Alt+, are not handled.
- 速成 candidate order does not match Microsoft 速成, and is not claimed to.
- v1 does not implement full Cangjie, association words (聯想詞), writing back into a folder, or IME hooks.
- `SendInput` is still subject to User Interface Privilege Isolation (UIPI): this tool cannot type into a window that is running at a higher integrity level (for example an elevated Administrator window) unless on99KB is also elevated.
