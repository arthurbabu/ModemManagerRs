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

/// Async TCP+TLS sockets over the modem, via the `embedded-nal-async` traits.
/// Only available with the `embassy` feature.
#[cfg(feature = "embassy")]
pub mod tcp;

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
