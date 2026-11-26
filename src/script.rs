//! 繁／簡 output conversion with embedded OpenCC `hk2s` (香港繁體到簡體).
//!
//! Coding and lookup are always Traditional. When the user picks 簡, only
//! the Chinese characters shown as 速成 / 九方 candidates and committed from
//! those layouts go through [`to_simplified`]. CP437, Latin, digits,
//! ordinary punctuation and the 速成 符號 pad are never converted.
//!
//! The map is precomputed by `build.rs` from the OpenCC dictionaries
//! vendored in `data/opencc/` (Apache-2.0; see `data/SOURCES.md`), applying
//! the `hk2s.json` chain per character: compatibility normalisation →
//! HKVariantsRev → TSCharacters. For one-to-many entries the first listed
//! value (OpenCC's default) is used. Characters with no simplified form,
//! including many HKSCS characters, are returned unchanged. Coverage is that
//! of the vendored dictionaries; it is not claimed to be complete.

use crate::prefs::OutputScript;
use std::sync::OnceLock;

/// Shown on the hint row when 簡 is on and the current / committed pick has
/// no simplified mapping.
pub const NO_MAPPING_HINT: &str = "此字無對應簡體，將輸出原字";

static MAP: OnceLock<Vec<(u32, u32)>> = OnceLock::new();

fn map() -> &'static [(u32, u32)] {
    MAP.get_or_init(|| {
        let bytes: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hk2s.bin"));
        bytes
            .chunks_exact(8)
            .map(|c| {
                (
                    u32::from_le_bytes(c[0..4].try_into().unwrap()),
                    u32::from_le_bytes(c[4..8].try_into().unwrap()),
                )
            })
            .collect()
    })
}

/// Number of characters the embedded `hk2s` map changes.
pub fn mapping_count() -> usize {
    map().len()
}

/// HK Traditional → Simplified for one character (OpenCC `hk2s`).
pub fn to_simplified(ch: char) -> char {
    let m = map();
    match m.binary_search_by_key(&u32::from(ch), |p| p.0) {
        Ok(i) => char::from_u32(m[i].1).unwrap_or(ch),
        Err(_) => ch,
    }
}

/// `true` when `hk2s` gives a different character.
pub fn has_simplified(ch: char) -> bool {
    to_simplified(ch) != ch
}

/// The glyph that will be typed (and shown on the candidate) for a 速成 /
/// 九方 candidate under `output`.
pub fn output_char(output: OutputScript, ch: char) -> char {
    match output {
        OutputScript::Traditional => ch,
        OutputScript::Simplified => to_simplified(ch),
    }
}

/// Whether the hint row should say [`NO_MAPPING_HINT`] for this pick.
pub fn needs_no_mapping_hint(output: OutputScript, pick: Option<char>) -> bool {
    output == OutputScript::Simplified && pick.is_some_and(|c| !has_simplified(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hk2s_converts_common_pairs_and_is_not_plain_t2s() {
        assert_eq!(to_simplified('國'), '国');
        assert_eq!(to_simplified('說'), '说');
        assert_eq!(to_simplified('體'), '体');
        // HK variants go through HKVariantsRev first. Plain t2s
        // (TSCharacters alone) leaves these unchanged; hk2s does not.
        assert_eq!(to_simplified('衞'), '卫', "衞 → 衛 → 卫");
        assert_eq!(to_simplified('枱'), '台', "枱 → 檯 → 台");
        assert_eq!(to_simplified('綫'), '线');
        assert!(mapping_count() > 3000);
    }

    #[test]
    fn unmapped_characters_are_unchanged() {
        // HKSCS / Cantonese characters with no simplified form, and
        // characters that are the same in both scripts.
        for ch in ['嘅', '啲', '嘢', '冇', '咗', '㗎', '日', '月', '上'] {
            assert_eq!(to_simplified(ch), ch, "{ch}");
            assert!(!has_simplified(ch));
        }
        // Non-Chinese text is never touched.
        for ch in ['A', '1', ' ', '☺', '，'] {
            assert_eq!(to_simplified(ch), ch);
        }
    }

    #[test]
    fn candidate_display_matches_emitted_glyph_and_hint() {
        let s = OutputScript::Simplified;
        let t = OutputScript::Traditional;
        assert_eq!(output_char(s, '國'), '国');
        assert_eq!(output_char(t, '國'), '國');
        assert_eq!(output_char(s, '嘅'), '嘅');
        assert!(needs_no_mapping_hint(s, Some('嘅')));
        assert!(!needs_no_mapping_hint(s, Some('國')));
        assert!(!needs_no_mapping_hint(t, Some('嘅')), "繁 never shows it");
        assert!(!needs_no_mapping_hint(s, None));
        assert_eq!(NO_MAPPING_HINT, "此字無對應簡體，將輸出原字");
    }
}
