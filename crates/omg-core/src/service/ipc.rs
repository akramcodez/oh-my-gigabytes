//! Local, authenticated, scan-only IPC. The protocol is a closed enum: there is no way to express
//! cleanup, plan approval or catalog changes, so forged messages cannot reach destructive code.
//! Peers are authenticated by kernel-reported credentials of the connected Unix socket.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::Duration,
};

pub const PROTOCOL: u32 = 1;
pub const MAX_MESSAGE: u64 = 512;
const SOCKET: &str = "service.sock";
const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Clients wait longer than the server's per-connection timeout: a loaded service may be inside a
/// bounded SQLite wait before it reaches the socket.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Status,
    /// Ask for a reconciliation scan at the service's own low priority.
    Scan,
    /// Foreground handoff: pause scheduled work so a user-initiated scan can take the scan lock.
    Yield,
    Stop,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub v: u32,
    pub op: Operation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}
impl Response {
    pub fn ok(data: Option<serde_json::Value>) -> Self {
        Self {
            ok: true,
            error: None,
            data,
        }
    }
    pub fn fail(message: &str) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            data: None,
        }
    }
}

pub fn socket_path(state: &Path) -> PathBuf {
    state.join(SOCKET)
}

/// Unix socket paths are limited to ~108 bytes, which deep state directories exceed. Anchor on an
/// opened, non-symlink directory descriptor and address the socket through `/proc/self/fd` so the
/// limit never depends on the state directory's depth (and ancestor swaps cannot redirect it).
fn anchored(state: &Path) -> Result<(rustix::fd::OwnedFd, PathBuf)> {
    use rustix::fs::{Mode, OFlags};
    use std::os::fd::AsRawFd;
    let fd = rustix::fs::open(
        state,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .context("state directory is not accessible")?;
    let path = PathBuf::from(format!("/proc/self/fd/{}/{SOCKET}", fd.as_raw_fd()));
    Ok((fd, path))
}

fn peer_is_current_user(stream: &UnixStream) -> bool {
    rustix::net::sockopt::socket_peercred(stream)
        .is_ok_and(|cred| cred.uid == rustix::process::geteuid())
}

/// Listener owned by the single service process (which already holds `service.lock`).
pub struct Server {
    listener: UnixListener,
    _anchor: rustix::fd::OwnedFd,
    path: PathBuf,
    pub rejected_peers: u64,
}
impl Server {
    pub fn bind(state: &Path) -> Result<Self> {
        let (anchor, path) = anchored(state)?;
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_socket() => std::fs::remove_file(&path)?,
            Ok(_) => bail!("refusing to replace a non-socket at the service socket path"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let listener = UnixListener::bind(&path).context("cannot bind service socket")?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            _anchor: anchor,
            path,
            rejected_peers: 0,
        })
    }
    /// Accept at most one pending connection and answer it using `handle`. Never blocks long.
    pub fn poll(&mut self, mut handle: impl FnMut(Operation) -> Response) -> Result<()> {
        let (mut stream, _) = match self.listener.accept() {
            Ok(pair) => pair,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        if !peer_is_current_user(&stream) {
            self.rejected_peers += 1;
            return Ok(());
        }
        let response = match read_line(&mut stream)
            .and_then(|line| serde_json::from_str::<Request>(&line).map_err(Into::into))
        {
            Ok(request) if request.v == PROTOCOL => handle(request.op),
            Ok(_) => Response::fail("unsupported protocol version"),
            Err(_) => Response::fail("malformed or unsupported request"),
        };
        let mut bytes = serde_json::to_vec(&response)?;
        bytes.push(b'\n');
        let _ = stream.write_all(&bytes);
        Ok(())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn read_line(stream: &mut UnixStream) -> Result<String> {
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    while stream.read(&mut byte)? == 1 && byte[0] != b'\n' {
        buffer.push(byte[0]);
        if buffer.len() as u64 > MAX_MESSAGE {
            bail!("message too large");
        }
    }
    Ok(String::from_utf8(buffer)?)
}

/// Send one request to the service owned by the same user.
pub fn request(state: &Path, op: Operation) -> Result<Response> {
    let (_anchor, path) = anchored(state)?;
    let mut stream = UnixStream::connect(path).context("service is not reachable")?;
    stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
    if !peer_is_current_user(&stream) {
        bail!("service socket is owned by a different user");
    }
    let mut bytes = serde_json::to_vec(&Request { v: PROTOCOL, op })?;
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    let mut text = String::new();
    stream.take(64 * 1024).read_to_string(&mut text)?;
    Ok(serde_json::from_str(text.trim())?)
}
