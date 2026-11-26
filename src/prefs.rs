//! Remembered window size, position, opacity, always-on-top, and keyboard mode.

/// First-launch and minimum CP437 client size, in 96-DPI design pixels
/// (device-independent pixels). The app scales these to the window's DPI, so
/// at 150% the default client is 720×690 physical pixels.
///
/// Measured from Timothy's expected-default-size screenshot (PNG 495×510
/// including a teal desktop margin; the window itself is about 482×492
/// with a 31 px title bar). The client area is exactly 480×460, with a
/// 28 px status bar still fully visible.
pub const DEFAULT_CLIENT_W: i32 = 480;
pub const DEFAULT_CLIENT_H: i32 = 460;
pub const MIN_CLIENT_W: i32 = 480;
pub const MIN_CLIENT_H: i32 = 460;
pub const MAX_CLIENT: i32 = 10_000;

/// Window visibility: 100% is fully opaque (the default), 50% and 25% are
/// more transparent. These are percentages of opacity, not of transparency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opacity {
    P25,
    P50,
    P100,
}

impl Opacity {
    pub const DEFAULT: Self = Self::P100;

    pub fn from_percent(n: i32) -> Option<Self> {
        match n {
            25 => Some(Self::P25),
            50 => Some(Self::P50),
            100 => Some(Self::P100),
            _ => None,
        }
    }

    pub fn percent(self) -> u8 {
        match self {
            Self::P25 => 25,
            Self::P50 => 50,
            Self::P100 => 100,
        }
    }

    /// Alpha value for `SetLayeredWindowAttributes` (`LWA_ALPHA`).
    pub fn alpha(self) -> u8 {
        match self {
            Self::P25 => 64,
            Self::P50 => 128,
            Self::P100 => 255,
        }
    }

    /// Cycle 100% → 50% → 25% → 100%.
    pub fn next(self) -> Self {
        match self {
            Self::P100 => Self::P50,
            Self::P50 => Self::P25,
            Self::P25 => Self::P100,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::P25 => "25%",
            Self::P50 => "50%",
            Self::P100 => "100%",
        }
    }
}

/// Keyboard layout mode. Missing or invalid ini values default to CP437.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardMode {
    Cp437,
    Simplex,
}

impl KeyboardMode {
    pub const DEFAULT: Self = Self::Cp437;

    pub fn as_ini(self) -> &'static str {
        match self {
            Self::Cp437 => "cp437",
            Self::Simplex => "simplex",
        }
    }

    pub fn from_ini(s: &str) -> Option<Self> {
        match s.trim() {
            "cp437" => Some(Self::Cp437),
            "simplex" => Some(Self::Simplex),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowPrefs {
    pub width: i32,
    pub height: i32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub opacity: Opacity,
    /// Always-on-top. Missing or invalid ini values default to `true`.
    pub topmost: bool,
    pub mode: KeyboardMode,
    /// `true` when `width` / `height` are 96-DPI design pixels (`units=dip`).
    /// Files written before DPI scaling stored physical pixels and lack the
    /// key; the app converts those using the current monitor's DPI.
    pub dip: bool,
}

pub fn sanitize(
    width: i32,
    height: i32,
    x: Option<i32>,
    y: Option<i32>,
    opacity: Opacity,
    topmost: bool,
    mode: KeyboardMode,
) -> Option<WindowPrefs> {
    if !(MIN_CLIENT_W..=MAX_CLIENT).contains(&width) {
        return None;
    }
    if !(MIN_CLIENT_H..=MAX_CLIENT).contains(&height) {
        return None;
    }
    Some(WindowPrefs {
        width,
        height,
        x,
        y,
        opacity,
        topmost,
        mode,
        dip: true,
    })
}

/// Parse a small `width=` / `height=` / optional `x=` / `y=` / optional
/// `opacity=` / optional `topmost=` / optional `mode=` ini. Corrupt or
/// out-of-range sizes yield `None` (caller uses the default). A missing or
/// invalid `opacity` falls back to 100%, a missing or invalid `topmost`
/// falls back to on (1), and a missing or invalid `mode` falls back to
/// CP437; none of these discard a valid size.
pub fn parse_window_ini(s: &str) -> Option<WindowPrefs> {
    let mut width = None;
    let mut height = None;
    let mut x = None;
    let mut y = None;
    let mut opacity = Opacity::DEFAULT;
    let mut topmost = true;
    let mut mode = KeyboardMode::DEFAULT;
    let mut dip = false;
    for raw in s.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = v.trim();
        if key == "units" {
            dip = val == "dip";
            continue;
        }
        if key == "mode" {
            if let Some(m) = KeyboardMode::from_ini(val) {
                mode = m;
            }
            continue;
        }
        let Ok(n) = val.parse::<i32>() else {
            continue;
        };
        match key {
            "width" => width = Some(n),
            "height" => height = Some(n),
            "x" => x = Some(n),
            "y" => y = Some(n),
            "opacity" => {
                if let Some(o) = Opacity::from_percent(n) {
                    opacity = o;
                }
            }
            "topmost" => match n {
                1 => topmost = true,
                0 => topmost = false,
                _ => {}
            },
            _ => {}
        }
    }
    let mut prefs = sanitize(width?, height?, x, y, opacity, topmost, mode)?;
    prefs.dip = dip;
    Some(prefs)
}

pub fn format_window_ini(prefs: &WindowPrefs) -> String {
    let mut s = format!(
        "# on99KB window size (client area, 96-DPI pixels), opacity, always-on-top, and mode.\n{}width={}\nheight={}\nopacity={}\ntopmost={}\nmode={}\n",
        if prefs.dip { "units=dip\n" } else { "" },
        prefs.width,
        prefs.height,
        prefs.opacity.percent(),
        if prefs.topmost { 1 } else { 0 },
        prefs.mode.as_ini()
    );
    if let (Some(x), Some(y)) = (prefs.x, prefs.y) {
        s.push_str(&format!("x={x}\ny={y}\n"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_screenshot_client() {
        assert_eq!(DEFAULT_CLIENT_W, 480);
        assert_eq!(DEFAULT_CLIENT_H, 460);
        assert_eq!(MIN_CLIENT_W, DEFAULT_CLIENT_W);
        assert_eq!(MIN_CLIENT_H, DEFAULT_CLIENT_H);
    }

    #[test]
    fn parses_and_round_trips() {
        let p = parse_window_ini("width=640\nheight=512\nx=10\ny=20\n").unwrap();
        assert_eq!(p.width, 640);
        assert_eq!(p.height, 512);
        assert_eq!(p.x, Some(10));
        assert_eq!(p.y, Some(20));
        assert_eq!(p.opacity, Opacity::DEFAULT);
        assert!(p.topmost);
        assert_eq!(p.mode, KeyboardMode::DEFAULT);
        let again = parse_window_ini(&format_window_ini(&p)).unwrap();
        assert_eq!(p, again);
        let formatted = format_window_ini(&p);
        assert!(formatted.contains("opacity=100"));
        assert!(formatted.contains("topmost=1"));
        assert!(formatted.contains("mode=cp437"));
    }

    #[test]
    fn opacity_parses_and_falls_back() {
        let p = parse_window_ini("width=480\nheight=460\nopacity=25\n").unwrap();
        assert_eq!(p.opacity, Opacity::P25);
        assert_eq!(p.opacity.alpha(), 64);
        let p = parse_window_ini("width=480\nheight=460\nopacity=50\n").unwrap();
        assert_eq!(p.opacity, Opacity::P50);
        assert_eq!(p.opacity.alpha(), 128);
        let p = parse_window_ini("width=480\nheight=460\nopacity=100\n").unwrap();
        assert_eq!(p.opacity, Opacity::P100);
        assert_eq!(p.opacity.alpha(), 255);
        // invalid or missing opacity must not drop a valid size
        let p = parse_window_ini("width=480\nheight=460\nopacity=40\n").unwrap();
        assert_eq!(p.opacity, Opacity::P100);
        let p = parse_window_ini("width=480\nheight=460\n").unwrap();
        assert_eq!(p.opacity, Opacity::P100);
        assert_eq!(Opacity::P100.next(), Opacity::P50);
        assert_eq!(Opacity::P50.next(), Opacity::P25);
        assert_eq!(Opacity::P25.next(), Opacity::P100);
    }

    #[test]
    fn topmost_parses_and_falls_back() {
        let p = parse_window_ini("width=480\nheight=460\ntopmost=0\n").unwrap();
        assert!(!p.topmost);
        assert_eq!(p.width, 480);
        let p = parse_window_ini("width=480\nheight=460\ntopmost=1\n").unwrap();
        assert!(p.topmost);
        // invalid or missing topmost must not drop a valid size, and defaults to on
        let p = parse_window_ini("width=480\nheight=460\ntopmost=2\n").unwrap();
        assert!(p.topmost);
        let p = parse_window_ini("width=480\nheight=460\n").unwrap();
        assert!(p.topmost);
        let off = sanitize(
            480,
            460,
            Some(10),
            Some(20),
            Opacity::P50,
            false,
            KeyboardMode::Cp437,
        )
        .unwrap();
        let text = format_window_ini(&off);
        assert!(text.contains("topmost=0"));
        assert!(text.contains("opacity=50"));
        let again = parse_window_ini(&text).unwrap();
        assert_eq!(off, again);
    }

    #[test]
    fn mode_parses_and_falls_back() {
        let p = parse_window_ini("width=480\nheight=460\nmode=simplex\n").unwrap();
        assert_eq!(p.mode, KeyboardMode::Simplex);
        assert_eq!(p.width, 480);
        let p = parse_window_ini("width=480\nheight=460\nmode=cp437\n").unwrap();
        assert_eq!(p.mode, KeyboardMode::Cp437);
        // invalid or missing mode must not drop a valid size, and defaults to CP437
        let p = parse_window_ini("width=480\nheight=460\nmode=fullcangjie\n").unwrap();
        assert_eq!(p.mode, KeyboardMode::Cp437);
        let p = parse_window_ini("width=480\nheight=460\n").unwrap();
        assert_eq!(p.mode, KeyboardMode::Cp437);
        let stored = sanitize(
            480,
            460,
            Some(10),
            Some(20),
            Opacity::P100,
            true,
            KeyboardMode::Simplex,
        )
        .unwrap();
        let text = format_window_ini(&stored);
        assert!(text.contains("mode=simplex"));
        let again = parse_window_ini(&text).unwrap();
        assert_eq!(stored, again);
    }

    #[test]
    fn units_marker_round_trips_and_legacy_is_physical() {
        // Files from before DPI scaling have no units key: physical pixels.
        let legacy = parse_window_ini("width=720\nheight=690\n").unwrap();
        assert!(!legacy.dip);
        let p = parse_window_ini("units=dip\nwidth=480\nheight=460\n").unwrap();
        assert!(p.dip);
        // Newly sanitized prefs are always design pixels and say so.
        let fresh = sanitize(
            480,
            460,
            None,
            None,
            Opacity::P100,
            true,
            KeyboardMode::Cp437,
        )
        .unwrap();
        assert!(fresh.dip);
        let text = format_window_ini(&fresh);
        assert!(text.contains("units=dip"));
        assert_eq!(parse_window_ini(&text).unwrap(), fresh);
    }

    #[test]
    fn ignores_corrupt_and_out_of_range() {
        assert!(parse_window_ini("").is_none());
        assert!(parse_window_ini("width=abc\nheight=460\n").is_none());
        assert!(parse_window_ini("width=100\nheight=460\n").is_none());
        assert!(parse_window_ini("width=480\nheight=100\n").is_none());
        assert!(parse_window_ini("width=99999\nheight=460\n").is_none());
        assert!(parse_window_ini("height=460\n").is_none());
        // unknown keys and comments are fine
        let p = parse_window_ini("# c\nwidth=480\nfoo=1\nheight=460\n").unwrap();
        assert_eq!(p.width, 480);
        assert_eq!(p.height, 460);
        assert_eq!(p.x, None);
        assert_eq!(p.opacity, Opacity::P100);
        assert!(p.topmost);
        assert_eq!(p.mode, KeyboardMode::Cp437);
    }
}
