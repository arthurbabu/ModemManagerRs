#![cfg_attr(not(feature = "std"), no_std)]

// Exactly one runtime backend must be selected.
#[cfg(all(feature = "std", feature = "embassy"))]
compile_error!(
    "features `std` and `embassy` are mutually exclusive; enable exactly one runtime backend"
);
#[cfg(not(any(feature = "std", feature = "embassy")))]
compile_error!(
    "no runtime backend selected; enable either the `std` (blocking) or `embassy` (async) feature"
);

// Exactly one chip must be selected.
#[cfg(not(any(feature = "bg95", feature = "bg96", feature = "eg916u")))]
compile_error!("no chip selected; enable exactly one of `bg95`, `bg96` or `eg916u`");
#[cfg(any(
    all(feature = "bg95", feature = "bg96"),
    all(feature = "bg95", feature = "eg916u"),
    all(feature = "bg96", feature = "eg916u")
))]
compile_error!(
    "features `bg95`, `bg96` and `eg916u` are mutually exclusive; enable exactly one chip"
);

use thiserror::Error;

pub mod cellular;
pub mod quectel_atat;

/// Which transport a modem-backed socket uses.
///
/// Passed to the socket wrappers ([`tcp`] under `embassy`, [`tcp_std`] under
/// `std`) to select between the modem's plain-TCP and TLS engines per
/// connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    /// Plain TCP (`AT+QIOPEN` / `QISEND` / `QIRD` / `QICLOSE`).
    Tcp,
    /// Modem-terminated TLS on a pre-configured SSL context
    /// (`AT+QSSLOPEN` / `QSSLSEND` / `QSSLRECV` / `QSSLCLOSE`). The context
    /// `ssl_ctx_id` must already be set up via
    /// [`cellular::QuectelBG9X::configure_ssl_context`].
    Tls {
        /// SSL context id (0-5) configured via `AT+QSSLCFG`.
        ssl_ctx_id: u8,
    },
}

/// Async TCP / TLS sockets over the modem, via the `embedded-nal-async` traits.
/// Only available with the `embassy` feature.
#[cfg(feature = "embassy")]
pub mod tcp;

/// Blocking TCP / TLS sockets over the modem, exposing `std::io` and
/// `embedded-io` / `embedded-nal` (blocking) interfaces. Only available with the
/// `std` feature.
#[cfg(feature = "std")]
pub mod tcp_std;

#[derive(Debug, Error)]
pub enum ModemError {
    #[error("Modem not responding")]
    NotResponding,
    #[error("SIM failure")]
    SimError,
    #[error("SIM unknown")]
    SimErrorUnknown,
    #[error("Not attached to network")]
    NoNetwork,
    #[error("No active context")]
    NoContext,
    #[error("MQTT error")]
    MqttRequestFailed,
    #[error("MQTT request failed")]
    NtpRequestFailed,
    #[error("NTP response not valid")]
    NotSupported,
    #[error("Request timeout")]
    OperationTimeout,
    #[error("GNSS position not fixed")]
    GnssNotFixed,
    #[error("File upload failed")]
    FileUploadFailed,
    #[error("File deletion failed")]
    FileDeletionFailed,
    #[error("SSL certificate not found")]
    SslCertificateNotFound,
    #[error("SSL certificate invalid")]
    SslCertificateInvalid,
    #[error("SSL hostname mismatch")]
    SslHostnameMismatch,
    #[error("SSL cipher negotiation failed")]
    SslCipherNegotiationFailed,
    #[error("Socket open failed")]
    SocketOpenFailed,
    #[error("Socket send failed")]
    SocketSendFailed,
    #[error("Socket receive failed")]
    SocketRecvFailed,
    #[error("Socket close failed")]
    SocketCloseFailed,
}
