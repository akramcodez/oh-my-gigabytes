//! Palette, gradient, spinner and small formatting helpers. 24-bit colour when the terminal
//! advertises it, 16-colour fallback otherwise, and no colour at all when `NO_COLOR` is set.
use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    TrueColor,
    Basic,
    Mono,
}
#[derive(Clone, Copy)]
pub struct Theme {
    pub mode: Mode,
}

pub const FRAMES: [&str; 6] = ["·", "✢", "✳", "✶", "✻", "✽"];
pub const VERBS: [&str; 5] = [
    "Weighing gigabytes",
    "Sniffing caches",
    "Counting bytes",
    "Measuring folders",
    "Gobbling inodes",
];

impl Theme {
    pub fn detect() -> Self {
        let mode = if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            Mode::Mono
        } else if std::env::var("COLORTERM")
            .is_ok_and(|v| v.contains("truecolor") || v.contains("24bit"))
        {
            Mode::TrueColor
        } else {
            Mode::Basic
        };
        Self { mode }
    }
    fn pick(&self, rgb: (u8, u8, u8), basic: Color) -> Option<Color> {
        match self.mode {
            Mode::TrueColor => Some(Color::Rgb(rgb.0, rgb.1, rgb.2)),
            Mode::Basic => Some(basic),
            Mode::Mono => None,
        }
    }
    fn style(&self, rgb: (u8, u8, u8), basic: Color) -> Style {
        self.pick(rgb, basic)
            .map_or_else(Style::new, |c| Style::new().fg(c))
    }
    pub fn accent(&self) -> Style {
        self.style((255, 79, 216), Color::Magenta)
    }
    pub fn violet(&self) -> Style {
        self.style((155, 92, 255), Color::LightMagenta)
    }
    pub fn cyan(&self) -> Style {
        self.style((61, 224, 255), Color::Cyan)
    }
    pub fn good(&self) -> Style {
        self.style((92, 242, 138), Color::Green)
    }
    pub fn warn(&self) -> Style {
        self.style((255, 181, 71), Color::Yellow)
    }
    pub fn danger(&self) -> Style {
        self.style((255, 107, 87), Color::Red)
    }
    pub fn dim(&self) -> Style {
        match self.mode {
            Mode::Mono => Style::new().add_modifier(Modifier::DIM),
            _ => self.style((122, 127, 140), Color::DarkGray),
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
    /// Magenta → cyan gradient colour for position `i` of `n`.
    pub fn gradient(&self, i: usize, n: usize) -> Style {
        let t = if n <= 1 {
            0.0
        } else {
            i as f32 / (n - 1) as f32
        };
        let (a, b) = ((255.0, 79.0, 216.0), (61.0, 224.0, 255.0));
        let mix = |x: f32, y: f32| (x + (y - x) * t) as u8;
        let rgb = (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2));
        match self.mode {
            Mode::TrueColor => Style::new()
                .fg(Color::Rgb(rgb.0, rgb.1, rgb.2))
                .add_modifier(Modifier::BOLD),
            Mode::Basic => Style::new()
                .fg(if t < 0.5 { Color::Magenta } else { Color::Cyan })
                .add_modifier(Modifier::BOLD),
            Mode::Mono => Style::new().add_modifier(Modifier::BOLD),
        }
    }
    pub fn tier(&self, tier: &str) -> Style {
        match tier {
            "protected" => self.danger(),
            "review" => self.warn(),
            _ => self.cyan(),
        }
    }
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
