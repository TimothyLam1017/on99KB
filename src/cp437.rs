//! IBM Code Page 437 (DOS / classic Alt+numpad) glyphs.
//!
//! Index `n` is the character produced by Alt+n on a US IBM PC (codes 1..=255).
//! Code 0 is unused. Code 32 is space (U+0020). Code 255 is NBSP (U+00A0).

/// CP437 glyphs. `CP437[n]` is the Unicode character for Alt+n (`n` in 0..=255).
pub const CP437: [char; 256] = [
    '\0',       // 0
    '☺',        // 1
    '☻',        // 2
    '♥',        // 3
    '♦',        // 4
    '♣',        // 5
    '♠',        // 6
    '•',        // 7
    '◘',        // 8
    '○',        // 9
    '◙',        // 10
    '♂',        // 11
    '♀',        // 12
    '♪',        // 13
    '♫',        // 14
    '☼',        // 15
    '►',        // 16
    '◄',        // 17
    '↕',        // 18
    '‼',        // 19
    '¶',        // 20
    '§',        // 21
    '▬',        // 22
    '↨',        // 23
    '↑',        // 24
    '↓',        // 25
    '→',        // 26
    '←',        // 27
    '∟',        // 28
    '↔',        // 29
    '▲',        // 30
    '▼',        // 31
    ' ',        // 32
    '!',        // 33
    '"',        // 34
    '#',        // 35
    '$',        // 36
    '%',        // 37
    '&',        // 38
    '\'',       // 39
    '(',        // 40
    ')',        // 41
    '*',        // 42
    '+',        // 43
    ',',        // 44
    '-',        // 45
    '.',        // 46
    '/',        // 47
    '0',        // 48
    '1',        // 49
    '2',        // 50
    '3',        // 51
    '4',        // 52
    '5',        // 53
    '6',        // 54
    '7',        // 55
    '8',        // 56
    '9',        // 57
    ':',        // 58
    ';',        // 59
    '<',        // 60
    '=',        // 61
    '>',        // 62
    '?',        // 63
    '@',        // 64
    'A',        // 65
    'B',        // 66
    'C',        // 67
    'D',        // 68
    'E',        // 69
    'F',        // 70
    'G',        // 71
    'H',        // 72
    'I',        // 73
    'J',        // 74
    'K',        // 75
    'L',        // 76
    'M',        // 77
    'N',        // 78
    'O',        // 79
    'P',        // 80
    'Q',        // 81
    'R',        // 82
    'S',        // 83
    'T',        // 84
    'U',        // 85
    'V',        // 86
    'W',        // 87
    'X',        // 88
    'Y',        // 89
    'Z',        // 90
    '[',        // 91
    '\\',       // 92
    ']',        // 93
    '^',        // 94
    '_',        // 95
    '`',        // 96
    'a',        // 97
    'b',        // 98
    'c',        // 99
    'd',        // 100
    'e',        // 101
    'f',        // 102
    'g',        // 103
    'h',        // 104
    'i',        // 105
    'j',        // 106
    'k',        // 107
    'l',        // 108
    'm',        // 109
    'n',        // 110
    'o',        // 111
    'p',        // 112
    'q',        // 113
    'r',        // 114
    's',        // 115
    't',        // 116
    'u',        // 117
    'v',        // 118
    'w',        // 119
    'x',        // 120
    'y',        // 121
    'z',        // 122
    '{',        // 123
    '|',        // 124
    '}',        // 125
    '~',        // 126
    '⌂',        // 127
    'Ç',        // 128
    'ü',        // 129
    'é',        // 130
    'â',        // 131
    'ä',        // 132
    'à',        // 133
    'å',        // 134
    'ç',        // 135
    'ê',        // 136
    'ë',        // 137
    'è',        // 138
    'ï',        // 139
    'î',        // 140
    'ì',        // 141
    'Ä',        // 142
    'Å',        // 143
    'É',        // 144
    'æ',        // 145
    'Æ',        // 146
    'ô',        // 147
    'ö',        // 148
    'ò',        // 149
    'û',        // 150
    'ù',        // 151
    'ÿ',        // 152
    'Ö',        // 153
    'Ü',        // 154
    '¢',        // 155
    '£',        // 156
    '¥',        // 157
    '₧',        // 158
    'ƒ',        // 159
    'á',        // 160
    'í',        // 161
    'ó',        // 162
    'ú',        // 163
    'ñ',        // 164
    'Ñ',        // 165
    'ª',        // 166
    'º',        // 167
    '¿',        // 168
    '⌐',        // 169
    '¬',        // 170
    '½',        // 171
    '¼',        // 172
    '¡',        // 173
    '«',        // 174
    '»',        // 175
    '░',        // 176
    '▒',        // 177
    '▓',        // 178
    '│',        // 179
    '┤',        // 180
    '╡',        // 181
    '╢',        // 182
    '╖',        // 183
    '╕',        // 184
    '╣',        // 185
    '║',        // 186
    '╗',        // 187
    '╝',        // 188
    '╜',        // 189
    '╛',        // 190
    '┐',        // 191
    '└',        // 192
    '┴',        // 193
    '┬',        // 194
    '├',        // 195
    '─',        // 196
    '┼',        // 197
    '╞',        // 198
    '╟',        // 199
    '╚',        // 200
    '╔',        // 201
    '╩',        // 202
    '╦',        // 203
    '╠',        // 204
    '═',        // 205
    '╬',        // 206
    '╧',        // 207
    '╨',        // 208
    '╤',        // 209
    '╥',        // 210
    '╙',        // 211
    '╘',        // 212
    '╒',        // 213
    '╓',        // 214
    '╫',        // 215
    '╪',        // 216
    '┘',        // 217
    '┌',        // 218
    '█',        // 219
    '▄',        // 220
    '▌',        // 221
    '▐',        // 222
    '▀',        // 223
    'α',        // 224
    'ß',        // 225
    'Γ',        // 226
    'π',        // 227
    'Σ',        // 228
    'σ',        // 229
    'µ',        // 230
    'τ',        // 231
    'Φ',        // 232
    'Θ',        // 233
    'Ω',        // 234
    'δ',        // 235
    '∞',        // 236
    'φ',        // 237
    'ε',        // 238
    '∩',        // 239
    '≡',        // 240
    '±',        // 241
    '≥',        // 242
    '≤',        // 243
    '⌠',        // 244
    '⌡',        // 245
    '÷',        // 246
    '≈',        // 247
    '°',        // 248
    '∙',        // 249
    '·',        // 250
    '√',        // 251
    'ⁿ',        // 252
    '²',        // 253
    '■',        // 254
    '\u{00A0}', // 255
];

/// Returns the Unicode character injected for Alt+`code`, if `code` is 1..=255.
pub fn glyph(code: u8) -> Option<char> {
    if code == 0 {
        None
    } else {
        Some(CP437[code as usize])
    }
}

/// True when the key face should use a name rather than the raw glyph.
pub fn is_named_blank(code: u8) -> bool {
    code == 32 || code == 255
}

/// UI label for a key: a named caption for invisible glyphs, otherwise the char.
pub fn display_label(code: u8) -> String {
    match code {
        0 => "on99KB".to_string(),
        32 => "空格".to_string(),
        255 => "不換行空格".to_string(),
        n => glyph(n).map(|c| c.to_string()).unwrap_or_default(),
    }
}

/// Tooltip / status-bar text, e.g. `Alt+3  ♥  U+2665`.
pub fn hint_text(code: u8) -> String {
    match code {
        0 => "on99KB  ·  IBM CP437 螢幕鍵盤  ·  點擊按鍵輸入到先前聚焦嘅視窗".to_string(),
        32 => "Alt+32  空格  U+0020  SPACE".to_string(),
        255 => "Alt+255  不換行空格  U+00A0  NBSP".to_string(),
        n => {
            if let Some(ch) = glyph(n) {
                format!("Alt+{n}  {ch}  U+{:04X}", ch as u32)
            } else {
                String::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact glyphs from the on99KB request (Alt+1 ..= Alt+255).
    /// Code 32 is space; code 255 is NBSP (U+00A0).
    const USER_LIST: &str = concat!(
        "☺☻♥♦♣♠•◘○◙♂♀♪♫☼►◄↕‼¶§▬↨↑↓→←∟↔▲▼",
        " !\"#$%&'()*+,-./0123456789:;<=>?@",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`",
        "abcdefghijklmnopqrstuvwxyz{|}~⌂",
        "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒ",
        "áíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐",
        "└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀",
        "αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■",
        "\u{00A0}",
    );

    #[test]
    fn user_list_has_255_chars() {
        assert_eq!(USER_LIST.chars().count(), 255);
    }

    #[test]
    fn all_255_match_user_list() {
        let expected: Vec<char> = USER_LIST.chars().collect();
        for code in 1u8..=255 {
            let got = glyph(code).expect("glyph");
            let exp = expected[(code as usize) - 1];
            assert_eq!(
                got, exp,
                "Alt+{code} mismatch: got U+{:04X} ({got:?}), expected U+{:04X} ({exp:?})",
                got as u32, exp as u32
            );
        }
    }

    #[test]
    fn space_and_nbsp() {
        assert_eq!(glyph(32), Some(' '));
        assert_eq!(glyph(255), Some('\u{00A0}'));
        assert_eq!(CP437[32], ' ');
        assert_eq!(CP437[255], '\u{00A0}');
        assert!(is_named_blank(32));
        assert!(is_named_blank(255));
        assert_eq!(display_label(32), "空格");
        assert_eq!(display_label(255), "不換行空格");
    }

    #[test]
    fn selected_well_known_codes() {
        assert_eq!(glyph(1), Some('☺'));
        assert_eq!(glyph(3), Some('♥'));
        assert_eq!(glyph(65), Some('A'));
        assert_eq!(glyph(127), Some('⌂'));
        assert_eq!(glyph(155), Some('¢'));
        assert_eq!(glyph(158), Some('₧'));
        assert_eq!(glyph(219), Some('█'));
        assert_eq!(glyph(224), Some('α'));
        assert_eq!(glyph(254), Some('■'));
    }

    #[test]
    fn no_glyph_for_zero() {
        assert_eq!(glyph(0), None);
    }
}
