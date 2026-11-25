# Codebook and symbol-table sources

This file records the exact upstream sources vendored into `data/` and
used to build the embedded 速成 table, plus the public references for
the Ctrl+Alt+, symbol keyboard. on99KB does not embed any Microsoft IME
private dictionary or binary.

## OpenVanilla `simplex-ext.cin` (base 速成 / 簡易 table)

- Project: [openvanilla/openvanilla](https://github.com/openvanilla/openvanilla)
- File: [`DataTables/simplex-ext.cin`](https://github.com/openvanilla/openvanilla/blob/69aa216a2429d1bffa8533666bd8d2ce07a34d6f/DataTables/simplex-ext.cin)
- Raw URL used: `https://raw.githubusercontent.com/openvanilla/openvanilla/69aa216a2429d1bffa8533666bd8d2ce07a34d6f/DataTables/simplex-ext.cin`
- Last commit that changed this file: `69aa216a2429d1bffa8533666bd8d2ce07a34d6f` (2012-08-03T05:54:11Z)
- The file is unchanged on `master` as of repo HEAD `ecf3260a49e61aacf4ffc951d40d390a46dc4cec` (2026-10-28).
- Vendored copy: `data/simplex-ext.cin`
- SHA-256: `8d925bfbd524dbc6042a1ce89912aadc17d8aa34109410a22948fd07c2d006e9`
- File header: “CNS11643 and Unicode compatible characters based on opendesktop.org.tw’s simplex.cin”, work by yylin and b6s. The table is 簡易（大字集）/ Simplex (Large Character Set), i.e. 速成 / first-and-last Cangjie codes, not full Cangjie and not Cangjie V / Rime.
- Licence: the OpenVanilla project is released under the MIT License. The project `LICENSE.txt` (vendored as `data/LICENSE-OpenVanilla.txt`, taken from the same repo at `ecf3260a49e61aacf4ffc951d40d390a46dc4cec`) says that each data table under `DataTables/` carries its own licence note. `simplex-ext.cin` itself has only the authorship header above and no additional restrictive licence. The MIT licence text is reproduced in `data/LICENSE-OpenVanilla.txt` and must be retained.

## HKSCS-2016 (gap fill)

- Dataset: Hong Kong Supplementary Character Set related information, Digital Policy Office
- JSON URL: `https://www.digitalpolicy.gov.hk/open_data/ccli/HKSCS2016.json`
- Catalogue: `https://data.gov.hk/en-data/dataset/hk-dpo-dpo_hp-hong-kong-supplementary-character-set-related-information`
- Data dictionary: `https://www.digitalpolicy.gov.hk/open_data/ccli/hkscs2016-data-dictionary-en.pdf`
- Terms of use: `https://data.gov.hk/en/terms-and-conditions`
- Vendored copy: `data/HKSCS2016.json` (UTF-8 with BOM, 5033 entries)
- SHA-256: `a28a3b74e469726c7df2174346e9a3992760f5842c02ac07b834529bc5b931e5`
- Retrieved: 2026-11-25
- Attribution and acknowledgement of the Government’s intellectual property: see `data/HKSCS-ATTRIBUTION.md`.

### Merge rule

1. Start from `simplex-ext.cin` order (letter keys of length 1 or 2 only; punctuation keys in the `.cin` are not used by the on-screen 速成 keyboard).
2. For each HKSCS-2016 character whose `cangjie` field is non-empty, derive the 速成 key from each official Cangjie code (first + last letters). Codes may be Latin letters, comma-separated alternatives, or radical-form strings such as `口尸卜日`, which are translated with the standard 倉頡字母對部首 map and are not invented. Characters are appended to a key only when that character is not already in that key’s list. Characters with an empty `cangjie` field are skipped (no invented codes).
3. Promote a documented list of common spoken-Cantonese / Hong Kong colloquial characters to the front of whatever lists they already belong to, without dropping anyone. The list is in `src/simplex.rs` (`PROMOTED_CANTONESE`). It includes at least: 嘅 喺 咁 咋 嘸 哋 啲 嘢 擸 佢 唔 咆 噉 咯 呢 嗰 冇 係 睇 啫 啱 嘗 嘔 嘕 啾 咻 哇 喃, plus other clearly common spoken-Cantonese characters already in the table.
4. Candidate order is therefore not claimed to match Microsoft 速成.

The merged table is compiled into the exe by `build.rs` (no loose `.cin` / `.json` is required at runtime).

## Microsoft 速成 / 倉頡 Ctrl+Alt+, symbol keyboard

The 符號 button opens the publicly documented Traditional Chinese IME
symbol keyboard (符號鍵盤 / 輸入法整合器) shared by 微軟倉頡 and 微軟速成.
This is a copy of the on-screen character set and approximate keyboard
order, not a Microsoft IME private dictionary.

- Microsoft Support documents the shortcut: in Chinese mode,
  **Ctrl+Alt+comma** shows the symbol keyboard; press the corresponding
  key (or click) to insert.
  https://support.microsoft.com/zh-tw/windows/hardware/input-devices/microsoft-traditional-chinese-ime
  (English: https://support.microsoft.com/en-us/windows/hardware/input-devices/microsoft-traditional-chinese-ime )

Public key maps of that keyboard (read as a physical keyboard layout):

- https://water0622.pixnet.net/blog/posts/4043845348 (2017, 輸入法整合器)
- https://rakutentw.pixnet.net/blog/posts/11404319206 (2019, Win10; notes that some OS versions show a single-column symbol list instead of the on-screen keypad)
- Corroboration for individual keys: https://www.moonlol.com/windows-punctuation-marks-7684.html and https://www.tsjh.ntpc.edu.tw/p/404-1000-6387.php

Conflicts resolved against those pages:

- **Key 7** is ’ (U+2019 RIGHT SINGLE QUOTATION MARK). The 2017 pixnet table writes ˙ on 7; that looks like a typo. The 2019 layout, moonlol, and the tsjh school page all have ’.
- **Key D** is ┘. The 2017 table appears to skip D (it jumps from S ┴ to F ○). The 2019 layout is the primary map.
- **Period** is ‧ (U+2027 HYPHENATION POINT, 間隔號), as in both pixnet tables, moonlol, and tsjh — not Katakana middle dot ・ (U+30FB).

The 2019 layout, which on99KB follows in full (not only keys 1..R), is:

```
` €
1 ┌  2 ┬  3 ┐  4 〝  5 〞  6 ‘  7 ’  8 “  9 ”  0 『  - 』  = 「
q ├  w ┼  e ┤  r ※  t 〈  y 〉  u 《  i 》  o 【  p 】  [ ﹝  ] ﹞  \ 」
a └  s ┴  d ┘  f ○  g ●  h ↑  j ↓  k ！  l ：  ; ；  ' 、
z ─  x │  c ◎  v §  b ←  n →  m 。  , ，  . ‧  / ？
```

旺財’s requested range 「┌」 through 「※」 is key 1 through key R. on99KB includes at least that span, and also the rest of the same official keyboard (keys after R, plus € on backtick). No extra box-drawing or punctuation (═, ╪, ╳, …, etc.) is added beyond these sources. The physical Ctrl+Alt+, shortcut is not implemented; the on-screen 符號 button is mouse-only and does not steal focus.
