//! Generate the embedded 速成 table from vendored sources, and (when
//! targeting Windows GNU) compile the icon resource with windres.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-changed=data/simplex-ext.cin");
    println!("cargo:rerun-if-changed=data/HKSCS2016.json");
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    generate_table(&manifest);

    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let abi = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && abi == "gnu" {
        compile_icon_resource(&manifest);
    }
}

fn compile_icon_resource(manifest: &Path) {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app_icon.res");
    let status = Command::new("x86_64-w64-mingw32-windres")
        .current_dir(manifest)
        .args(["res/app.rc", "-O", "coff", "-o"])
        .arg(&out)
        .status()
        .expect("failed to spawn x86_64-w64-mingw32-windres");
    if !status.success() {
        panic!("windres failed with {status}");
    }
    println!("cargo:rustc-link-arg={}", out.display());
}

// Same list as src/simplex.rs::PROMOTED_CANTONESE — keep in sync.
const PROMOTED: &[char] = &[
    '嘅', '喺', '咁', '咋', '嘸', '哋', '啲', '嘢', '擸', '佢', '唔', '咆', '噉', '咯', '呢', '嗰',
    '冇', '係', '睇', '啫', '啱', '嘗', '嘔', '嘕', '啾', '咻', '哇', '喃', '喎', '嘛', '咩', '嚟',
    '咗', '㗎', '啩', '噃', '吖', '唓', '唞', '啋', '嘥', '嚿', '冚', '揾', '掟', '嬲', '乸', '撳',
    '孭', '冧', '餸', '黐', '嚡', '噏', '喐', '嗱', '咹', '攰', '嗻', '唥', '揦', '搲', '吓', '㩒',
    '嘞',
];

const RADICAL_TO_LETTER: &[(char, char)] = &[
    ('日', 'a'),
    ('月', 'b'),
    ('金', 'c'),
    ('木', 'd'),
    ('水', 'e'),
    ('火', 'f'),
    ('土', 'g'),
    ('竹', 'h'),
    ('戈', 'i'),
    ('十', 'j'),
    ('大', 'k'),
    ('中', 'l'),
    ('一', 'm'),
    ('弓', 'n'),
    ('人', 'o'),
    ('心', 'p'),
    ('手', 'q'),
    ('口', 'r'),
    ('尸', 's'),
    ('廿', 't'),
    ('山', 'u'),
    ('女', 'v'),
    ('田', 'w'),
    ('難', 'x'),
    ('卜', 'y'),
    ('重', 'z'),
];

fn radical_letter(ch: char) -> Option<char> {
    RADICAL_TO_LETTER
        .iter()
        .find(|(r, _)| *r == ch)
        .map(|(_, l)| *l)
}

fn simplex_key_from_code(code: &str) -> Option<String> {
    let code = code.trim();
    if code.is_empty() {
        return None;
    }
    let letters: Option<String> = if code.bytes().all(|b| b.is_ascii_alphabetic()) {
        Some(code.to_ascii_lowercase())
    } else {
        let mut s = String::new();
        for ch in code.chars() {
            s.push(radical_letter(ch)?);
        }
        Some(s)
    };
    let s = letters?;
    if s.is_empty() {
        return None;
    }
    let b = s.as_bytes();
    if b.len() == 1 {
        Some(s)
    } else {
        Some(format!("{}{}", b[0] as char, b[b.len() - 1] as char))
    }
}

fn cangjie_keys(field: &str) -> Vec<String> {
    let field = field.trim();
    if field.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for part in field.split(|c| c == ',' || c == '，') {
        if let Some(k) = simplex_key_from_code(part) {
            if !out.contains(&k) {
                out.push(k);
            }
        }
    }
    out
}

fn pack_key(key: &str) -> Option<u16> {
    let b = key.as_bytes();
    match b {
        [a] if a.is_ascii_lowercase() => Some(u16::from(*a) << 8),
        [a, c] if a.is_ascii_lowercase() && c.is_ascii_lowercase() => {
            Some((u16::from(*a) << 8) | u16::from(*c))
        }
        _ => None,
    }
}

fn parse_cin(text: &str) -> BTreeMap<String, Vec<char>> {
    let mut table: BTreeMap<String, Vec<char>> = BTreeMap::new();
    let mut in_def = false;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with("%chardef") && s.contains("begin") {
            in_def = true;
            continue;
        }
        if s.starts_with("%chardef") && s.contains("end") {
            in_def = false;
            continue;
        }
        if !in_def || s.is_empty() || s.starts_with('#') {
            continue;
        }
        let mut parts = s.split_whitespace();
        let Some(key) = parts.next() else { continue };
        let Some(ch) = parts.next().and_then(|t| {
            let mut c = t.chars();
            let first = c.next()?;
            if c.next().is_some() {
                None
            } else {
                Some(first)
            }
        }) else {
            continue;
        };
        if pack_key(key).is_none() {
            continue;
        }
        let list = table.entry(key.to_string()).or_default();
        if !list.contains(&ch) {
            list.push(ch);
        }
    }
    table
}

fn extract_json_string(obj: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":");
    let i = obj.find(&pat)?;
    let rest = obj[i + pat.len()..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = rest[1..].chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(cp) {
                            out.push(ch);
                        }
                    }
                }
                Some(other) => out.push(other),
                None => break,
            }
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

fn parse_hkscs(text: &str) -> Vec<(char, String)> {
    let text = text.trim_start_matches('\u{feff}');
    let mut out = Vec::new();
    for obj in text.split('{').skip(1) {
        let Some(ch_s) = extract_json_string(obj, "char") else {
            continue;
        };
        let mut it = ch_s.chars();
        let Some(ch) = it.next() else { continue };
        if it.next().is_some() {
            continue;
        }
        let cj = extract_json_string(obj, "cangjie").unwrap_or_default();
        out.push((ch, cj));
    }
    out
}

fn promote(table: &mut BTreeMap<String, Vec<char>>) {
    let promoted: Vec<char> = PROMOTED.to_vec();
    for list in table.values_mut() {
        let mut head = Vec::new();
        for &p in &promoted {
            if list.contains(&p) && !head.contains(&p) {
                head.push(p);
            }
        }
        if head.is_empty() {
            continue;
        }
        let rest: Vec<char> = list.iter().copied().filter(|c| !head.contains(c)).collect();
        list.clear();
        list.extend(head);
        list.extend(rest);
    }
}

fn generate_table(manifest: &Path) {
    let cin =
        fs::read_to_string(manifest.join("data/simplex-ext.cin")).expect("data/simplex-ext.cin");
    let mut table = parse_cin(&cin);

    let hkscs_bytes = fs::read(manifest.join("data/HKSCS2016.json")).expect("HKSCS2016.json");
    let hkscs_text = String::from_utf8_lossy(&hkscs_bytes);
    let hkscs = parse_hkscs(&hkscs_text);
    let hkscs_total = hkscs.len() as u32;

    let mut with_cj = 0u32;
    for (ch, cj) in &hkscs {
        if cj.trim().is_empty() {
            continue;
        }
        with_cj += 1;
        for k in cangjie_keys(cj) {
            let list = table.entry(k).or_default();
            if !list.contains(ch) {
                list.push(*ch);
            }
        }
    }

    promote(&mut table);

    let mut covered = 0u32;
    for (ch, cj) in &hkscs {
        if cj.trim().is_empty() {
            continue;
        }
        let keys = cangjie_keys(cj);
        if keys
            .iter()
            .any(|k| table.get(k).is_some_and(|l| l.contains(ch)))
        {
            covered += 1;
        }
    }

    let mut keys: Vec<(u16, Vec<char>)> = table
        .into_iter()
        .filter_map(|(k, list)| pack_key(&k).map(|p| (p, list)))
        .collect();
    keys.sort_by_key(|(k, _)| *k);

    let nkeys = keys.len() as u32;
    let nchars: u32 = keys.iter().map(|(_, l)| l.len() as u32).sum();

    let mut bin = Vec::new();
    bin.extend_from_slice(b"O99S");
    bin.extend_from_slice(&1u32.to_le_bytes());
    bin.extend_from_slice(&hkscs_total.to_le_bytes());
    bin.extend_from_slice(&with_cj.to_le_bytes());
    bin.extend_from_slice(&covered.to_le_bytes());
    bin.extend_from_slice(&nkeys.to_le_bytes());
    bin.extend_from_slice(&nchars.to_le_bytes());
    bin.extend_from_slice(&0u32.to_le_bytes()); // reserved, keeps header 32 bytes

    let mut start = 0u32;
    for (key, list) in &keys {
        bin.extend_from_slice(&key.to_le_bytes());
        bin.extend_from_slice(&0u16.to_le_bytes());
        bin.extend_from_slice(&start.to_le_bytes());
        bin.extend_from_slice(&(list.len() as u32).to_le_bytes());
        start += list.len() as u32;
    }
    for (_, list) in &keys {
        for ch in list {
            bin.extend_from_slice(&u32::from(*ch).to_le_bytes());
        }
    }

    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("simplex.bin");
    fs::write(&out, &bin).expect("write simplex.bin");
    // Also drop a short text summary next to the bin for inspection.
    let summary = format!(
        "hkscs_total={hkscs_total}\nhkscs_with_cangjie={with_cj}\nhkscs_covered={covered}\nkeys={nkeys}\nchars={nchars}\nbytes={}\n",
        bin.len()
    );
    fs::write(out.with_extension("txt"), summary).ok();
}
