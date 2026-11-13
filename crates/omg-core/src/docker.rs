//! Unused Docker data, found and removed only through the Docker Engine API on this machine.
//!
//! omg never touches Docker's files or its virtual machine disk: one disk holds every image,
//! container and volume. It asks the engine what is unused and, after the user confirms an exact
//! plan, asks the engine to remove those items one by one, without force. Volumes, running
//! containers and anything in use are never removed. Only a local Unix socket or Windows named pipe
//! is used; remote hosts are refused.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

/// Largest response accepted from the engine.
const MAX_RESPONSE: usize = 32 << 20;
/// A disk-usage report can take a while on a large engine.
const TIMEOUT: Duration = Duration::from_secs(90);
/// A reviewed plan older than this must be reviewed again.
pub const PLAN_TTL: i64 = 900;
/// Build cache, tagged images and stopped containers this recent may still be in use.
pub const UNUSED_DAYS: i64 = 30;
/// A dangling image this recent may come from a build that is still going on.
pub const DANGLING_DAYS: i64 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An image with no tag that no container uses: left behind by rebuilds and pulls.
    DanglingImage,
    /// Build cache no build is using. Docker rebuilds it when needed.
    BuildCache,
    /// A tagged image no container uses. It can be pulled or built again.
    UnusedImage,
    /// A container that is not running. Its own changes, outside volumes, go with it.
    StoppedContainer,
}

impl Kind {
    pub const ALL: [Kind; 4] = [
        Kind::DanglingImage,
        Kind::BuildCache,
        Kind::UnusedImage,
        Kind::StoppedContainer,
    ];
    /// What a plan includes unless the user asks for more: data Docker recreates on its own.
    pub const DEFAULT: [Kind; 2] = [Kind::DanglingImage, Kind::BuildCache];
    pub fn label(self) -> &'static str {
        match self {
            Kind::DanglingImage => "dangling image",
            Kind::BuildCache => "build cache",
            Kind::UnusedImage => "unused image",
            Kind::StoppedContainer => "stopped container",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub kind: Kind,
    pub id: String,
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    /// Where the engine was reached, for messages: a socket path or pipe name.
    pub endpoint: String,
    pub items: Vec<Item>,
    /// Volumes are reported, never removed.
    pub volumes: u64,
    pub volume_bytes: u64,
    pub running_containers: u64,
}

impl Report {
    pub fn total(&self, kinds: &[Kind]) -> u64 {
        self.items
            .iter()
            .filter(|i| kinds.contains(&i.kind))
            .map(|i| i.bytes)
            .sum()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub endpoint: String,
    pub kinds: Vec<Kind>,
    pub items: Vec<Item>,
    pub bytes: u64,
    pub created: i64,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Outcome {
    pub removed: Vec<Item>,
    pub skipped: Vec<(Item, String)>,
    /// What the engine reported freeing, where it reports it.
    pub reclaimed: u64,
}

/// How to reach the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Unix(PathBuf),
    Pipe(String),
}

impl Endpoint {
    fn label(&self) -> String {
        match self {
            Endpoint::Unix(path) => path.display().to_string(),
            Endpoint::Pipe(name) => name.clone(),
        }
    }
}

/// Where Docker-compatible engines listen for the current user, in the order they are tried.
/// `DOCKER_HOST` comes first; a remote or unknown scheme there is refused rather than skipped.
pub fn candidates(home: &Path) -> Result<Vec<Endpoint>> {
    let host = std::env::var_os("DOCKER_HOST").map(|h| h.to_string_lossy().into_owned());
    endpoints(
        host.as_deref(),
        home,
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
    )
}

fn endpoints(host: Option<&str>, home: &Path, runtime: Option<PathBuf>) -> Result<Vec<Endpoint>> {
    let mut out = Vec::new();
    if let Some(host) = host.filter(|h| !h.is_empty()) {
        if let Some(path) = host.strip_prefix("unix://") {
            out.push(Endpoint::Unix(PathBuf::from(path)));
        } else if let Some(pipe) = host.strip_prefix("npipe://") {
            out.push(Endpoint::Pipe(pipe.replace('/', "\\")));
        } else {
            bail!(
                "DOCKER_HOST points to a remote or unsupported engine ({host}); omg only talks to Docker on this machine"
            );
        }
        return Ok(out);
    }
    if cfg!(windows) {
        out.push(Endpoint::Pipe(r"\\.\pipe\docker_engine".into()));
        return Ok(out);
    }
    let mut paths = vec![
        home.join(".docker/run/docker.sock"),
        home.join(".colima/default/docker.sock"),
        home.join(".colima/docker.sock"),
        home.join(".orbstack/run/docker.sock"),
        home.join(".rd/docker.sock"),
        PathBuf::from("/var/run/docker.sock"),
        PathBuf::from("/run/docker.sock"),
    ];
    if let Some(runtime) = runtime {
        paths.push(runtime.join("docker.sock"));
        paths.push(runtime.join("podman/podman.sock"));
    }
    out.extend(paths.into_iter().map(Endpoint::Unix));
    Ok(out)
}

/// A connection target that answered.
pub struct Engine {
    endpoint: Endpoint,
}

impl Engine {
    /// The first engine that is reachable and answers a ping, or `None` when none is running.
    pub fn find(home: &Path) -> Result<Option<Engine>> {
        let mut refused = None;
        for endpoint in candidates(home)? {
            if !usable(&endpoint) {
                continue;
            }
            let engine = Engine { endpoint };
            match engine.call("GET", "/_ping") {
                Ok((200, body)) if body.trim_ascii() == b"OK" => return Ok(Some(engine)),
                Ok(_) => {}
                Err(error) => refused = Some(error),
            }
        }
        match refused {
            Some(error) if error.to_string().contains("ermission") => Err(error
                .context("Docker is running but this user may not use it (on Linux, add yourself to the docker group)")),
            _ => Ok(None),
        }
    }
    pub fn at(endpoint: Endpoint) -> Engine {
        Engine { endpoint }
    }
    pub fn endpoint(&self) -> String {
        self.endpoint.label()
    }

    /// Everything unused, read from the engine's own disk-usage report.
    pub fn report(&self) -> Result<Report> {
        let (status, body) = self.call("GET", "/system/df")?;
        if status != 200 {
            bail!("Docker's disk usage request failed ({status})");
        }
        let df: SystemDf = serde_json::from_slice(&body)
            .context("Docker's disk usage report could not be read")?;
        Ok(classify(&self.endpoint.label(), df))
    }

    /// The items of `kinds` as they are now, with a fingerprint the user confirms.
    pub fn plan(&self, kinds: &[Kind]) -> Result<Plan> {
        Ok(make_plan(&self.report()?, kinds))
    }

    /// Remove the planned items, only if Docker still describes exactly what was reviewed and each
    /// item is still unused when its turn comes. Nothing is forced, and volumes are never removed.
    #[cfg(feature = "permanent-delete")]
    pub fn remove(&self, plan: &Plan, confirmed: &str) -> Result<Outcome> {
        if confirmed != plan.fingerprint {
            bail!("the confirmation does not match the reviewed plan");
        }
        if crate::now() - plan.created > PLAN_TTL || crate::now() < plan.created {
            bail!("this plan is too old; review it again");
        }
        let now = make_plan(&self.report()?, &plan.kinds);
        let still: Vec<&Item> = now
            .items
            .iter()
            .filter(|i| plan.items.contains(i))
            .collect();
        if still.len() != plan.items.len() || now.items.len() != plan.items.len() {
            bail!("Docker changed since you reviewed this; review it again");
        }
        let mut outcome = Outcome::default();
        let mut caches = Vec::new();
        for item in &plan.items {
            // Each item is checked again right before its own removal.
            let fresh = self.report()?;
            if !fresh.items.contains(item) {
                outcome
                    .skipped
                    .push((item.clone(), "no longer unused".into()));
                continue;
            }
            match item.kind {
                Kind::BuildCache => caches.push(item.clone()),
                Kind::DanglingImage | Kind::UnusedImage => {
                    let (status, body) = self.call(
                        "DELETE",
                        &format!("/images/{}?force=false&noprune=false", item.id),
                    )?;
                    record(&mut outcome, item, status, &body);
                }
                Kind::StoppedContainer => {
                    let (status, body) = self.call(
                        "DELETE",
                        &format!("/containers/{}?v=false&force=false&link=false", item.id),
                    )?;
                    record(&mut outcome, item, status, &body);
                }
            }
        }
        if !caches.is_empty() {
            let ids: Vec<&str> = caches.iter().map(|c| c.id.as_str()).collect();
            let filters = serde_json::json!({ "id": ids, "inuse": ["false"] }).to_string();
            let (status, body) = self.call(
                "POST",
                &format!("/build/prune?filters={}", encode(&filters)),
            )?;
            if status == 200 {
                let pruned: BuildPrune = serde_json::from_slice(&body).unwrap_or_default();
                outcome.reclaimed += pruned.space_reclaimed.max(0) as u64;
                for cache in caches {
                    if pruned.caches_deleted.iter().any(|d| d == &cache.id) {
                        outcome.removed.push(cache);
                    } else {
                        outcome.skipped.push((cache, "Docker kept it".into()));
                    }
                }
            } else {
                for cache in caches {
                    outcome
                        .skipped
                        .push((cache, format!("Docker refused ({status})")));
                }
            }
        }
        Ok(outcome)
    }

    fn call(&self, method: &str, path: &str) -> Result<(u16, Vec<u8>)> {
        check_request(method, path)?;
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: docker\r\nUser-Agent: omg/{}\r\nAccept: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            env!("CARGO_PKG_VERSION")
        );
        let raw = transport(&self.endpoint, request.as_bytes())?;
        parse_response(&raw)
    }
}

#[cfg(feature = "permanent-delete")]
fn record(outcome: &mut Outcome, item: &Item, status: u16, body: &[u8]) {
    if (200..300).contains(&status) {
        outcome.reclaimed += item.bytes;
        outcome.removed.push(item.clone());
    } else {
        let message: ApiError = serde_json::from_slice(body).unwrap_or_default();
        let reason = if status == 409 {
            "in use or shared, so Docker kept it".to_string()
        } else {
            format!("Docker refused ({status})")
        };
        outcome.skipped.push((
            item.clone(),
            if message.message.is_empty() {
                reason
            } else {
                format!("{reason}: {}", clip(&message.message))
            },
        ));
    }
}

fn clip(text: &str) -> String {
    text.chars().take(160).collect()
}

/// The only requests omg ever sends, with ids validated, so no other engine call can be built.
fn check_request(method: &str, path: &str) -> Result<()> {
    let ok = match method {
        "GET" => path == "/_ping" || path == "/system/df",
        "DELETE" => {
            if let Some(rest) = path.strip_prefix("/images/") {
                rest.strip_suffix("?force=false&noprune=false")
                    .is_some_and(image_id)
            } else if let Some(rest) = path.strip_prefix("/containers/") {
                rest.strip_suffix("?v=false&force=false&link=false")
                    .is_some_and(container_id)
            } else {
                false
            }
        }
        "POST" => path.starts_with("/build/prune?filters=") && !path.contains("all"),
        _ => false,
    };
    if !ok {
        bail!("refused an unexpected Docker request");
    }
    Ok(())
}

fn image_id(id: &str) -> bool {
    id.strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn container_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn cache_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[cfg(feature = "permanent-delete")]
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Only a socket owned by this user or by root counts, so another account cannot pose as Docker.
#[cfg(unix)]
fn usable(endpoint: &Endpoint) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let Endpoint::Unix(path) = endpoint else {
        return false;
    };
    std::fs::symlink_metadata(path).is_ok_and(|m| {
        m.file_type().is_socket()
            && (m.uid() == 0 || m.uid() == rustix::process::geteuid().as_raw())
    })
}
#[cfg(windows)]
fn usable(endpoint: &Endpoint) -> bool {
    matches!(endpoint, Endpoint::Pipe(name) if name.starts_with(r"\\.\pipe\"))
}
#[cfg(not(any(unix, windows)))]
fn usable(_: &Endpoint) -> bool {
    false
}

#[cfg(unix)]
fn transport(endpoint: &Endpoint, request: &[u8]) -> Result<Vec<u8>> {
    let Endpoint::Unix(path) = endpoint else {
        bail!("not a local socket")
    };
    let mut stream = std::os::unix::net::UnixStream::connect(path)
        .with_context(|| format!("could not reach Docker at {}", path.display()))?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.write_all(request)?;
    read_response(&mut stream)
}
#[cfg(windows)]
fn transport(endpoint: &Endpoint, request: &[u8]) -> Result<Vec<u8>> {
    let Endpoint::Pipe(name) = endpoint else {
        bail!("not a local pipe")
    };
    let (name, request) = (name.clone(), request.to_vec());
    // A named pipe has no read timeout, so the call runs on its own thread with a deadline.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<Vec<u8>> {
            let mut pipe = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&name)
                .with_context(|| format!("could not reach Docker at {name}"))?;
            pipe.write_all(&request)?;
            read_response(&mut pipe)
        })();
        let _ = tx.send(result);
    });
    rx.recv_timeout(TIMEOUT)
        .map_err(|_| anyhow::anyhow!("Docker did not answer in time"))?
}
#[cfg(not(any(unix, windows)))]
fn transport(_: &Endpoint, _: &[u8]) -> Result<Vec<u8>> {
    bail!("Docker is not supported on this platform")
}

/// Read one HTTP response, stopping at its declared end rather than at end of stream, because a
/// named pipe may stay open.
fn read_response(stream: &mut impl Read) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        if raw.len() > MAX_RESPONSE {
            bail!("Docker's response is too large");
        }
        if complete(&raw) {
            return Ok(raw);
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(raw);
        }
        raw.extend_from_slice(&chunk[..n]);
    }
}

fn split_head(raw: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    Some((&raw[..at], &raw[at + 4..]))
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn complete(raw: &[u8]) -> bool {
    let Some((head, body)) = split_head(raw) else {
        return false;
    };
    let head = String::from_utf8_lossy(head);
    if let Some(length) = header(&head, "content-length").and_then(|v| v.parse::<usize>().ok()) {
        return body.len() >= length;
    }
    if header(&head, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        return dechunk(body).is_ok_and(|(_, done)| done);
    }
    false
}

/// Decode a chunked body. Returns the data and whether the final chunk was seen.
fn dechunk(mut body: &[u8]) -> Result<(Vec<u8>, bool)> {
    let mut out = Vec::new();
    loop {
        let Some(end) = body.windows(2).position(|w| w == b"\r\n") else {
            return Ok((out, false));
        };
        let size_text = std::str::from_utf8(&body[..end])?
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let size = usize::from_str_radix(size_text, 16).context("bad chunk size")?;
        if size > MAX_RESPONSE {
            bail!("Docker's response is too large");
        }
        if size == 0 {
            return Ok((out, true));
        }
        let start = end + 2;
        if body.len() < start + size + 2 {
            return Ok((out, false));
        }
        out.extend_from_slice(&body[start..start + size]);
        body = &body[start + size + 2..];
    }
}

fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>)> {
    let (head, body) = split_head(raw).context("Docker sent an incomplete response")?;
    let head = String::from_utf8_lossy(head);
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .context("Docker sent an unreadable response")?;
    let body =
        if header(&head, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
            dechunk(body)?.0
        } else if let Some(length) =
            header(&head, "content-length").and_then(|v| v.parse::<usize>().ok())
        {
            body[..length.min(body.len())].to_vec()
        } else {
            body.to_vec()
        };
    Ok((status, body))
}

// The engine's own report. Docker adds fields over time, so unknown ones are ignored here; the
// response size is bounded above and every id is validated before it is used.
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct SystemDf {
    images: Option<Vec<DfImage>>,
    containers: Option<Vec<DfContainer>>,
    volumes: Option<Vec<DfVolume>>,
    build_cache: Option<Vec<DfCache>>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct DfImage {
    id: String,
    repo_tags: Option<Vec<String>>,
    size: i64,
    shared_size: i64,
    containers: i64,
    created: i64,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct DfContainer {
    id: String,
    names: Option<Vec<String>>,
    image: String,
    state: String,
    size_rw: i64,
    created: i64,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct DfVolume {
    usage_data: Option<DfUsage>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct DfUsage {
    size: i64,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct DfCache {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "Description")]
    description: String,
    #[serde(rename = "InUse")]
    in_use: bool,
    #[serde(rename = "Size")]
    size: i64,
    #[serde(rename = "LastUsedAt")]
    last_used: Option<String>,
    #[serde(rename = "CreatedAt")]
    created: Option<String>,
}

/// Seconds since the epoch of an RFC 3339 time such as `2026-10-01T12:34:56.789Z` or with an offset.
fn rfc3339(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        std::str::from_utf8(b.get(r)?).ok()?.parse().ok()
    };
    if b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || !matches!(b.get(10), Some(b'T' | b't' | b' '))
    {
        return None;
    }
    let (y, m, d, hh, mm, ss) = (
        num(0..4)?,
        num(5..7)?,
        num(8..10)?,
        num(11..13)?,
        num(14..16)?,
        num(17..19)?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        rest = frac.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest.as_bytes().first() {
        Some(b'Z' | b'z') => 0,
        Some(sign @ (b'+' | b'-')) if rest.len() >= 6 => {
            let o = rest[1..3].parse::<i64>().ok()? * 3600 + rest[4..6].parse::<i64>().ok()? * 60;
            if *sign == b'+' { o } else { -o }
        }
        _ => return None,
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss - offset)
}

/// Whether a time (seconds) is at least `days` old; an unknown time counts as recent.
fn older_than(when: Option<i64>, days: i64, now: i64) -> bool {
    when.is_some_and(|w| w > 0 && now - w >= days * 86_400)
}
#[cfg(feature = "permanent-delete")]
#[derive(Deserialize, Default)]
#[serde(default)]
struct BuildPrune {
    #[serde(rename = "CachesDeleted")]
    caches_deleted: Vec<String>,
    #[serde(rename = "SpaceReclaimed")]
    space_reclaimed: i64,
}
#[cfg(feature = "permanent-delete")]
#[derive(Deserialize, Default)]
#[serde(default)]
struct ApiError {
    message: String,
}

fn classify(endpoint: &str, df: SystemDf) -> Report {
    classify_at(endpoint, df, crate::now())
}

fn classify_at(endpoint: &str, df: SystemDf, now: i64) -> Report {
    let mut items = Vec::new();
    for image in df.images.unwrap_or_default() {
        // An unknown container count (-1) is not "unused".
        if !image_id(&image.id) || image.containers != 0 {
            continue;
        }
        let tags: Vec<String> = image
            .repo_tags
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t != "<none>:<none>")
            .collect();
        let bytes = (image.size - image.shared_size.max(0)).max(0) as u64;
        let short = &image.id[7..19];
        let age = if tags.is_empty() {
            DANGLING_DAYS
        } else {
            UNUSED_DAYS
        };
        if !older_than(Some(image.created), age, now) {
            continue;
        }
        items.push(match tags.first() {
            None => Item {
                kind: Kind::DanglingImage,
                id: image.id.clone(),
                name: format!("<none> {short}"),
                bytes,
            },
            Some(tag) => Item {
                kind: Kind::UnusedImage,
                id: image.id.clone(),
                name: tag.clone(),
                bytes,
            },
        });
    }
    let mut running = 0;
    for container in df.containers.unwrap_or_default() {
        if !container_id(&container.id) {
            continue;
        }
        match container.state.as_str() {
            "exited" | "created" | "dead" => {}
            _ => {
                running += 1;
                continue;
            }
        }
        if !older_than(Some(container.created), UNUSED_DAYS, now) {
            continue;
        }
        let name = container
            .names
            .unwrap_or_default()
            .first()
            .map(|n| n.trim_start_matches('/').to_string())
            .unwrap_or_default();
        items.push(Item {
            kind: Kind::StoppedContainer,
            id: container.id.clone(),
            name: format!("{name} ({})", container.image),
            bytes: container.size_rw.max(0) as u64,
        });
    }
    for cache in df.build_cache.unwrap_or_default() {
        let last = cache
            .last_used
            .as_deref()
            .or(cache.created.as_deref())
            .and_then(rfc3339);
        if cache.in_use || !cache_id(&cache.id) || !older_than(last, UNUSED_DAYS, now) {
            continue;
        }
        let what = if cache.description.is_empty() {
            cache.kind.clone()
        } else {
            clip(&cache.description)
        };
        items.push(Item {
            kind: Kind::BuildCache,
            id: cache.id.clone(),
            name: what,
            bytes: cache.size.max(0) as u64,
        });
    }
    items.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then(b.bytes.cmp(&a.bytes))
            .then(a.id.cmp(&b.id))
    });
    let volumes = df.volumes.unwrap_or_default();
    Report {
        endpoint: endpoint.to_string(),
        volume_bytes: volumes
            .iter()
            .filter_map(|v| v.usage_data.as_ref())
            .map(|u| u.size.max(0) as u64)
            .sum(),
        volumes: volumes.len() as u64,
        running_containers: running,
        items,
    }
}

pub fn make_plan(report: &Report, kinds: &[Kind]) -> Plan {
    let mut kinds: Vec<Kind> = kinds.to_vec();
    kinds.sort();
    kinds.dedup();
    let items: Vec<Item> = report
        .items
        .iter()
        .filter(|i| kinds.contains(&i.kind))
        .cloned()
        .collect();
    let mut hash = Sha256::new();
    hash.update(b"omg-docker-plan-v1\0");
    hash.update(report.endpoint.as_bytes());
    for item in &items {
        hash.update(format!("\0{:?}\0{}\0{}", item.kind, item.id, item.bytes).as_bytes());
    }
    Plan {
        endpoint: report.endpoint.clone(),
        bytes: items.iter().map(|i| i.bytes).sum(),
        kinds,
        items,
        created: crate::now(),
        fingerprint: hash
            .finalize()
            .iter()
            .take(12)
            .map(|b| format!("{b:02x}"))
            .collect(),
    }
}

impl crate::Index {
    /// Record a Docker removal before it starts, so an interrupted one is visible afterwards.
    pub fn docker_intent(&self, plan: &Plan) -> Result<i64> {
        self.db.execute(
            "INSERT INTO operations(created,kind,fingerprint,outcome,detail) VALUES(?,'docker',?,'intent',?)",
            rusqlite::params![crate::now(), plan.fingerprint, serde_json::to_string(plan)?],
        )?;
        Ok(self.db.last_insert_rowid())
    }
    pub fn docker_outcome(&self, operation: i64, outcome: &Outcome) -> Result<()> {
        let state = if outcome.skipped.is_empty() {
            "removed"
        } else if outcome.removed.is_empty() {
            "skipped"
        } else {
            "partial"
        };
        self.db.execute(
            "UPDATE operations SET outcome=?,detail=? WHERE id=? AND kind='docker'",
            rusqlite::params![state, serde_json::to_string(outcome)?, operation],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_few_fixed_requests_can_be_sent() {
        let image = format!("sha256:{}", "a".repeat(64));
        let container = "b".repeat(64);
        assert!(check_request("GET", "/system/df").is_ok());
        assert!(
            check_request(
                "DELETE",
                &format!("/images/{image}?force=false&noprune=false")
            )
            .is_ok()
        );
        assert!(
            check_request(
                "DELETE",
                &format!("/containers/{container}?v=false&force=false&link=false")
            )
            .is_ok()
        );
        for (method, path) in [
            (
                "DELETE",
                format!("/images/{image}?force=true&noprune=false"),
            ),
            (
                "DELETE",
                format!("/containers/{container}?v=true&force=false&link=false"),
            ),
            ("DELETE", "/volumes/data".to_string()),
            ("POST", "/containers/prune".to_string()),
            ("POST", "/images/prune".to_string()),
            ("POST", "/build/prune?all=true".to_string()),
            (
                "DELETE",
                "/images/ubuntu?force=false&noprune=false".to_string(),
            ),
            ("GET", "/containers/json".to_string()),
        ] {
            assert!(check_request(method, &path).is_err(), "{method} {path}");
        }
    }

    #[test]
    fn chunked_and_sized_responses_are_read_to_their_end() {
        let sized = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOKextra";
        assert!(complete(sized));
        assert_eq!(parse_response(sized).unwrap(), (200, b"OK".to_vec()));
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        assert!(complete(chunked));
        assert_eq!(parse_response(chunked).unwrap(), (200, b"abcde".to_vec()));
        assert!(!complete(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nab"
        ));
    }

    #[test]
    fn a_remote_docker_host_is_refused_and_a_local_one_is_the_only_target() {
        let home = Path::new("/home/me");
        for remote in [
            "tcp://10.0.0.5:2375",
            "ssh://me@server",
            "http://localhost:2375",
            "fd://",
        ] {
            assert!(endpoints(Some(remote), home, None).is_err(), "{remote}");
        }
        assert_eq!(
            endpoints(Some("unix:///run/user/1000/docker.sock"), home, None).unwrap(),
            vec![Endpoint::Unix(PathBuf::from("/run/user/1000/docker.sock"))]
        );
        assert_eq!(
            endpoints(Some("npipe:////./pipe/docker_engine"), home, None).unwrap(),
            vec![Endpoint::Pipe(r"\\.\pipe\docker_engine".into())]
        );
        assert!(!endpoints(None, home, None).unwrap().is_empty());
    }

    const DAY: i64 = 86_400;
    // 2026-11-12T00:00:00Z
    const NOW: i64 = 1_794_441_600;
    const OLD: i64 = NOW - 60 * DAY;

    #[test]
    fn dates_from_docker_are_read_exactly() {
        assert_eq!(rfc3339("2026-11-12T00:00:00Z"), Some(NOW));
        assert_eq!(rfc3339("2026-11-12T05:30:00.5+05:30"), Some(NOW));
        assert_eq!(rfc3339("1970-01-01T00:00:00Z"), Some(0));
        for bad in [
            "",
            "yesterday",
            "2026-13-01T00:00:00Z",
            "2026-11-12",
            "2026-11-12T00:00:00",
        ] {
            assert_eq!(rfc3339(bad), None, "{bad}");
        }
        assert!(!older_than(None, 1, NOW));
        assert!(older_than(Some(NOW - 30 * DAY), 30, NOW));
        assert!(!older_than(Some(NOW - 29 * DAY), 30, NOW));
    }

    #[test]
    fn classification_keeps_used_running_and_unknown_things_out() {
        let df: SystemDf = serde_json::from_value(serde_json::json!({
            "Images": [
                { "Id": format!("sha256:{}", "1".repeat(64)), "RepoTags": ["<none>:<none>"], "Size": 500, "SharedSize": 100, "Containers": 0, "Created": OLD },
                { "Id": format!("sha256:{}", "2".repeat(64)), "RepoTags": ["node:22"], "Size": 900, "SharedSize": 0, "Containers": 0, "Created": OLD },
                { "Id": format!("sha256:{}", "5".repeat(64)), "RepoTags": ["redis:7"], "Size": 600, "SharedSize": 0, "Containers": 0, "Created": NOW - 3 * DAY },
                { "Id": format!("sha256:{}", "6".repeat(64)), "RepoTags": null, "Size": 40, "SharedSize": 0, "Containers": 0, "Created": NOW - 2 * DAY },
                { "Id": format!("sha256:{}", "3".repeat(64)), "RepoTags": ["postgres:16"], "Size": 800, "SharedSize": 0, "Containers": 1 },
                { "Id": format!("sha256:{}", "4".repeat(64)), "RepoTags": null, "Size": 70, "SharedSize": -1, "Containers": -1 },
                { "Id": "not-an-id", "RepoTags": null, "Size": 5, "Containers": 0 }
            ],
            "Containers": [
                { "Id": "a".repeat(64), "Names": ["/db"], "Image": "postgres:16", "State": "running", "SizeRw": 10 },
                { "Id": "b".repeat(64), "Names": ["/old"], "Image": "node:22", "State": "exited", "SizeRw": 30, "Created": OLD },
                { "Id": "c".repeat(64), "Names": ["/fresh"], "Image": "node:22", "State": "exited", "SizeRw": 20, "Created": NOW - DAY }
            ],
            "Volumes": [{ "Name": "pgdata", "UsageData": { "Size": 4000, "RefCount": 1 } }],
            "BuildCache": [
                { "ID": "abc123", "Type": "regular", "Description": "RUN npm ci", "InUse": false, "Size": 300, "LastUsedAt": "2026-08-01T10:00:00.123456Z" },
                { "ID": "def456", "Type": "regular", "Description": "", "InUse": true, "Size": 999, "LastUsedAt": "2026-08-01T10:00:00Z" },
                { "ID": "ghi789", "Type": "regular", "Description": "recent", "InUse": false, "Size": 77, "LastUsedAt": "2026-11-10T10:00:00+05:30" },
                { "ID": "jkl012", "Type": "regular", "Description": "no date", "InUse": false, "Size": 55 }
            ],
            "LayersSize": 1
        }))
        .unwrap();
        let report = classify_at("/tmp/docker.sock", df, NOW);
        let kinds: Vec<(Kind, u64)> = report.items.iter().map(|i| (i.kind, i.bytes)).collect();
        assert_eq!(
            kinds,
            vec![
                (Kind::DanglingImage, 400),
                (Kind::BuildCache, 300),
                (Kind::UnusedImage, 900),
                (Kind::StoppedContainer, 30)
            ]
        );
        assert_eq!(
            (
                report.volumes,
                report.volume_bytes,
                report.running_containers
            ),
            (1, 4000, 1)
        );
        let plan = make_plan(&report, &Kind::DEFAULT);
        assert_eq!(plan.items.len(), 2);
        assert_eq!(plan.bytes, 700);
        assert_eq!(
            plan.fingerprint,
            make_plan(&report, &[Kind::BuildCache, Kind::DanglingImage]).fingerprint
        );
        assert_ne!(plan.fingerprint, make_plan(&report, &Kind::ALL).fingerprint);
    }
}
