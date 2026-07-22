//! Per-chip "plugin" layer: one [`ChipProfile`] implementation per supported
//! Quectel chip (BG95, BG96, EG916U), selected at compile time via Cargo
//! feature and resolved through the crate-internal [`ActiveChip`] alias.
//!
//! This is the `no_std`/static-dispatch analogue of Linux ModemManager's
//! per-vendor plugin: MM resolves "which modem is this" at runtime via
//! D-Bus/GObject interface probing across a dynamically discovered set of
//! devices; here it's resolved at compile time (a single binary only ever
//! targets one chip), so a plain trait implemented once per chip and
//! selected via `#[cfg(feature = "...")]` type aliases does the same job
//! without needing dynamic dispatch or an allocator.
//!
//! See `CONTRIBUTING.md` for a full walkthrough of adding a new chip.

use crate::cellular::{ModemMode, INGRESS_BUF_SIZE};
use crate::quectel_atat::types::ModemConfiguration;
use crate::ModemError;
use atat::asynch::Client;
use embedded_io_async::Write;

#[cfg(any(feature = "bg95", feature = "bg96"))]
mod classic;

#[cfg(feature = "bg95")]
mod bg95;
#[cfg(feature = "bg96")]
mod bg96;
#[cfg(feature = "eg916u")]
mod eg916u;

#[cfg(feature = "bg95")]
pub(crate) type ActiveChip = bg95::Bg95;
#[cfg(feature = "bg96")]
pub(crate) type ActiveChip = bg96::Bg96;
#[cfg(feature = "eg916u")]
pub(crate) type ActiveChip = eg916u::Eg916u;

/// Firmware/chip revision, used for revision-specific quirks (see
/// [`ChipProfile::needs_explicit_mqtt_close`]).
///
/// Only one chip's variants are ever constructed in a given build (chip
/// selection is a compile-time Cargo feature), so the other chips' variants
/// are legitimately unconstructed dead code in that build -- not a bug.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) enum ModemRevision {
    R200,
    R018,
    R014,
    R012,
    /// Quectel EG916U (LTE Cat 1bis), e.g. "EG916QGLLGR01A05M04_...".
    Eg916u,
    Unknown,
}

/// One implementation per supported chip -- the crate-internal equivalent of
/// a ModemManager vendor plugin. `QuectelBG9X` calls through the
/// [`ActiveChip`] alias instead of branching on `#[cfg(feature = "...")]`
/// inline; adding a new chip means adding a new impl of this trait, not
/// touching the driver's control flow.
///
/// Methods are plain `async fn` (no boxing / `async-trait`): safe here
/// because this trait is only ever used through the concrete `ActiveChip`
/// alias (static dispatch, never `dyn ChipProfile`), so it stays alloc-free
/// and `no_std`-compatible.
pub(crate) trait ChipProfile {
    /// Human-readable chip name, for logging.
    const NAME: &'static str;
    /// All-bands bitmask for `EmtcBands::Any` (see
    /// [`crate::quectel_atat::types::Band`]).
    const EMTC_ALL_BANDS_MASK: u128;
    /// All-bands bitmask for `NbIotBands::Any`.
    const NB_ALL_BANDS_MASK: u128;

    /// Send whatever AT commands configure bands / RAT search order /
    /// service domain for this chip family.
    async fn configure_modem<W: Write>(
        client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
        config: &ModemConfiguration,
    ) -> Result<(), ModemError>;

    /// One network-attach polling attempt. `Ok(None)` means "keep polling",
    /// `Ok(Some(mode))` means attached with the given radio technology.
    async fn poll_attach_status<W: Write>(
        client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
    ) -> Result<Option<ModemMode>, ModemError>;

    /// Classify an `AT+QGMR` firmware-version reply into a [`ModemRevision`].
    fn classify_revision(version: &str) -> ModemRevision;

    /// R200 BG95 firmware quirk: `AT+QMTDISC` alone isn't reliable, so the
    /// driver must also wait for the `MqttStatus` URC and send an explicit
    /// `AT+QMTCLO`. Default: not needed.
    fn needs_explicit_mqtt_close(_rev: ModemRevision) -> bool {
        false
    }
}
