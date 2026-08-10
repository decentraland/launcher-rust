use std::io;
#[cfg(unix)]
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use log::info;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::protocol::{Command, Frame, Response};
use crate::status::Status;

#[cfg(windows)]
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

#[cfg(windows)]
const PIPE_BASE: &str = r"\\.\pipe\dcl-launcher-service";

/// Test-only escape hatch: hermetic e2e runs give every service instance its
/// own endpoint. Debug builds only; release always uses the default endpoint.
fn env_endpoint_suffix() -> Option<String> {
    #[cfg(debug_assertions)]
    {
        std::env::var("DCL_LAUNCHER_IPC_ENDPOINT")
            .ok()
            .filter(|s| !s.is_empty())
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

#[cfg(windows)]
fn pipe_name(suffix: Option<&str>) -> String {
    suffix.map_or_else(|| PIPE_BASE.to_owned(), |s| format!("{PIPE_BASE}-{s}"))
}

/// Default endpoint: the socket lives in the launcher's data dir. Suffixed
/// (test-only) endpoints instead live at a fixed short path under `/tmp`:
/// the suffix alone must determine the endpoint on both ends — the test
/// process that connects cannot see the service's `DCL_LAUNCHER_BASE_DIR`
/// redirect, so an `app_dir()`-relative path would resolve differently in
/// each process. Named pipes on Windows are name-addressed and already
/// behave this way. `/tmp` also keeps the path under the ~104-byte macOS
/// socket-path cap.
#[cfg(unix)]
pub fn socket_path_for(suffix: Option<&str>) -> PathBuf {
    suffix.map_or_else(
        || dcl_launcher_shared::app_dir().join("service.sock"),
        |s| PathBuf::from("/tmp").join(format!("dcl-launcher-service-{s}.sock")),
    )
}

#[cfg(unix)]
pub fn socket_path() -> PathBuf {
    socket_path_for(env_endpoint_suffix().as_deref())
}

#[derive(Debug, thiserror::Error)]
pub enum BindError {
    #[error("another service instance already owns the IPC endpoint")]
    AlreadyRunning,
    #[error("cannot bind the IPC endpoint: {0}")]
    Io(#[from] io::Error),
}

type BoxedReader = Box<dyn AsyncRead + Send + Unpin>;
type BoxedWriter = Box<dyn AsyncWrite + Send + Unpin>;

pub struct FrameReader {
    inner: BufReader<BoxedReader>,
    line: String,
}

impl FrameReader {
    fn new(reader: BoxedReader) -> Self {
        Self {
            inner: BufReader::new(reader),
            line: String::new(),
        }
    }

    /// Reads the next frame. `Ok(None)` means the peer closed the connection.
    pub async fn read(&mut self) -> Result<Option<Frame>> {
        loop {
            self.line.clear();
            let bytes = self
                .inner
                .read_line(&mut self.line)
                .await
                .context("Cannot read from the IPC connection")?;
            if bytes == 0 {
                return Ok(None);
            }
            let raw = self.line.trim();
            if raw.is_empty() {
                continue;
            }
            let frame: Frame =
                serde_json::from_str(raw).with_context(|| format!("Malformed frame: {raw}"))?;
            return Ok(Some(frame));
        }
    }
}

pub struct FrameWriter {
    inner: BoxedWriter,
}

impl FrameWriter {
    const fn new(writer: BoxedWriter) -> Self {
        Self { inner: writer }
    }

    pub async fn write(&mut self, frame: &Frame) -> Result<()> {
        let mut payload = serde_json::to_string(frame).context("Cannot serialize the IPC frame")?;
        payload.push('\n');
        self.inner
            .write_all(payload.as_bytes())
            .await
            .context("Cannot write to the IPC connection")?;
        self.inner
            .flush()
            .await
            .context("Cannot flush the IPC connection")?;
        Ok(())
    }
}

pub struct IpcConnection {
    reader: FrameReader,
    writer: FrameWriter,
}

impl IpcConnection {
    fn from_halves(reader: BoxedReader, writer: BoxedWriter) -> Self {
        Self {
            reader: FrameReader::new(reader),
            writer: FrameWriter::new(writer),
        }
    }

    #[must_use]
    pub fn split(self) -> (FrameReader, FrameWriter) {
        (self.reader, self.writer)
    }
}

pub struct IpcServer {
    #[cfg(windows)]
    next: NamedPipeServer,
    #[cfg(windows)]
    name: String,
    #[cfg(unix)]
    listener: UnixListener,
}

impl IpcServer {
    /// Binds the IPC endpoint (default or env-overridden in debug). Binding
    /// doubles as the single-instance lock: [`BindError::AlreadyRunning`]
    /// means another service owns it.
    pub fn bind() -> Result<Self, BindError> {
        Self::bind_to(env_endpoint_suffix().as_deref())
    }

    /// Binds an explicitly suffixed endpoint (tests use this to host fake
    /// services without touching process-global env).
    #[cfg(windows)]
    pub fn bind_to(suffix: Option<&str>) -> Result<Self, BindError> {
        let name = pipe_name(suffix);
        let next = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .map_err(|e| {
                if e.kind() == io::ErrorKind::PermissionDenied {
                    BindError::AlreadyRunning
                } else {
                    BindError::Io(e)
                }
            })?;
        Ok(Self { next, name })
    }

    /// Binds an explicitly suffixed endpoint (tests use this to host fake
    /// services without touching process-global env).
    #[cfg(unix)]
    pub fn bind_to(suffix: Option<&str>) -> Result<Self, BindError> {
        let path = socket_path_for(suffix);
        match UnixListener::bind(&path) {
            Ok(listener) => Ok(Self { listener }),
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                match std::os::unix::net::UnixStream::connect(&path) {
                    Ok(_) => Err(BindError::AlreadyRunning),
                    Err(_) => {
                        std::fs::remove_file(&path)?;
                        let listener = UnixListener::bind(&path)?;
                        Ok(Self { listener })
                    }
                }
            }
            Err(e) => Err(BindError::Io(e)),
        }
    }

    #[cfg(windows)]
    pub async fn accept(&mut self) -> io::Result<IpcConnection> {
        self.next.connect().await?;
        let fresh = ServerOptions::new().create(&self.name)?;
        let connected = std::mem::replace(&mut self.next, fresh);
        let (reader, writer) = tokio::io::split(connected);
        Ok(IpcConnection::from_halves(
            Box::new(reader),
            Box::new(writer),
        ))
    }

    #[cfg(unix)]
    pub async fn accept(&mut self) -> io::Result<IpcConnection> {
        let (stream, _addr) = self.listener.accept().await?;
        let (reader, writer) = tokio::io::split(stream);
        Ok(IpcConnection::from_halves(
            Box::new(reader),
            Box::new(writer),
        ))
    }

    /// Removes the endpoint artifacts on graceful shutdown.
    #[cfg(unix)]
    pub fn cleanup_endpoint() {
        let path = socket_path();
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != io::ErrorKind::NotFound {
                log::warn!("Cannot remove the service socket {}: {e}", path.display());
            }
        }
    }

    /// Removes the endpoint artifacts on graceful shutdown (no-op: a named
    /// pipe disappears with its last handle).
    #[cfg(windows)]
    pub const fn cleanup_endpoint() {}
}

pub struct IpcClient {
    reader: FrameReader,
    writer: FrameWriter,
    next_id: u64,
}

impl IpcClient {
    /// Connects to the default endpoint (env-overridden in debug builds).
    pub async fn connect() -> io::Result<Self> {
        Self::connect_to(env_endpoint_suffix().as_deref()).await
    }

    /// `async` only for signature parity with the unix impl. Explicit suffix
    /// lets tests target a specific service without process-global env.
    #[cfg(windows)]
    #[allow(clippy::unused_async)]
    pub async fn connect_to(suffix: Option<&str>) -> io::Result<Self> {
        let stream = ClientOptions::new().open(pipe_name(suffix))?;
        let (reader, writer) = tokio::io::split(stream);
        Ok(Self::from_halves(Box::new(reader), Box::new(writer)))
    }

    /// Explicit suffix lets tests target a specific service without
    /// process-global env.
    #[cfg(unix)]
    pub async fn connect_to(suffix: Option<&str>) -> io::Result<Self> {
        let stream = UnixStream::connect(socket_path_for(suffix)).await?;
        let (reader, writer) = tokio::io::split(stream);
        Ok(Self::from_halves(Box::new(reader), Box::new(writer)))
    }

    fn from_halves(reader: BoxedReader, writer: BoxedWriter) -> Self {
        Self {
            reader: FrameReader::new(reader),
            writer: FrameWriter::new(writer),
            next_id: 1,
        }
    }

    /// Sends a command and waits for its response. Unsolicited
    /// [`Frame::Event`] frames received in the meantime go to `on_event`.
    pub async fn request(
        &mut self,
        cmd: Command,
        mut on_event: impl FnMut(Status),
    ) -> Result<Response> {
        info!("IPC command request: {cmd:?}");

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        self.writer.write(&Frame::Req { id, cmd }).await?;

        loop {
            let frame = self
                .reader
                .read()
                .await?
                .ok_or_else(|| anyhow!("The service closed the IPC connection"))?;

            info!("IPC frame received: {frame:?}");

            match frame {
                Frame::Event { status } => on_event(status),
                Frame::Res { id: res_id, result } if res_id == id => return Ok(result),
                Frame::Res { id: res_id, .. } => {
                    log::warn!("Dropping stale response with id {res_id}, expected {id}");
                }
                Frame::Req { id: req_id, .. } => {
                    log::warn!("Unexpected request frame from the service (id {req_id})");
                }
            }
        }
    }

    pub async fn request_silent(&mut self, cmd: Command) -> Result<Response> {
        self.request(cmd, |_| {}).await
    }
}
