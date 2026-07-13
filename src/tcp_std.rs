//! Blocking TCP / TLS sockets backed by the modem, for the `std` runtime.
//!
//! This is the blocking counterpart of [`crate::tcp`] (which targets `embassy`
//! via `embedded-nal-async`). It exposes the modem's plain-TCP
//! (`AT+QIOPEN`/`QISEND`/`QIRD`/`QICLOSE`) and modem-terminated TLS
//! (`AT+QSSLOPEN`/…) engines as a socket that implements several standard
//! interfaces so it drops into existing code:
//!
//! - [`std::io::Read`] / [`std::io::Write`] — the "tokio-style" `TcpStream`
//!   interface (blocking).
//! - [`embedded_io::Read`] / [`embedded_io::Write`] — the portable embedded I/O
//!   traits (matching the `embedded-io-async` traits used on the embassy side).
//! - [`embedded_nal::TcpClientStack`] — the portable blocking network stack
//!   trait, via [`QuectelTcpStack`].
//!
//! # Sharing the modem
//!
//! Socket operations need `&mut` access to the driver, but the ergonomic
//! [`QuectelTcpClient`] hands out multiple sockets that each borrow the modem
//! per operation. The modem is therefore shared behind a
//! [`core::cell::RefCell`] (single-threaded interior mutability, matching the
//! `std` example's usage where the AT client lives on one thread and the ingress
//! reader on another).
//!
//! # Peer-close / EOF
//!
//! The modem signals readable data with `+QIURC: "recv"` / `+QSSLURC: "recv"`
//! and peer close with the corresponding `"closed"` URC. [`read`](std::io::Read)
//! polls for buffered data and, between polls, checks for the `"closed"` URC to
//! return EOF (`Ok(0)`). As a backstop it also gives up after an idle timeout
//! (see [`QuectelTcpStream::set_read_timeout`]) — this covers the case where a
//! `"closed"` URC was delivered while no subscriber was alive.
//!
//! # Example
//!
//! ```ignore
//! use std::cell::RefCell;
//! use std::io::{Read, Write};
//! use modem_manager_rs::Transport;
//! use modem_manager_rs::tcp_std::QuectelTcpClient;
//!
//! // `modem` is a fully-initialised, network-attached QuectelBG9X.
//! let modem = RefCell::new(modem);
//! let client = QuectelTcpClient::new(&modem);
//!
//! // Plain TCP:
//! let mut sock = client.connect("example.com", 80, Transport::Tcp)?;
//! sock.write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")?;
//! let mut body = Vec::new();
//! sock.read_to_end(&mut body)?;
//! ```

use core::cell::{Cell, RefCell};
use core::net::SocketAddr;
use std::io;
use std::time::{Duration, Instant};

use embedded_hal::digital::OutputPin;
use embedded_nal::{TcpClientStack, TcpError, TcpErrorKind};

use crate::cellular::QuectelBG9X;
use crate::{ModemError, Transport};

/// Default idle timeout used by blocking reads before returning EOF.
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Back-off between socket read polls while waiting for data.
const READ_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Error returned by the blocking modem-backed sockets.
#[derive(Debug)]
pub enum SocketError {
    /// The peer closed the connection.
    Closed,
    /// An underlying driver error.
    Modem(ModemError),
}

impl From<ModemError> for SocketError {
    fn from(e: ModemError) -> Self {
        SocketError::Modem(e)
    }
}

impl core::fmt::Display for SocketError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SocketError::Closed => write!(f, "socket closed by peer"),
            SocketError::Modem(e) => write!(f, "modem error: {e}"),
        }
    }
}

impl std::error::Error for SocketError {}

impl From<SocketError> for io::Error {
    fn from(e: SocketError) -> Self {
        let kind = match e {
            SocketError::Closed => io::ErrorKind::ConnectionReset,
            SocketError::Modem(_) => io::ErrorKind::Other,
        };
        io::Error::new(kind, e)
    }
}

impl embedded_io::Error for SocketError {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            SocketError::Closed => embedded_io::ErrorKind::ConnectionReset,
            SocketError::Modem(_) => embedded_io::ErrorKind::Other,
        }
    }
}

impl TcpError for SocketError {
    fn kind(&self) -> TcpErrorKind {
        match self {
            SocketError::Closed => TcpErrorKind::PipeClosed,
            SocketError::Modem(_) => TcpErrorKind::Other,
        }
    }
}

/// Ergonomic factory for [`QuectelTcpStream`]s over a shared modem.
///
/// Create one and call [`connect`](Self::connect) for each connection; every
/// connection allocates the next socket (`clientID`) identifier.
pub struct QuectelTcpClient<'a, W: embedded_io::Write, P: OutputPin> {
    modem: &'a RefCell<QuectelBG9X<W, P>>,
    next_id: Cell<u8>,
}

impl<'a, W: embedded_io::Write, P: OutputPin> QuectelTcpClient<'a, W, P> {
    /// Create a client over a shared, already-initialised modem.
    pub fn new(modem: &'a RefCell<QuectelBG9X<W, P>>) -> Self {
        Self {
            modem,
            next_id: Cell::new(0),
        }
    }

    /// Open a connection to `host:port` using `transport`.
    ///
    /// `host` may be a hostname or IP; it is passed to the modem verbatim (so it
    /// is used for SNI / hostname verification on TLS when those are enabled on
    /// the SSL context). For [`Transport::Tls`] the SSL context must already be
    /// configured on the modem via
    /// [`configure_ssl_context`](crate::cellular::QuectelBG9X::configure_ssl_context).
    pub fn connect(
        &self,
        host: &str,
        port: u16,
        transport: Transport,
    ) -> Result<QuectelTcpStream<'a, W, P>, SocketError> {
        let client_id = self.next_id.get();
        self.next_id.set(client_id.wrapping_add(1));

        {
            let mut modem = self.modem.borrow_mut();
            match transport {
                Transport::Tcp => modem.tcp_socket_open(client_id, host, port)?,
                Transport::Tls { ssl_ctx_id } => {
                    modem.ssl_socket_open(client_id, ssl_ctx_id, host, port)?
                }
            }
        }

        Ok(QuectelTcpStream {
            modem: self.modem,
            client_id,
            transport,
            read_timeout: Some(DEFAULT_READ_TIMEOUT),
            closed: false,
        })
    }
}

/// An open, blocking TCP (or TLS) connection to a peer.
///
/// Implements [`std::io::Read`]/[`Write`](std::io::Write) and
/// [`embedded_io::Read`]/[`Write`](embedded_io::Write). Dropping the stream
/// best-effort closes the socket on the modem; call [`close`](Self::close)
/// explicitly to observe the result.
pub struct QuectelTcpStream<'a, W: embedded_io::Write, P: OutputPin> {
    modem: &'a RefCell<QuectelBG9X<W, P>>,
    client_id: u8,
    transport: Transport,
    read_timeout: Option<Duration>,
    closed: bool,
}

impl<W: embedded_io::Write, P: OutputPin> QuectelTcpStream<'_, W, P> {
    /// The modem socket identifier (`clientID`) backing this connection.
    pub fn client_id(&self) -> u8 {
        self.client_id
    }

    /// The transport this socket was opened with.
    pub fn transport(&self) -> Transport {
        self.transport
    }

    /// Set the idle timeout after which a blocking read returns EOF (`Ok(0)`).
    ///
    /// `None` blocks indefinitely until data arrives or a `"closed"` URC is
    /// observed. The default is 30 s.
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) {
        self.read_timeout = timeout;
    }

    /// Explicitly close the socket on the modem.
    pub fn close(mut self) -> Result<(), SocketError> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> Result<(), SocketError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut modem = self.modem.borrow_mut();
        match self.transport {
            Transport::Tcp => modem.tcp_socket_close(self.client_id)?,
            Transport::Tls { .. } => modem.ssl_socket_close(self.client_id)?,
        }
        Ok(())
    }

    /// One non-blocking read attempt; `Ok(0)` means "no data buffered now".
    fn recv_once(&self, buf: &mut [u8]) -> Result<usize, SocketError> {
        let mut modem = self.modem.borrow_mut();
        let n = match self.transport {
            Transport::Tcp => modem.tcp_socket_recv(self.client_id, buf)?,
            Transport::Tls { .. } => modem.ssl_socket_recv(self.client_id, buf)?,
        };
        Ok(n)
    }

    fn poll_closed(&self) -> bool {
        self.modem.borrow_mut().socket_poll_closed(self.client_id)
    }

    fn do_send(&self, buf: &[u8]) -> Result<(), SocketError> {
        let mut modem = self.modem.borrow_mut();
        match self.transport {
            Transport::Tcp => modem.tcp_socket_send(self.client_id, buf)?,
            Transport::Tls { .. } => modem.ssl_socket_send(self.client_id, buf)?,
        }
        Ok(())
    }

    /// Block until at least one byte is available, the peer closes (EOF), or the
    /// idle timeout elapses (also reported as EOF).
    fn blocking_read(&mut self, buf: &mut [u8]) -> Result<usize, SocketError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let start = Instant::now();
        loop {
            let n = self.recv_once(buf)?;
            if n > 0 {
                return Ok(n);
            }
            if self.poll_closed() {
                // Drain any final bytes delivered alongside the close, then EOF.
                self.closed = true;
                return self.recv_once(buf);
            }
            if let Some(timeout) = self.read_timeout {
                if start.elapsed() >= timeout {
                    return Ok(0);
                }
            }
            std::thread::sleep(READ_POLL_INTERVAL);
        }
    }
}

impl<W: embedded_io::Write, P: OutputPin> Drop for QuectelTcpStream<'_, W, P> {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        // Best-effort; only if the modem isn't currently borrowed elsewhere.
        if self.modem.try_borrow_mut().is_ok() {
            let _ = self.close_inner();
        }
    }
}

// ---- std::io -------------------------------------------------------------

impl<W: embedded_io::Write, P: OutputPin> io::Read for QuectelTcpStream<'_, W, P> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Ok(self.blocking_read(buf)?)
    }
}

impl<W: embedded_io::Write, P: OutputPin> io::Write for QuectelTcpStream<'_, W, P> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.do_send(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // Each `write` pushes straight to the modem, so there is nothing buffered.
        Ok(())
    }
}

// ---- embedded-io ---------------------------------------------------------

impl<W: embedded_io::Write, P: OutputPin> embedded_io::ErrorType for QuectelTcpStream<'_, W, P> {
    type Error = SocketError;
}

impl<W: embedded_io::Write, P: OutputPin> embedded_io::Read for QuectelTcpStream<'_, W, P> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.blocking_read(buf)
    }
}

impl<W: embedded_io::Write, P: OutputPin> embedded_io::Write for QuectelTcpStream<'_, W, P> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.do_send(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

// ---- embedded-nal (blocking TcpClientStack) ------------------------------

/// A socket handle for the [`embedded_nal::TcpClientStack`] implementation.
///
/// Created by [`QuectelTcpStack::socket`]; carries the transport and, once
/// [`connect`](TcpClientStack::connect)ed, the allocated `clientID`.
pub struct QuectelSocket {
    client_id: Option<u8>,
    transport: Transport,
}

/// An [`embedded_nal::TcpClientStack`] over a shared modem.
///
/// Sockets it creates use the stack's configured [`Transport`]. Note that
/// [`TcpClientStack::connect`] only receives a [`SocketAddr`] (an IP), so this
/// path cannot supply a hostname for TLS SNI — use [`QuectelTcpClient`] when you
/// need hostnames.
pub struct QuectelTcpStack<'a, W: embedded_io::Write, P: OutputPin> {
    modem: &'a RefCell<QuectelBG9X<W, P>>,
    transport: Transport,
    next_id: Cell<u8>,
}

impl<'a, W: embedded_io::Write, P: OutputPin> QuectelTcpStack<'a, W, P> {
    /// Create a stack whose sockets use `transport`.
    pub fn new(modem: &'a RefCell<QuectelBG9X<W, P>>, transport: Transport) -> Self {
        Self {
            modem,
            transport,
            next_id: Cell::new(0),
        }
    }
}

impl<W: embedded_io::Write, P: OutputPin> TcpClientStack for QuectelTcpStack<'_, W, P> {
    type TcpSocket = QuectelSocket;
    type Error = SocketError;

    fn socket(&mut self) -> Result<Self::TcpSocket, Self::Error> {
        Ok(QuectelSocket {
            client_id: None,
            transport: self.transport,
        })
    }

    fn connect(
        &mut self,
        socket: &mut Self::TcpSocket,
        remote: SocketAddr,
    ) -> nb::Result<(), Self::Error> {
        let client_id = self.next_id.get();
        self.next_id.set(client_id.wrapping_add(1));

        // TcpClientStack only gives us an IP; render it as the open host.
        let host = format!("{}", remote.ip());

        let mut modem = self.modem.borrow_mut();
        match socket.transport {
            Transport::Tcp => modem
                .tcp_socket_open(client_id, &host, remote.port())
                .map_err(|e| nb::Error::Other(SocketError::Modem(e)))?,
            Transport::Tls { ssl_ctx_id } => modem
                .ssl_socket_open(client_id, ssl_ctx_id, &host, remote.port())
                .map_err(|e| nb::Error::Other(SocketError::Modem(e)))?,
        }
        socket.client_id = Some(client_id);
        Ok(())
    }

    fn send(
        &mut self,
        socket: &mut Self::TcpSocket,
        buffer: &[u8],
    ) -> nb::Result<usize, Self::Error> {
        let id = socket.client_id.ok_or(nb::Error::Other(SocketError::Modem(
            ModemError::SocketSendFailed,
        )))?;
        let mut modem = self.modem.borrow_mut();
        match socket.transport {
            Transport::Tcp => modem.tcp_socket_send(id, buffer),
            Transport::Tls { .. } => modem.ssl_socket_send(id, buffer),
        }
        .map_err(|e| nb::Error::Other(SocketError::Modem(e)))?;
        Ok(buffer.len())
    }

    fn receive(
        &mut self,
        socket: &mut Self::TcpSocket,
        buffer: &mut [u8],
    ) -> nb::Result<usize, Self::Error> {
        let id = socket.client_id.ok_or(nb::Error::Other(SocketError::Modem(
            ModemError::SocketRecvFailed,
        )))?;
        let mut modem = self.modem.borrow_mut();
        let n = match socket.transport {
            Transport::Tcp => modem.tcp_socket_recv(id, buffer),
            Transport::Tls { .. } => modem.ssl_socket_recv(id, buffer),
        }
        .map_err(|e| nb::Error::Other(SocketError::Modem(e)))?;
        if n > 0 {
            return Ok(n);
        }
        if modem.socket_poll_closed(id) {
            return Err(nb::Error::Other(SocketError::Closed));
        }
        // No data buffered right now: the caller should retry.
        Err(nb::Error::WouldBlock)
    }

    fn close(&mut self, socket: Self::TcpSocket) -> Result<(), Self::Error> {
        if let Some(id) = socket.client_id {
            let mut modem = self.modem.borrow_mut();
            match socket.transport {
                Transport::Tcp => modem.tcp_socket_close(id)?,
                Transport::Tls { .. } => modem.ssl_socket_close(id)?,
            }
        }
        Ok(())
    }
}
