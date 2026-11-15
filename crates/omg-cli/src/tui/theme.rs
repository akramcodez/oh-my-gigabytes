//! Palette, gradient, spinner and small formatting helpers. 24-bit colour when the terminal
//! advertises it, 16-colour fallback otherwise, and no colour at all when `NO_COLOR` is set.
use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    TrueColor,
    Basic,
    Mono,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Palette {
    Dark,
    Light,
}
#[derive(Clone, Copy)]
pub struct Theme {
    pub mode: Mode,
    pub palette: Palette,
    /// Paint the palette's own background, so a chosen theme reads the same in any terminal.
    pub paint: bool,
}

pub const FRAMES: [&str; 6] = ["·", "✢", "✳", "✶", "✻", "✽"];
pub const VERBS: [&str; 5] = [
    "Weighing gigabytes",
    "Sniffing caches",
    "Counting bytes",
    "Measuring folders",
    "Gobbling inodes",
];

struct Colors {
    brand: ((u8, u8, u8), Color),
    good: ((u8, u8, u8), Color),
    warn: ((u8, u8, u8), Color),
    danger: ((u8, u8, u8), Color),
    dim: ((u8, u8, u8), Color),
    ink: (u8, u8, u8),
    paper: (u8, u8, u8),
}

const DARK: Colors = Colors {
    brand: ((139, 124, 255), Color::LightMagenta),
    good: ((92, 242, 138), Color::Green),
    warn: ((255, 181, 71), Color::Yellow),
    danger: ((255, 107, 87), Color::Red),
    dim: ((122, 127, 140), Color::DarkGray),
    ink: (230, 229, 238),
    paper: (11, 11, 14),
};

const LIGHT: Colors = Colors {
    brand: ((194, 65, 28), Color::Red),
    good: ((31, 122, 61), Color::Green),
    warn: ((150, 88, 0), Color::Yellow),
    danger: ((179, 38, 30), Color::Magenta),
    dim: ((122, 118, 108), Color::DarkGray),
    ink: (27, 26, 23),
    paper: (244, 242, 236),
};

impl Theme {
    pub fn detect_mode() -> Mode {
        let var = |k: &str| std::env::var(k).unwrap_or_default();
        let (term, program, colorterm) = (var("TERM"), var("TERM_PROGRAM"), var("COLORTERM"));
        if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            Mode::Mono
        } else if colorterm.contains("truecolor")
            || colorterm.contains("24bit")
            // Windows Terminal (also inside WSL), VS Code, iTerm2, WezTerm, Ghostty, kitty...
            || std::env::var_os("WT_SESSION").is_some()
            || ["vscode", "iTerm.app", "WezTerm", "ghostty"].contains(&program.as_str())
            || ["kitty", "alacritty", "direct", "truecolor"].iter().any(|t| term.contains(t))
        {
            Mode::TrueColor
        } else {
            Mode::Basic
        }
    }
    /// The palette `auto` uses. It asks the terminal, so call it before the interface starts.
    pub fn auto_palette() -> Palette {
        background().unwrap_or(Palette::Dark)
    }
    /// `choice` is the saved setting (`auto`, `dark` or `light`); `OMG_THEME` overrides it.
    pub fn resolve(mode: Mode, choice: &str, auto: Palette) -> Self {
        let env = std::env::var("OMG_THEME").unwrap_or_default();
        let choice = if omg_core::index::THEMES.contains(&env.as_str()) {
            env.as_str()
        } else {
            choice
        };
        let (palette, paint) = match choice {
            "dark" => (Palette::Dark, true),
            "light" => (Palette::Light, true),
            _ => (auto, false),
        };
        Self {
            mode,
            palette,
            paint: paint && mode == Mode::TrueColor,
        }
    }
    fn colors(&self) -> &'static Colors {
        match self.palette {
            Palette::Dark => &DARK,
            Palette::Light => &LIGHT,
        }
    }
    fn pick(&self, rgb: (u8, u8, u8), basic: Color) -> Option<Color> {
        match self.mode {
            Mode::TrueColor => Some(Color::Rgb(rgb.0, rgb.1, rgb.2)),
            Mode::Basic => Some(basic),
            Mode::Mono => None,
        }
    }
    fn style(&self, (rgb, basic): ((u8, u8, u8), Color)) -> Style {
        self.pick(rgb, basic)
            .map_or_else(Style::new, |c| Style::new().fg(c))
    }
    /// The whole-screen style when the theme paints its own background.
    pub fn base(&self) -> Option<Style> {
        let c = self.colors();
        self.paint.then(|| {
            Style::new()
                .fg(Color::Rgb(c.ink.0, c.ink.1, c.ink.2))
                .bg(Color::Rgb(c.paper.0, c.paper.1, c.paper.2))
        })
    }
    /// The one signature colour (violet in dark, orange in light) for brand, selection, bars and keys.
    pub fn brand(&self) -> Style {
        self.style(self.colors().brand)
    }
    pub fn accent(&self) -> Style {
        self.brand()
    }
    pub fn good(&self) -> Style {
        self.style(self.colors().good)
    }
    pub fn warn(&self) -> Style {
        self.style(self.colors().warn)
    }
    pub fn danger(&self) -> Style {
        self.style(self.colors().danger)
    }
    pub fn dim(&self) -> Style {
        match self.mode {
            Mode::Mono => Style::new().add_modifier(Modifier::DIM),
            _ => self.style(self.colors().dim),
        }
    }
    pub fn bold(&self) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }
    pub fn selected(&self) -> Style {
        match self.mode {
            Mode::Mono => Style::new().add_modifier(Modifier::REVERSED),
            _ => self.accent().add_modifier(Modifier::BOLD),
        }
    }
    pub fn tier(&self, tier: &str) -> Style {
        match tier {
            "protected" => self.danger(),
            "review" => self.warn(),
            _ => self.brand(),
        }
    }
}

/// The terminal's background, from `COLORFGBG` or, on Unix, by asking the terminal itself.
fn background() -> Option<Palette> {
    if let Ok(value) = std::env::var("COLORFGBG")
        && let Some(bg) = value.rsplit(';').next().and_then(|b| b.parse::<u8>().ok())
    {
        return Some(if matches!(bg, 7 | 15) {
            Palette::Light
        } else {
            Palette::Dark
        });
    }
    #[cfg(unix)]
    {
        query_background()
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Sends OSC 11 (background colour) followed by DA1, which every terminal answers, and reads
/// until the DA1 reply, so a terminal that ignores OSC 11 leaves nothing behind as keystrokes.
#[cfg(unix)]
fn query_background() -> Option<Palette> {
    use rustix::fs::{Mode as FileMode, OFlags, open};
    use std::io::Write;
    use std::time::{Duration, Instant};
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return None;
    }
    let tty = open(
        "/dev/tty",
        OFlags::RDWR | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC,
        FileMode::empty(),
    )
    .ok()?;
    crossterm::terminal::enable_raw_mode().ok()?;
    let mut file = std::fs::File::from(tty);
    let mut reply = Vec::new();
    if file.write_all(b"\x1b]11;?\x1b\\\x1b[c").is_ok() {
        let deadline = Instant::now() + Duration::from_millis(400);
        let mut buf = [0u8; 256];
        while Instant::now() < deadline && reply.len() < 1024 && !da1_done(&reply) {
            match rustix::io::read(&file, &mut buf) {
                Ok(0) => break,
                Ok(n) => reply.extend_from_slice(&buf[..n]),
                Err(rustix::io::Errno::AGAIN) => std::thread::sleep(Duration::from_millis(5)),
                Err(_) => break,
            }
        }
    }
    let _ = crossterm::terminal::disable_raw_mode();
    parse_background(&reply)
}

#[cfg(unix)]
fn da1_done(reply: &[u8]) -> bool {
    reply
        .windows(3)
        .position(|w| w == b"\x1b[?")
        .is_some_and(|at| reply[at..].contains(&b'c'))
}

/// Reads `rgb:RRRR/GGGG/BBBB` (1 to 4 hex digits each) from an OSC 11 reply.
pub fn parse_background(reply: &[u8]) -> Option<Palette> {
    let text = std::str::from_utf8(reply).ok()?;
    let rest = &text[text.find("rgb:")? + 4..];
    let mut channels = rest.split('/').take(3).map(|part| {
        let hex: String = part.chars().take_while(char::is_ascii_hexdigit).collect();
        let max = 16f64.powi(hex.len() as i32) - 1.0;
        u32::from_str_radix(&hex, 16)
            .ok()
            .filter(|_| !hex.is_empty())
            .map(|v| v as f64 / max)
    });
    let (r, g, b) = (channels.next()??, channels.next()??, channels.next()??);
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    Some(if luminance > 0.5 {
        Palette::Light
    } else {
        Palette::Dark
    })
}

/// Horizontal bar of `width` cells for `fraction` (0..=1) with eighth-block precision.
pub fn bar(fraction: f64, width: usize) -> String {
    const PARTS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let cells = (fraction.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    let (full, part) = (cells / 8, cells % 8);
    let mut out = "█".repeat(full.min(width));
    if full < width && part > 0 {
        out.push(PARTS[part]);
    }
    let used = out.chars().count();
    out.push_str(&" ".repeat(width.saturating_sub(used)));
    out
}

pub fn ago(now: i64, then: Option<i64>) -> String {
    let Some(then) = then else {
        return "never".into();
    };
    let d = (now - then).max(0);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86399 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86400),
    }
}

/// Calendar date (UTC) of a Unix time, as YYYY-MM-DD.
pub fn date(at: i64) -> String {
    let days = at.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn short_count(n: u64) -> String {
    match n {
        0..=9_999 => count(n),
        10_000..=999_999 => format!("{:.0}K", n as f64 / 1e3),
        _ => format!("{:.2}M", n as f64 / 1e6),
    }
}

pub fn duration(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {}s", s / 60, s % 60)
    } else {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    }
}

/// Raw path bytes → printable text. Control characters and invalid UTF-8 are escaped.
pub fn clean(raw: &[u8]) -> String {
    let shown = omg_core::display_path(raw);
    shown
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(&shown)
        .to_string()
}

pub fn truncate_middle(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width || width < 8 {
        return text.chars().take(width).collect();
    }
    let keep = width - 1;
    let (head, tail) = (keep / 3, keep - keep / 3);
    format!(
        "{}…{}",
        chars[..head].iter().collect::<String>(),
        chars[chars.len() - tail..].iter().collect::<String>()
    )
}

/// Two-row block-letter wordmark. Full "OH MY GIGABYTES" when it fits, otherwise "OMG".
pub fn banner(width: usize) -> [String; 2] {
    fn glyph(c: char) -> [&'static str; 2] {
        match c {
            'O' => ["█▀█", "█▄█"],
            'H' => ["█ █", "█▀█"],
            'M' => ["█▀▄▀█", "█ ▀ █"],
            'Y' => ["█▄█", " █ "],
            'G' => ["█▀▀", "█▄█"],
            'I' => ["█", "█"],
            'A' => ["▄▀█", "█▀█"],
            'B' => ["█▄▄", "█▄█"],
            'T' => ["▀█▀", " █ "],
            'E' => ["█▀▀", "██▄"],
            'S' => ["█▀", "▄█"],
            _ => ["  ", "  "],
        }
    }
    let render = |words: &[&str]| -> [String; 2] {
        let mut rows = [String::new(), String::new()];
        for (w, word) in words.iter().enumerate() {
            if w > 0 {
                rows[0].push_str("   ");
                rows[1].push_str("   ");
            }
            for (i, c) in word.chars().enumerate() {
                if i > 0 {
                    rows[0].push(' ');
                    rows[1].push(' ');
                }
                let [a, b] = glyph(c);
                rows[0].push_str(a);
                rows[1].push_str(b);
            }
        }
        rows
    };
    let full = render(&["OH", "MY", "GIGABYTES"]);
    if full[0].chars().count() <= width {
        full
    } else {
        render(&["OMG"])
    }
}
