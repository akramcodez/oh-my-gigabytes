//! Docker cleanup against a fake engine on a Unix socket: only the planned, still-unused items are
//! removed, through the fixed requests, without force, and nothing at all when Docker changed.
#![cfg(all(unix, feature = "permanent-delete"))]
use omg_core::docker::{Endpoint, Engine, Kind};
use std::{
    io::{Read, Write},
    os::unix::net::UnixListener,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Fake {
    df: serde_json::Value,
    requests: Vec<String>,
    /// After this many disk-usage reports, the engine reports `later` instead.
    change_after: Option<(usize, serde_json::Value)>,
    reports: usize,
}

fn serve(state: Arc<Mutex<Fake>>) -> (tempfile::TempDir, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("docker.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            let line = request.lines().next().unwrap_or("").to_string();
            let mut fake = state.lock().unwrap();
            fake.requests.push(line.clone());
            let (status, body) = if line.starts_with("GET /_ping") {
                (200, "OK".to_string())
            } else if line.starts_with("GET /system/df") {
                fake.reports += 1;
                if let Some((after, later)) = fake.change_after.clone()
                    && fake.reports > after
                {
                    fake.df = later;
                }
                (200, fake.df.to_string())
            } else if line.starts_with("DELETE /images/") {
                // A real engine removes the image; reflect that in later reports.
                let id = line
                    .split('/')
                    .nth(2)
                    .unwrap()
                    .split('?')
                    .next()
                    .unwrap()
                    .to_string();
                if let Some(images) = fake.df["Images"].as_array_mut() {
                    images.retain(|i| i["Id"] != id.as_str());
                }
                (200, "[]".to_string())
            } else if line.starts_with("POST /build/prune") {
                let caches = fake.df["BuildCache"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let deleted: Vec<String> = caches
                    .iter()
                    .filter(|c| line.contains(c["ID"].as_str().unwrap()))
                    .map(|c| c["ID"].as_str().unwrap().to_string())
                    .collect();
                (
                    200,
                    serde_json::json!({ "CachesDeleted": deleted, "SpaceReclaimed": 300 })
                        .to_string(),
                )
            } else {
                (404, "{\"message\":\"no\"}".to_string())
            };
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    let engine = Engine::at(Endpoint::Unix(path));
    (dir, engine)
}

fn sample() -> serde_json::Value {
    serde_json::json!({
        "Images": [
            { "Id": format!("sha256:{}", "1".repeat(64)), "RepoTags": ["<none>:<none>"], "Size": 500, "SharedSize": 0, "Containers": 0, "Created": 1_700_000_000 },
            { "Id": format!("sha256:{}", "2".repeat(64)), "RepoTags": ["node:22"], "Size": 900, "SharedSize": 0, "Containers": 0, "Created": 1_700_000_000 },
            { "Id": format!("sha256:{}", "3".repeat(64)), "RepoTags": ["postgres:16"], "Size": 800, "SharedSize": 0, "Containers": 1 }
        ],
        "Containers": [
            { "Id": "a".repeat(64), "Names": ["/db"], "Image": "postgres:16", "State": "running", "SizeRw": 10 },
            { "Id": "b".repeat(64), "Names": ["/old"], "Image": "node:22", "State": "exited", "SizeRw": 30, "Created": 1_700_000_000 }
        ],
        "Volumes": [{ "Name": "pgdata", "UsageData": { "Size": 4000, "RefCount": 1 } }],
        "BuildCache": [
            { "ID": "abc123", "Type": "regular", "Description": "RUN npm ci", "InUse": false, "Size": 300, "LastUsedAt": "2023-11-14T22:13:20Z" },
            { "ID": "def456", "Type": "regular", "Description": "", "InUse": true, "Size": 999 }
        ]
    })
}

#[test]
fn the_default_plan_removes_only_dangling_images_and_unused_cache_without_force() {
    let state = Arc::new(Mutex::new(Fake {
        df: sample(),
        ..Default::default()
    }));
    let (_dir, engine) = serve(state.clone());
    let plan = engine.plan(&Kind::DEFAULT).unwrap();
    assert_eq!(
        plan.items.iter().map(|i| i.kind).collect::<Vec<_>>(),
        vec![Kind::DanglingImage, Kind::BuildCache]
    );
    let outcome = engine.remove(&plan, &plan.fingerprint).unwrap();
    assert_eq!(outcome.removed.len(), 2, "{outcome:?}");
    assert!(outcome.skipped.is_empty(), "{outcome:?}");
    let requests = state.lock().unwrap().requests.clone();
    let changes: Vec<&String> = requests.iter().filter(|r| !r.starts_with("GET")).collect();
    assert_eq!(changes.len(), 2, "{requests:?}");
    assert!(changes[0].starts_with(&format!(
        "DELETE /images/sha256:{}?force=false&noprune=false",
        "1".repeat(64)
    )));
    assert!(
        changes[1].starts_with("POST /build/prune?filters=")
            && changes[1].contains("abc123")
            && !changes[1].contains("def456")
    );
    // The in-use image, the tagged image, the running container, the stopped container and the
    // volume were never touched.
    for untouched in [
        "3333",
        "2222",
        "aaaa",
        "bbbb",
        "volumes",
        "force=true",
        "v=true",
    ] {
        assert!(
            changes.iter().all(|r| !r.contains(untouched)),
            "{untouched}: {changes:?}"
        );
    }
}

#[test]
fn nothing_is_removed_when_docker_changed_after_review_or_the_confirmation_is_wrong() {
    let mut later = sample();
    later["Images"][0]["Containers"] = serde_json::json!(1);
    let state = Arc::new(Mutex::new(Fake {
        df: sample(),
        change_after: Some((1, later)),
        ..Default::default()
    }));
    let (_dir, engine) = serve(state.clone());
    let plan = engine.plan(&Kind::DEFAULT).unwrap();
    assert!(
        engine
            .remove(&plan, "not-the-fingerprint")
            .unwrap_err()
            .to_string()
            .contains("does not match")
    );
    let error = engine
        .remove(&plan, &plan.fingerprint)
        .unwrap_err()
        .to_string();
    assert!(error.contains("changed since you reviewed"), "{error}");
    let requests = state.lock().unwrap().requests.clone();
    assert!(
        requests.iter().all(|r| r.starts_with("GET")),
        "{requests:?}"
    );
}

#[test]
fn an_item_that_becomes_used_during_removal_is_skipped() {
    // Reports: 1 for the plan, 1 for the whole-plan check, then 1 before each item.
    let mut later = sample();
    later["BuildCache"][0]["InUse"] = serde_json::json!(true);
    let state = Arc::new(Mutex::new(Fake {
        df: sample(),
        change_after: Some((3, later)),
        ..Default::default()
    }));
    let (_dir, engine) = serve(state.clone());
    let plan = engine.plan(&Kind::DEFAULT).unwrap();
    let outcome = engine.remove(&plan, &plan.fingerprint).unwrap();
    assert_eq!(outcome.removed.len(), 1, "{outcome:?}");
    assert_eq!(outcome.skipped.len(), 1, "{outcome:?}");
    assert!(
        state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|r| !r.starts_with("POST /build/prune"))
    );
}

#[test]
fn opt_in_kinds_include_unused_images_and_stopped_containers_but_never_running_ones() {
    let state = Arc::new(Mutex::new(Fake {
        df: sample(),
        ..Default::default()
    }));
    let (_dir, engine) = serve(state);
    let plan = engine.plan(&Kind::ALL).unwrap();
    let names: Vec<&str> = plan.items.iter().map(|i| i.name.as_str()).collect();
    assert!(
        names.contains(&"node:22") && names.iter().any(|n| n.starts_with("old ")),
        "{names:?}"
    );
    assert!(
        names
            .iter()
            .all(|n| !n.contains("postgres") && !n.starts_with("db ")),
        "{names:?}"
    );
    let report = engine.report().unwrap();
    assert_eq!((report.volumes, report.running_containers), (1, 1));
}
