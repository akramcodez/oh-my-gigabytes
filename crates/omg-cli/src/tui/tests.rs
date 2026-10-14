use super::*;
use omg_core::rollup::{Child, Totals};

fn draw(state: &AppState, width: u16, height: u16) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| {
            render(
                f,
                state,
                &theme::Theme {
                    mode: theme::Mode::Mono,
                },
            )
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rendering takes only `AppState`: there is no index to query, which keeps the UI fluid.
#[test]
fn every_view_renders_at_standard_sizes_without_an_index() {
    for (width, height) in [(50, 16), (64, 20), (80, 24), (120, 40)] {
        for tab in Tab::ALL {
            let state = AppState {
                tab,
                ..Default::default()
            };
            let text = draw(&state, width, height);
            assert!(text.contains("█▀█"), "banner at {width}x{height}");
            assert!(text.contains("quit"), "{} at {width}x{height}", tab.name());
        }
    }
    assert!(draw(&AppState::default(), 40, 10).contains("too small"));
}

#[test]
fn banner_is_full_when_wide_and_omg_when_narrow() {
    let wide = theme::banner(80);
    assert_eq!(wide[0].chars().count(), wide[1].chars().count());
    assert!(wide[0].chars().count() > 40);
    let narrow = theme::banner(20);
    assert!(narrow[0].chars().count() <= 20);
}

#[test]
fn browse_shows_sizes_escapes_names_and_progress() {
    let hostile = Child {
        path: b"evil\x1b[31mname\n".to_vec(),
        kind: "directory".into(),
        allocated: 5 << 30,
        apparent: 5 << 30,
        files: 1200,
        dirs: 3,
        protected: 1,
        newest: 0,
    };
    let mut state = AppState {
        tab: Tab::Browse,
        scan: Some(ScanView {
            progress: Arc::new(AtomicU64::new(500)),
            total: 1000,
            started: Instant::now(),
            finishing_since: None,
        }),
        ..Default::default()
    };
    state.dirs.insert(
        Vec::new(),
        DirView {
            children: vec![hostile],
            marks: vec![],
            total: Some(Totals {
                allocated: 6 << 30,
                ..Default::default()
            }),
        },
    );
    let text = draw(&state, 110, 30);
    assert!(text.contains("5.00 GiB"));
    assert!(
        !text.contains('\u{1b}'),
        "terminal escapes must not reach the screen"
    );
    assert!(text.contains("50%"));
}

#[test]
fn delete_confirmation_defaults_to_cancel_and_shows_warnings() {
    let menu = Menu {
        title: "Delete permanently?".into(),
        body: vec![
            (
                "1 item(s) · ≈ 2.00 GiB · this cannot be undone".into(),
                Tone::Danger,
            ),
            ("⚠ inside the Git repository at /home/x".into(), Tone::Warn),
        ],
        choices: vec![
            Choice {
                label: "Cancel".into(),
                key: None,
                action: Action::CancelPlan {
                    ids: vec![],
                    fingerprint: String::new(),
                },
                tone: Tone::Normal,
            },
            Choice {
                label: "Yes, delete permanently".into(),
                key: None,
                action: Action::CancelPlan {
                    ids: vec![],
                    fingerprint: String::new(),
                },
                tone: Tone::Danger,
            },
        ],
        cursor: 0,
        tone: Tone::Danger,
    };
    let state = AppState {
        menu: Some(menu),
        ..Default::default()
    };
    let text = draw(&state, 100, 30);
    assert!(
        text.contains("❯ 1. Cancel"),
        "cancel is the default choice:\n{text}"
    );
    assert!(text.contains("2. Yes, delete permanently"));
    assert!(text.contains("cannot be undone") && text.contains("Git repository"));
}

#[test]
fn helpers_format_humanly() {
    assert_eq!(theme::count(1_234_567), "1,234,567");
    assert_eq!(theme::short_count(1_650_000), "1.65M");
    assert_eq!(theme::ago(1000, Some(1000 - 180)), "3m ago");
    assert_eq!(theme::bar(0.5, 4), "██  ");
    assert_eq!(theme::clean(b"a\x1bb"), "a\\u{1b}b");
}

#[test]
fn overview_explains_that_shared_extents_are_not_freed_space() {
    let state = AppState {
        summary: Some(Box::new(summary_with_marked(b"/cache"))),
        ..Default::default()
    };
    let text = draw(&state, 120, 30);
    assert!(text.contains("Shared clone/snapshot space is unknown"));
    assert!(text.contains("not space-freed estimates"));
}

/// `cargo test -p omg-cli preview_screens -- --ignored --nocapture` prints sample screens.
#[test]
#[ignore]
fn preview_screens() {
    let child = |name: &str, gib: f64, files: u64, protected: u64| Child {
        path: name.as_bytes().to_vec(),
        kind: "directory".into(),
        allocated: (gib * (1u64 << 30) as f64) as u64,
        apparent: 0,
        files,
        dirs: 2,
        protected,
        newest: 0,
    };
    let mut state = AppState {
        now: 10_000,
        message: "Welcome to OMG".into(),
        ..Default::default()
    };
    state.dirs.insert(
        Vec::new(),
        DirView {
            children: vec![
                child(".cache", 22.4, 480_000, 0),
                child("projects", 14.1, 90_000, 3),
                child("Downloads", 3.4, 120, 0),
            ],
            marks: vec![],
            total: None,
        },
    );
    for tab in [Tab::Overview, Tab::Browse] {
        state.tab = tab;
        println!("{}\n", draw(&state, 100, 26));
    }
}

#[test]
fn paths_saved_in_omg_trash_are_red_and_underlined() {
    use ratatui::style::Modifier;
    let child = |path: &str| Child {
        path: path.as_bytes().to_vec(),
        kind: "directory".into(),
        allocated: 1 << 30,
        apparent: 0,
        files: 1,
        dirs: 0,
        protected: 0,
        newest: 0,
    };
    let mut state = AppState {
        tab: Tab::Browse,
        ..Default::default()
    };
    state.dirs.insert(
        Vec::new(),
        DirView {
            children: vec![child("saved-dir"), child("plain-dir")],
            marks: vec![],
            total: None,
        },
    );
    state.summary = Some(Box::new(summary_with_marked(b"saved-dir")));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| {
            render(
                f,
                &state,
                &theme::Theme {
                    mode: theme::Mode::TrueColor,
                },
            )
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let styled = |needle: &str| -> (bool, bool) {
        for y in 0..30u16 {
            let row: String = (0..100u16)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect();
            if let Some(at) = row.find(needle) {
                let col = row[..at].chars().count() as u16;
                let cell = &buffer[(col, y)];
                return (
                    cell.fg == ratatui::style::Color::Rgb(255, 107, 87),
                    cell.modifier.contains(Modifier::UNDERLINED),
                );
            }
        }
        panic!("{needle} not drawn");
    };
    assert_eq!(
        styled("saved-dir"),
        (true, true),
        "saved path is red + underlined"
    );
    assert_eq!(
        styled("plain-dir"),
        (false, false),
        "other rows are untouched"
    );
}

fn summary_with_marked(path: &[u8]) -> worker::Summary {
    worker::Summary {
        status: omg_core::Status {
            root: Some(b"/".to_vec()),
            last_successful: Some(1),
            last_report: None,
            entries: 1,
            unique_apparent: 0,
            unique_allocated: 0,
            capacity: None,
            available: None,
            volume_sample_at: None,
            scope_note: String::new(),
        },
        rollups: true,
        findings: vec![],
        top: vec![],
        marked: vec![omg_core::cleanup::Marked {
            id: 1,
            path: path.to_vec(),
            identity: "1:1".into(),
            kind: "directory".into(),
            allocated: 1,
            added: 0,
            warning: None,
            birth: None,
            source: "user".into(),
        }],
        trend_delta: None,
        root: None,
        home: None,
        auto_trash: true,
        volume: None,
    }
}
