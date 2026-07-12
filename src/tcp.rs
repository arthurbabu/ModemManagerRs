//! Async TCP+TLS sockets backed by the modem, exposed through the
//! [`embedded-nal-async`](https://docs.rs/embedded-nal-async) traits.
//!
//! This module is only available with the `embassy` feature (the
//! `embedded-nal-async` traits are async-only). The modem terminates TLS
//! itself (see [`crate::cellular::QuectelBG9X::configure_ssl_context`] and the
//! `AT+QSSLCFG` commands), so — unlike a PPP + smoltcp + `embedded-tls` setup —
//! the client certificate and key for mutual TLS live in the modem's flash, not
//! on the MCU.
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
//! the [`QuectelTcpClient`] path cannot supply a hostname for SNI or hostname
//! verification. When you need those, call
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
//!
//! // `modem` is a fully-initialised, network-attached QuectelBG9X with an SSL
//! // context (id 2) already configured for mutual TLS.
//! let modem: Mutex<NoopRawMutex, _> = Mutex::new(modem);
//! let client = QuectelTcpClient::new(&modem, 2);
//!
//! let mut socket = client.connect("93.184.216.34:8883".parse().unwrap()).await?;
//! socket.write_all(b"hello").await?;
//! let mut buf = [0u8; 64];
//! let n = socket.read(&mut buf).await?;
//! socket.close().await?;
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
use crate::ModemError;

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
/// (`clientID`) identifier and opens a TLS connection on the configured SSL
/// context. Create one per SSL context you want to use.
pub struct QuectelTcpClient<'a, M: RawMutex, W: Write, P: OutputPin> {
    modem: &'a Mutex<M, QuectelBG9X<W, P>>,
    ssl_ctx_id: u8,
    next_id: core::cell::Cell<u8>,
}

impl<'a, M: RawMutex, W: Write, P: OutputPin> QuectelTcpClient<'a, M, W, P> {
    /// Create a client that opens sockets on SSL context `ssl_ctx_id`.
    ///
    /// The SSL context must already be configured on `modem` via
    /// [`QuectelBG9X::configure_ssl_context`](crate::cellular::QuectelBG9X::configure_ssl_context).
    pub fn new(modem: &'a Mutex<M, QuectelBG9X<W, P>>, ssl_ctx_id: u8) -> Self {
        Self {
            modem,
            ssl_ctx_id,
            next_id: core::cell::Cell::new(0),
        }
    }
}

impl<M: RawMutex, W: Write, P: OutputPin> TcpConnect for QuectelTcpClient<'_, M, W, P> {
    type Error = SocketError;
    type Connection<'m>
        = TlsSocket<'m, M, W, P>
    where
        Self: 'm;

    async fn connect(&self, remote: SocketAddr) -> Result<Self::Connection<'_>, Self::Error> {
        let client_id = self.next_id.get();
        self.next_id.set(client_id.wrapping_add(1));

        // TcpConnect only gives us an IP; render it as the QSSLOPEN host.
        let mut host = atat::heapless::String::<64>::new();
        write!(host, "{}", remote.ip()).map_err(|_| SocketError(ModemError::NotSupported))?;

        {
            let mut modem = self.modem.lock().await;
            modem
                .ssl_socket_open(client_id, self.ssl_ctx_id, host.as_str(), remote.port())
                .await?;
        }

        Ok(TlsSocket {
            modem: self.modem,
            client_id,
        })
    }
}

/// An open TCP+TLS connection to a peer.
///
/// Implements [`embedded_io_async::Read`] / [`Write`]. Call [`close`](Self::close)
/// to release the socket on the modem — [`Drop`] cannot do so because closing is
/// asynchronous.
pub struct TlsSocket<'a, M: RawMutex, W: Write, P: OutputPin> {
    modem: &'a Mutex<M, QuectelBG9X<W, P>>,
    client_id: u8,
}

impl<M: RawMutex, W: Write, P: OutputPin> TlsSocket<'_, M, W, P> {
    /// The modem socket identifier (`clientID`) backing this connection.
    pub fn client_id(&self) -> u8 {
        self.client_id
    }

    /// Close the socket on the modem.
    pub async fn close(self) -> Result<(), SocketError> {
        let mut modem = self.modem.lock().await;
        modem.ssl_socket_close(self.client_id).await?;
        Ok(())
    }
}

impl<M: RawMutex, W: Write, P: OutputPin> ErrorType for TlsSocket<'_, M, W, P> {
    type Error = SocketError;
}

impl<M: RawMutex, W: Write, P: OutputPin> Write for TlsSocket<'_, M, W, P> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut modem = self.modem.lock().await;
        modem.ssl_socket_send(self.client_id, buf).await?;
        Ok(buf.len())
    }
}

impl<M: RawMutex, W: Write, P: OutputPin> Read for TlsSocket<'_, M, W, P> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        // `read` must block until at least one byte is available. The modem's
        // QSSLRECV returns 0 when its buffer is momentarily empty, so poll with
        // a short back-off. NOTE: peer-close detection (the `+QSSLURC: "closed"`
        // URC) is not yet wired in, so a closed connection blocks rather than
        // returning EOF — a known limitation.
        loop {
            {
                let mut modem = self.modem.lock().await;
                let n = modem.ssl_socket_recv(self.client_id, buf).await?;
                if n > 0 {
                    return Ok(n);
                }
            }
            Timer::after(Duration::from_millis(100)).await;
        }
    }
}
