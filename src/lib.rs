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

use thiserror::Error;

pub mod cellular;
pub mod quectel_atat;

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
}
