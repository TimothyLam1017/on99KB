//! Remembered window size and opacity. Mode is never persisted.

/// First-launch and minimum CP437 client size, in pixels.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowPrefs {
    pub width: i32,
    pub height: i32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub opacity: Opacity,
}

pub fn sanitize(
    width: i32,
    height: i32,
    x: Option<i32>,
    y: Option<i32>,
    opacity: Opacity,
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
    })
}

/// Parse a small `width=` / `height=` / optional `x=` / `y=` / optional
/// `opacity=` ini. Corrupt or out-of-range sizes yield `None` (caller uses
/// the default). A missing or invalid `opacity` falls back to 100% and does
/// not discard a valid size.
pub fn parse_window_ini(s: &str) -> Option<WindowPrefs> {
    let mut width = None;
    let mut height = None;
    let mut x = None;
    let mut y = None;
    let mut opacity = Opacity::DEFAULT;
    for raw in s.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let Ok(n) = v.trim().parse::<i32>() else {
            continue;
        };
        match k.trim() {
            "width" => width = Some(n),
            "height" => height = Some(n),
            "x" => x = Some(n),
            "y" => y = Some(n),
            "opacity" => {
                if let Some(o) = Opacity::from_percent(n) {
                    opacity = o;
                }
            }
            _ => {}
        }
    }
    sanitize(width?, height?, x, y, opacity)
}

pub fn format_window_ini(prefs: &WindowPrefs) -> String {
    let mut s = format!(
        "# on99KB window size (client area, pixels) and opacity. Mode is not stored.\nwidth={}\nheight={}\nopacity={}\n",
        prefs.width,
        prefs.height,
        prefs.opacity.percent()
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
        let again = parse_window_ini(&format_window_ini(&p)).unwrap();
        assert_eq!(p, again);
        assert!(format_window_ini(&p).contains("opacity=100"));
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
    }
}
