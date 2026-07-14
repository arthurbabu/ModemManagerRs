#![cfg_attr(not(feature = "std"), no_std)]

// Exactly one runtime backend must be selected. Both are async -- `std` runs
// on a hosted OS under any executor (tokio in the examples), `embassy` runs
// on bare-metal `no_std` under the embassy executor.
#[cfg(all(feature = "std", feature = "embassy"))]
compile_error!(
    "features `std` and `embassy` are mutually exclusive; enable exactly one runtime backend"
);
#[cfg(not(any(feature = "std", feature = "embassy")))]
compile_error!(
    "no runtime backend selected; enable either the `std` (hosted, async) or `embassy` (no_std, async) feature"
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
/// Available under both `std` (drive it from tokio or any other executor) and
/// `embassy`.
pub mod tcp;

#[derive(Debug, Error)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
