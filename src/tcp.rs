//! Async TCP / TLS sockets backed by the modem, exposed through the
//! [`embedded-nal-async`](https://docs.rs/embedded-nal-async) traits.
//!
//! This module is only available with the `embassy` feature (the
//! `embedded-nal-async` traits are async-only). It supports **both** transports
//! the modem offers, selected per client via [`Transport`](crate::Transport):
//!
//! - [`Transport::Tcp`](crate::Transport::Tcp) — plain TCP
//!   (`AT+QIOPEN`/`QISEND`/`QIRD`/`QICLOSE`).
//! - [`Transport::Tls`](crate::Transport::Tls) — modem-terminated TLS on a
//!   pre-configured SSL context (`AT+QSSLOPEN`/…). Because the modem terminates
//!   TLS itself (see [`crate::cellular::QuectelBG9X::configure_ssl_context`]),
//!   the certificate/key for mutual TLS live in the modem's flash, not on the
//!   MCU.
//!
//! # Sharing the modem
//!
//! [`embedded_nal_async::TcpConnect::connect`] takes `&self`, yet issuing AT
//! commands needs `&mut` access to the driver. The modem is therefore shared
//! behind an [`embassy_sync::mutex::Mutex`]; each socket operation locks it for
//! the duration of a single command.
//!
//! # SNI / hostname note
//!
//! [`TcpConnect::connect`] only receives a [`core::net::SocketAddr`] (an IP), so
//! this path cannot supply a hostname for SNI or hostname verification. When you
//! need those for TLS, call
//! [`QuectelBG9X::ssl_socket_open`](crate::cellular::QuectelBG9X::ssl_socket_open)
//! directly with the hostname and enable `sni`/`checkhost` on the SSL context.
//!
//! # Example
//!
//! ```ignore
//! use embassy_sync::blocking_mutex::raw::NoopRawMutex;
//! use embassy_sync::mutex::Mutex;
//! use embedded_nal_async::TcpConnect;
//! use embedded_io_async::{Read, Write};
//! use quectel_bg9x_eh_driver::Transport;
//!
//! // `modem` is a fully-initialised, network-attached QuectelBG9X.
//! let modem: Mutex<NoopRawMutex, _> = Mutex::new(modem);
//!
//! // Plain TCP:
//! let tcp = QuectelTcpClient::new(&modem, Transport::Tcp);
//! let mut sock = tcp.connect("93.184.216.34:80".parse().unwrap()).await?;
//!
//! // TLS on SSL context 2 (already configured via configure_ssl_context):
//! let tls = QuectelTcpClient::new(&modem, Transport::Tls { ssl_ctx_id: 2 });
//! let mut sock = tls.connect("93.184.216.34:443".parse().unwrap()).await?;
//!
//! sock.write_all(b"hello").await?;
//! let mut buf = [0u8; 64];
//! let n = sock.read(&mut buf).await?;
//! sock.close().await?;
//! ```

use core::fmt::Write as _;
use core::net::SocketAddr;

use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use embedded_hal::digital::OutputPin;
use embedded_io_async::{Error as IoError, ErrorKind, ErrorType, Read, Write};
use embedded_nal_async::TcpConnect;

use crate::cellular::QuectelBG9X;
use crate::{ModemError, Transport};

/// Error returned by the modem-backed sockets.
///
/// Wraps the driver's [`ModemError`] and maps it onto
/// [`embedded_io_async::ErrorKind::Other`] (the modem does not expose enough
/// detail to distinguish finer kinds).
#[derive(Debug)]
pub struct SocketError(pub ModemError);

impl From<ModemError> for SocketError {
    fn from(e: ModemError) -> Self {
        Self(e)
    }
}

impl IoError for SocketError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

/// An [`embedded_nal_async::TcpConnect`] implementation over a shared modem.
///
/// Every [`connect`](TcpConnect::connect) allocates the next socket
/// (`clientID`) identifier and opens a connection using the client's
/// [`Transport`]. Create one per transport (and, for TLS, per SSL context) you
/// want to use.
pub struct QuectelTcpClient<'a, M: RawMutex, W: Write, P: OutputPin> {
    modem: &'a Mutex<M, QuectelBG9X<W, P>>,
    transport: Transport,
    next_id: core::cell::Cell<u8>,
}

impl<'a, M: RawMutex, W: Write, P: OutputPin> QuectelTcpClient<'a, M, W, P> {
    /// Create a client that opens sockets using `transport`.
    ///
    /// For [`Transport::Tls`] the SSL context must already be configured on
    /// `modem` via
    /// [`QuectelBG9X::configure_ssl_context`](crate::cellular::QuectelBG9X::configure_ssl_context).
    pub fn new(modem: &'a Mutex<M, QuectelBG9X<W, P>>, transport: Transport) -> Self {
        Self {
            modem,
            transport,
            next_id: core::cell::Cell::new(0),
        }
    }

    /// Convenience constructor for a plain-TCP client.
    pub fn new_tcp(modem: &'a Mutex<M, QuectelBG9X<W, P>>) -> Self {
        Self::new(modem, Transport::Tcp)
    }

    /// Convenience constructor for a TLS client on SSL context `ssl_ctx_id`.
    pub fn new_tls(modem: &'a Mutex<M, QuectelBG9X<W, P>>, ssl_ctx_id: u8) -> Self {
        Self::new(modem, Transport::Tls { ssl_ctx_id })
    }
}

impl<M: RawMutex, W: Write, P: OutputPin> TcpConnect for QuectelTcpClient<'_, M, W, P> {
    type Error = SocketError;
    type Connection<'m>
        = ModemSocket<'m, M, W, P>
    where
        Self: 'm;

    async fn connect(&self, remote: SocketAddr) -> Result<Self::Connection<'_>, Self::Error> {
        let client_id = self.next_id.get();
        self.next_id.set(client_id.wrapping_add(1));

        // TcpConnect only gives us an IP; render it as the open host.
        let mut host = atat::heapless::String::<64>::new();
        write!(host, "{}", remote.ip()).map_err(|_| SocketError(ModemError::NotSupported))?;

        {
            let mut modem = self.modem.lock().await;
            match self.transport {
                Transport::Tcp => {
                    modem
                        .tcp_socket_open(client_id, host.as_str(), remote.port())
                        .await?;
                }
                Transport::Tls { ssl_ctx_id } => {
                    modem
                        .ssl_socket_open(client_id, ssl_ctx_id, host.as_str(), remote.port())
                        .await?;
                }
            }
        }

        Ok(ModemSocket {
            modem: self.modem,
            client_id,
            transport: self.transport,
        })
    }
}

/// An open TCP (or TLS) connection to a peer.
///
/// Implements [`embedded_io_async::Read`] / [`Write`]. Call [`close`](Self::close)
/// to release the socket on the modem — [`Drop`] cannot do so because closing is
/// asynchronous.
pub struct ModemSocket<'a, M: RawMutex, W: Write, P: OutputPin> {
    modem: &'a Mutex<M, QuectelBG9X<W, P>>,
    client_id: u8,
    transport: Transport,
}

/// Back-compatible alias: modem sockets used to be TLS-only.
pub type TlsSocket<'a, M, W, P> = ModemSocket<'a, M, W, P>;

impl<M: RawMutex, W: Write, P: OutputPin> ModemSocket<'_, M, W, P> {
    /// The modem socket identifier (`clientID`) backing this connection.
    pub fn client_id(&self) -> u8 {
        self.client_id
    }

    /// The transport this socket was opened with.
    pub fn transport(&self) -> Transport {
        self.transport
    }

    /// Close the socket on the modem.
    pub async fn close(self) -> Result<(), SocketError> {
        let mut modem = self.modem.lock().await;
        match self.transport {
            Transport::Tcp => modem.tcp_socket_close(self.client_id).await?,
            Transport::Tls { .. } => modem.ssl_socket_close(self.client_id).await?,
        }
        Ok(())
    }
}

impl<M: RawMutex, W: Write, P: OutputPin> ErrorType for ModemSocket<'_, M, W, P> {
    type Error = SocketError;
}

impl<M: RawMutex, W: Write, P: OutputPin> Write for ModemSocket<'_, M, W, P> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut modem = self.modem.lock().await;
        match self.transport {
            Transport::Tcp => modem.tcp_socket_send(self.client_id, buf).await?,
            Transport::Tls { .. } => modem.ssl_socket_send(self.client_id, buf).await?,
        }
        Ok(buf.len())
    }
}

impl<M: RawMutex, W: Write, P: OutputPin> Read for ModemSocket<'_, M, W, P> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        // `read` must block until at least one byte is available or the peer
        // closes. The modem's QIRD/QSSLRECV returns 0 when its buffer is
        // momentarily empty, so poll with a short back-off; between polls, check
        // for the `"closed"` URC and return EOF (Ok(0)) when the peer is gone.
        loop {
            {
                let mut modem = self.modem.lock().await;
                let n = match self.transport {
                    Transport::Tcp => modem.tcp_socket_recv(self.client_id, buf).await?,
                    Transport::Tls { .. } => modem.ssl_socket_recv(self.client_id, buf).await?,
                };
                if n > 0 {
                    return Ok(n);
                }
                if modem.socket_poll_closed(self.client_id).await {
                    // Drain any last bytes that arrived alongside the close.
                    let n = match self.transport {
                        Transport::Tcp => modem.tcp_socket_recv(self.client_id, buf).await?,
                        Transport::Tls { .. } => modem.ssl_socket_recv(self.client_id, buf).await?,
                    };
                    return Ok(n);
                }
            }
            Timer::after(Duration::from_millis(100)).await;
        }
    }
}
