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
        incomplete: false,
        excluded: false,
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

/// A folder the scan could not read (permission, macOS privacy protection) must not look empty, and
/// one with an unreadable part must show its size as a lower bound.
#[test]
fn unreadable_folders_show_unknown_not_zero() {
    let folder = |name: &str, allocated: u64, files: u64, incomplete: bool| Child {
        path: name.as_bytes().to_vec(),
        kind: "directory".into(),
        allocated,
        apparent: allocated,
        files,
        dirs: 0,
        protected: 0,
        newest: 0,
        incomplete,
        excluded: false,
    };
    let mut state = AppState {
        tab: Tab::Browse,
        ..Default::default()
    };
    state.dirs.insert(
        Vec::new(),
        DirView {
            children: vec![
                folder("Library", 2 << 30, 900, true),
                // ext4 gives the folder itself 4 KiB; nothing inside was counted, so still unknown.
                folder("Documents", 4096, 0, true),
                folder("Movies", 0, 1, true),
                folder("empty", 0, 0, false),
                Child {
                    excluded: true,
                    ..folder(".ssh", 0, 0, false)
                },
            ],
            marks: vec![],
            total: None,
        },
    );
    let text = draw(&state, 110, 30);
    let line = |name: &str| {
        text.lines()
            .find(|l| l.contains(name))
            .unwrap_or_else(|| panic!("{name} missing:\n{text}"))
            .to_string()
    };
    assert!(line("Documents/").contains("unknown"), "{text}");
    assert!(line("Documents/").contains("unreadable"), "{text}");
    assert!(!line("Documents/").contains("0.00 B"), "{text}");
    assert!(line("Library/").contains("≥ 2.00 GiB"), "{text}");
    assert!(line("Library/").contains("incomplete"), "{text}");
    assert!(line("Movies/").contains("unknown"), "{text}");
    assert!(!line("Movies/").contains("≥"), "{text}");
    assert!(line("empty/").contains("0.00 B"), "{text}");
    assert!(line(".ssh/").contains("not scanned"), "{text}");
    assert!(!line(".ssh/").contains("0.00 B"), "{text}");
}

/// Inside a folder, rows show names and the path bar shows one crumb per folder. On Windows the
/// display form of a path uses backslashes, and both used to be split on `/` after that conversion,
/// so rows read `work\victim.bin` and the bar read `AppData\Local` (found by the ConPTY test).
#[test]
fn nested_folders_show_names_and_one_crumb_per_folder() {
    let file = |path: &str| Child {
        path: path.as_bytes().to_vec(),
        kind: "file".into(),
        allocated: 3 << 20,
        apparent: 3 << 20,
        files: 1,
        dirs: 0,
        protected: 0,
        newest: 0,
        incomplete: false,
        excluded: false,
    };
    let mut state = AppState {
        tab: Tab::Browse,
        dir: b"AppData/Local/uv".to_vec(),
        ..Default::default()
    };
    state.dirs.insert(
        b"AppData/Local/uv".to_vec(),
        DirView {
            children: vec![file("AppData/Local/uv/victim.bin")],
            marks: vec![],
            total: None,
        },
    );
    let text = draw(&state, 110, 30);
    assert!(text.contains("› AppData › Local › uv"), "{text}");
    let row = text
        .lines()
        .find(|l| l.contains("victim.bin"))
        .unwrap_or_else(|| panic!("row missing:\n{text}"));
    assert!(row.contains("  victim.bin"), "{row}");
    assert!(
        !row.contains("uv"),
        "the row shows the name, not the path: {row}"
    );
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
        incomplete: false,
        excluded: false,
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
        incomplete: false,
        excluded: false,
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
            index_bytes: 0,
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

fn test_app() -> (App, std::sync::mpsc::Receiver<Request>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let app = App {
        state: AppState::default(),
        tx,
        cancel: Arc::new(AtomicBool::new(false)),
        job: None,
        state_path: std::path::PathBuf::new(),
    };
    (app, rx)
}

fn running(progress: u64) -> Response {
    Response::Running {
        scan: Some(omg_core::index::RunningScan {
            started: omg_core::now() - 30,
            progress,
            estimated_total: 1000,
            finishing: false,
            phase_at: None,
        }),
        locked: true,
    }
}

/// A held scan lock with no scan behind it is housekeeping (pruning old scans). It used to be
/// reported as "Another omg scan is running" with no progress, and polling stopped.
#[test]
fn a_held_lock_without_a_scan_is_waiting_not_another_scan() {
    let (mut app, _rx) = test_app();
    app.state.watching = true;
    app.state.pending_scan = Some(false);
    app.on_response(Response::Running {
        scan: None,
        locked: true,
    });
    assert!(app.state.waiting.is_some());
    assert!(app.state.other.is_none());
    assert!(app.state.watching, "keeps polling until the lock is free");
    let text = draw(&app.state, 140, 30);
    assert!(text.contains("Waiting for another omg task"), "{text}");
    assert!(text.contains("no scan is running"), "{text}");
    assert!(text.contains("your scan starts after"), "{text}");
    assert!(!text.contains("Another omg scan is running"), "{text}");
}

#[test]
fn waiting_ends_when_the_lock_is_free_and_says_so() {
    let (mut app, _rx) = test_app();
    app.on_response(Response::Running {
        scan: None,
        locked: true,
    });
    app.on_response(Response::Running {
        scan: None,
        locked: false,
    });
    assert!(app.state.waiting.is_none());
    assert!(!app.state.watching);
    assert_eq!(app.state.message, "Housekeeping finished.");
}

#[test]
fn another_real_scan_still_shows_progress_and_ends_cleanly() {
    let (mut app, rx) = test_app();
    app.state.watching = true;
    // The user asked for a scan while another process was scanning: theirs makes ours redundant.
    app.state.pending_scan = Some(true);
    app.on_response(running(500));
    assert!(app.state.waiting.is_none());
    let text = draw(&app.state, 140, 30);
    assert!(text.contains("Another omg scan is running"), "{text}");
    assert!(text.contains("50%"), "{text}");
    app.on_response(Response::Running {
        scan: None,
        locked: false,
    });
    assert!(app.state.other.is_none());
    assert!(
        app.state.pending_scan.is_none(),
        "no second scan is started"
    );
    assert!(app.job.is_none() && app.state.scan.is_none());
    assert_eq!(
        app.state.message,
        "The other scan finished; showing its results."
    );
    // It asks the worker to reload the published results.
    assert!(rx.try_iter().any(|r| matches!(r, Request::Summary)));
}

#[test]
fn escape_cancels_a_scan_that_is_waiting_for_the_lock() {
    let (mut app, _rx) = test_app();
    app.state.pending_scan = Some(false);
    let key = event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.handle_key(key).unwrap());
    assert!(app.state.pending_scan.is_none());
    assert_eq!(app.state.message, "Cancelled the waiting scan.");
}

fn plan_of(
    items: Vec<omg_core::cleanup::Marked>,
    denied: Vec<(Vec<u8>, String)>,
    keepable: Vec<i64>,
) -> DeletePlan {
    DeletePlan {
        fingerprint: "fingerprint".into(),
        items,
        denied,
        estimated: 9 << 30,
        created: 0,
        keepable,
    }
}

fn piece(id: i64, path: &str) -> omg_core::cleanup::Marked {
    omg_core::cleanup::Marked {
        id,
        path: path.as_bytes().to_vec(),
        identity: "1:1".into(),
        kind: "directory".into(),
        allocated: 1 << 20,
        added: 0,
        warning: None,
        birth: Some("1".into()),
        source: "keep-protected".into(),
    }
}

/// Esc at "Are you sure?" used to send Unmark and DiscardPlan separately; the worker's reply to the
/// Unmark ("Removed from OMG Trash.") arrived after, and replaced, "Cancelled. Nothing was deleted."
/// Now one request carries both, and the worker's only reply is the cancel message.
#[cfg(feature = "permanent-delete")]
#[test]
fn escape_at_the_question_cancels_in_one_request_and_keeps_its_message() {
    let (mut app, rx) = test_app();
    app.confirm_delete(
        plan_of(vec![piece(7, "work/victim.bin")], vec![], vec![]),
        vec![7],
    );
    assert_eq!(
        app.state.menu.as_ref().map(|m| m.title.as_str()),
        Some("Are you sure?")
    );
    app.handle_key(event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(app.state.menu.is_none());
    assert_eq!(app.state.message, "Cancelled. Nothing was deleted.");
    let sent: Vec<Request> = rx.try_iter().collect();
    assert_eq!(sent.len(), 1, "one request, so no second reply can follow");
    assert!(matches!(
        &sent[0],
        Request::CancelPlan { fingerprint, undo } if fingerprint == "fingerprint" && undo == &[7]
    ));
}

/// A cache folder with a protected item inside used to look like "1 item" and was then skipped
/// after the user confirmed. Now the dialog says it cannot go whole and offers the alternative.
#[cfg(feature = "permanent-delete")]
#[test]
fn a_folder_that_cannot_go_whole_offers_deleting_around_its_protected_items() {
    let (mut app, rx) = test_app();
    let plan = plan_of(
        vec![],
        vec![(
            b".cache/uv".to_vec(),
            "contains 2 protected item(s), for example a/credentials; it cannot be deleted whole, but everything else in it can".into(),
        )],
        vec![7],
    );
    app.confirm_delete(plan, vec![7]);
    let menu = app.state.menu.as_ref().unwrap();
    assert_eq!(menu.title, "Cannot delete this whole");
    let labels: Vec<&str> = menu.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Cancel", "Delete everything except the protected items…"],
        "no plain 'Yes' for something that cannot go whole"
    );
    assert!(menu.body[0].0.contains("cannot be deleted whole"));
    // Choosing it discards the first plan and asks for the plan around the protected items.
    app.handle_key(event::KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
        .unwrap();
    let sent: Vec<Request> = rx.try_iter().collect();
    if !super::delete_available() {
        // The analysis-only build refuses the choice.
        assert!(sent.is_empty(), "{} request(s) sent", sent.len());
        assert_eq!(
            app.state.message,
            "Permanent delete is not available on this platform."
        );
        return;
    }
    assert!(matches!(&sent[0], Request::DiscardPlan(f) if f == "fingerprint"));
    assert!(matches!(
        &sent[1],
        Request::PrepareDeleteKeeping { ids, undo } if ids == &[7] && undo == &[7]
    ));
}

#[test]
fn the_plan_around_protected_items_says_what_stays_and_asks_again() {
    let (mut app, _rx) = test_app();
    let plan = plan_of(
        vec![
            piece(-1, ".cache/uv/archive-v0/aaa"),
            piece(-2, ".cache/uv/archive-v0/ccc"),
        ],
        vec![
            (
                b".cache/uv/archive-v0/bbb/credentials".to_vec(),
                "kept: protected name (credentials)".into(),
            ),
            (
                b".cache/uv/archive-v0/ddd/credentials".to_vec(),
                "kept: protected name (credentials)".into(),
            ),
        ],
        vec![],
    );
    app.confirm_delete(plan, vec![7]);
    let menu = app.state.menu.as_ref().unwrap();
    assert_eq!(menu.title, "Are you sure?");
    let body: String = menu
        .body
        .iter()
        .map(|(l, _)| l.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(body.contains("2 item(s)"), "{body}");
    assert!(
        body.contains("keeping 2 protected item(s) in place"),
        "{body}"
    );
    assert!(
        !body.contains("skipped"),
        "kept items are not reported as skipped: {body}"
    );
    let labels: Vec<&str> = menu.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Cancel", "Yes, delete permanently"]);
    // Cancelling takes back what the review added to OMG Trash.
    assert!(matches!(
        &menu.choices[0].action,
        Action::CancelPlan { ids, .. } if ids == &[7]
    ));
}

/// Long lines in a dialog wrap. The choices used to sit below the wrapped text inside a box sized
/// for unwrapped lines, so the last ones (including "Yes, delete permanently") were cut off.
#[test]
fn long_dialog_text_never_hides_the_choices() {
    let long = "contains 12 protected item(s), for example home/akram/.cache/uv/archive-v0/GG2Qi2KfBlMub8_qySeOD/docker/credentials; it cannot be deleted whole, but everything else in it can";
    for (width, height) in [(80u16, 24u16), (100, 30), (140, 40), (60, 20)] {
        let state = AppState {
            menu: Some(Menu {
                title: "Cannot delete this whole".into(),
                body: vec![
                    (
                        "this cannot be deleted whole: protected items are inside".into(),
                        Tone::Warn,
                    ),
                    (format!("skipped .cache/uv: {long}"), Tone::Warn),
                    (
                        format!("keeping 12 protected item(s) in place, for example {long}"),
                        Tone::Warn,
                    ),
                ],
                choices: vec![
                    Choice {
                        label: "Cancel".into(),
                        key: None,
                        action: Action::Help,
                        tone: Tone::Normal,
                    },
                    Choice {
                        label: "Yes, delete permanently".into(),
                        key: None,
                        action: Action::Help,
                        tone: Tone::Danger,
                    },
                    Choice {
                        label: "Delete everything except the protected items…".into(),
                        key: None,
                        action: Action::Help,
                        tone: Tone::Danger,
                    },
                ],
                cursor: 0,
                tone: Tone::Danger,
            }),
            ..Default::default()
        };
        let text = draw(&state, width, height);
        for label in [
            "1. Cancel",
            "2. Yes, delete permanently",
            "3. Delete everything except",
        ] {
            assert!(
                text.contains(label),
                "{label} is hidden at {width}x{height}:\n{text}"
            );
        }
    }
}

#[test]
fn the_result_says_protected_items_were_kept_not_that_they_failed() {
    let (mut app, _rx) = test_app();
    app.on_response(Response::Deleted(Box::new(
        omg_core::cleanup::DeleteReport {
            deleted: vec![b"a".to_vec(), b"b".to_vec()],
            skipped: vec![
                (
                    b"c/credentials".to_vec(),
                    "kept: protected name (credentials)".into(),
                ),
                (b"d".to_vec(), "changed since the review".into()),
            ],
            ..Default::default()
        },
    )));
    let message = &app.state.message;
    assert!(message.contains("Deleted 2 item(s)"), "{message}");
    assert!(
        message.contains("kept 1 protected item(s) in place"),
        "{message}"
    );
    assert!(
        message.contains("1 skipped: changed since the review"),
        "{message}"
    );
    assert!(!message.contains("skipped: kept"), "{message}");
}
