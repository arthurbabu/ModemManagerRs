// Tested with:
// BG95M3LAR02A03_01.012.01.012
// BG95M3LAR02A03_01.200.01.200

// To be tested with:
// BG95M3LAR02A03_01.014.01.014
// BG96MAR02A07M1G_01.018.01.018 (TLS does not work)
// BG95M3LAR02A03_A0.301.A0.301 (seems to work. TLS works)

//! Cellular driver for the Quectel BG9X family.
//!
//! The driver logic is written once in `async` form and runs unchanged on
//! both runtime backends: `std` (hosted OS, driven by any executor -- tokio in
//! the examples) and `embassy` (bare-metal `no_std`, driven by the embassy
//! executor). All timing (delays and timeouts) goes through the private
//! [`compat`] module, which is backed by `embassy-time` in both cases
//! (`embassy-time`'s `std` backend under `std`, its embedded time driver
//! under `embassy`).
//!
//! `QuectelBG9X`'s methods are grouped into capability modules (this file
//! holds only construction/lifecycle; [`power`], [`sim`], [`config`],
//! [`registration`], [`bearer`], [`mqtt`], [`socket`], [`gnss`] and [`file`]
//! each contribute an additional `impl QuectelBG9X` block), mirroring how
//! Linux ModemManager groups modem functionality into separate interfaces.
//! Per-chip AT-command differences (BG95/BG96/EG916U) are factored out into
//! [`crate::chip`] instead of living inline here -- see that module and
//! `CONTRIBUTING.md` for how to add a new chip.

#[cfg(feature = "defmt")]
use defmt::*;

#[cfg(not(feature = "defmt"))]
use log::*;

use embedded_hal::digital::OutputPin;

use crate::quectel_atat::responses::GnssPositionInformationResponse;
use crate::quectel_atat::types::*;
use crate::quectel_atat::urc::Urc;
use crate::quectel_atat::*;

use atat::asynch::{AtatClient, Client};
use embedded_io_async::Write;

use atat::heapless::String as HeaplessString;
use atat::heapless_bytes::Bytes as HeaplessBytes;
use atat::{UrcChannel, UrcSubscription};

use crate::chip::{ActiveChip, ChipProfile, ModemRevision};
use crate::ModemError;

mod bearer;
mod config;
mod digest;
mod file;
mod gnss;
mod mqtt;
mod power;
mod registration;
mod sim;
mod socket;

pub use digest::{ssl_recv_digest_hook, socket_recv_digest_hook, tcp_recv_digest_hook};

/// Runtime timing primitives, backed by `embassy-time` for both runtime
/// features (its `std` backend under `std`, its embedded time driver under
/// `embassy`). `Instant` / `elapsed_ms` provide a monotonic clock for timeout
/// loops that behaves identically in both worlds (elapsed time expressed as
/// whole milliseconds).
mod compat {
    pub use embassy_time::Instant;
    pub use embassy_time::{with_timeout, Duration, Timer};

    pub async fn delay_ms(ms: u64) {
        Timer::after(Duration::from_millis(ms)).await;
    }

    pub async fn delay_secs(secs: u64) {
        Timer::after(Duration::from_secs(secs)).await;
    }

    pub fn elapsed_ms(since: Instant) -> u64 {
        since.elapsed().as_millis()
    }
}

#[derive(Debug)]
pub enum ModemMode {
    EDGE,
    GPRS,
    NBIoT,
    LTEM,
    Unknown,
}

pub const INGRESS_BUF_SIZE: usize = 1024;
pub const URC_CAPACITY: usize = 128;
pub const URC_SUBSCRIBERS: usize = 3;

pub struct QuectelBG9X<W: Write, OutputPinGeneric: OutputPin> {
    pwr_key_pin: OutputPinGeneric,
    client: Client<'static, W, INGRESS_BUF_SIZE>,
    urc_channel: &'static UrcChannel<Urc, URC_CAPACITY, URC_SUBSCRIBERS>,
    imei: [u8; 15],
    mode: ModemMode,
    rev: ModemRevision,
    ssl_configured: bool,
    /// Long-lived URC subscription for the currently-open socket.
    ///
    /// Created fresh when a socket is opened (`tcp_socket_open`/`ssl_socket_open`)
    /// and drained by [`socket_poll_closed`](Self::socket_poll_closed) to detect
    /// `+QIURC`/`+QSSLURC` `"recv"`/`"closed"` events. It must persist across
    /// reads: an embassy `Subscriber` only observes URCs published after it
    /// subscribes, so re-subscribing per poll would miss a `"closed"` that lands
    /// between polls. Only one socket's events are tracked at a time.
    socket_sub: Option<UrcSubscription<'static, Urc, URC_CAPACITY, URC_SUBSCRIBERS>>,
}

impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
    pub async fn new(
        power_gpio: OutputPinGeneric,
        client: Client<'static, W, INGRESS_BUF_SIZE>,
        urc_channel: &'static UrcChannel<Urc, URC_CAPACITY, URC_SUBSCRIBERS>,
    ) -> Result<Self, ModemError> {
        info!("Initializing Quectel BG9X modem (chip: {})", ActiveChip::NAME);

        let mut driver = QuectelBG9X {
            pwr_key_pin: power_gpio,
            client,
            urc_channel,
            imei: [0; 15],
            mode: ModemMode::Unknown,
            rev: ModemRevision::Unknown,
            ssl_configured: false,
            socket_sub: None,
        };

        driver.power_on().await?;
        // driver.disable_echo().await?;  // not necessary in atat
        driver.update_module_revision().await?;
        driver.update_imei().await?;

        Ok(driver)
    }

    pub fn set_mode(&mut self, mode: ModemMode) {
        self.mode = mode;
    }

    async fn send_power_key(&mut self) {
        self.pwr_key_pin.set_high().unwrap();
        compat::delay_ms(500).await;

        self.pwr_key_pin.set_low().unwrap();
    }

    async fn update_module_revision(&mut self) -> Result<(), ModemError> {
        // TODO: this could go to a specific command parser
        // References:
        //   https://docs.rs/atat_derive/latest/atat_derive/derive.AtatCmd.html
        //
        // Right after boot the modem interleaves the RDY / APP RDY URCs, so the
        // first AT+QGMR can time out or come back empty. Retry until we read a
        // real version string.
        const ATTEMPTS: usize = 5;
        for attempt in 0..ATTEMPTS {
            match self.client.send(&GetVersionInfo).await {
                Ok(version) => {
                    let version_code: &[u8] = version.code.as_slice();
                    if version_code.is_empty() {
                        debug!(
                            "Empty modem version (attempt {}/{}), retrying...",
                            attempt + 1,
                            ATTEMPTS
                        );
                        compat::delay_ms(500).await;
                        continue;
                    }

                    let version_str = core::str::from_utf8(version_code).unwrap_or("");
                    info!("Modem version: {}", version_str);

                    self.rev = ActiveChip::classify_revision(version_str);
                    if self.rev == ModemRevision::Unknown {
                        warn!("Unknown modem revision: {}", version_str);
                    }
                    return Ok(());
                }
                Err(e) => {
                    debug!(
                        "AT+QGMR attempt {}/{} failed ({:?}), retrying...",
                        attempt + 1,
                        ATTEMPTS,
                        e
                    );
                    compat::delay_ms(500).await;
                }
            }
        }

        error!("Could not read modem version after {} attempts", ATTEMPTS);
        Err(ModemError::NotResponding)
    }

    async fn update_imei(&mut self) -> Result<(), ModemError> {
        let imei = match self.client.send(&GetImei).await {
            Ok(imei) => imei.imei,
            Err(e) => {
                error!("IMEI not found: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        };

        self.imei.copy_from_slice(imei.as_slice());
        let imei_str = core::str::from_utf8(&self.imei).unwrap();
        info!("IMEI: {}", imei_str);

        Ok(())
    }

    #[allow(dead_code)]
    async fn disable_echo(&mut self) -> Result<(), ModemError> {
        match self.client.send(&SetEcho { on: EchoOn::Off }).await {
            Ok(_) => {
                info!("Echo off");
                Ok(())
            }
            Err(e) => {
                error!("Echo off failed: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Mutable access to the underlying AT client.
    ///
    /// Escape hatch for [`bearer::dial_ppp`](Self::dial_ppp) callers: after a
    /// successful dial, use `self.client_mut().inner()` to reach the writer
    /// wrapped in [`crate::ppp::Reclaimable`] and `.take()` it back out, now
    /// that no further AT commands will be sent on this connection.
    #[cfg(feature = "ppp")]
    pub fn client_mut(&mut self) -> &mut Client<'static, W, INGRESS_BUF_SIZE> {
        &mut self.client
    }

    /// Consume this driver instance and reclaim the power-key GPIO pin and
    /// the AT client.
    ///
    /// For callers that power-cycle the modem and rebuild a fresh
    /// `QuectelBG9X` each session (e.g. [`crate::net::CellularNetwork`]'s
    /// reconnect loop): both must be retained across cycles rather than
    /// dropped along with the rest of the driver state, which this makes
    /// possible. The client's writer is typically empty at this point (see
    /// [`crate::ppp::Reclaimable::take`]) -- refill it via
    /// [`crate::ppp::Reclaimable::put`] on `client.inner()` before reusing
    /// the client for the next `QuectelBG9X::new` call, so the client's
    /// `'static` response-slot/command buffers don't need to be
    /// reallocated each cycle.
    #[cfg(feature = "ppp")]
    pub fn release(self) -> (OutputPinGeneric, Client<'static, W, INGRESS_BUF_SIZE>) {
        (self.pwr_key_pin, self.client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_serving_cell() {
        use crate::quectel_atat::responses::ServingCellInfo;
        use atat::serde_at;
        // The exact raw string from your failing LIMSRV log
        let log_output = b"+QENG: \"servingcell\",\"LIMSRV\",\"LTE\",\"FDD\",208,01,FDC8413,18,9335,28,5,5,F20E,-101,-10,-71,5,23";

        // Attempt to parse it
        let result: Result<ServingCellInfo, _> = serde_at::from_slice(log_output);

        // This will print the detailed internal Serde error (e.g., TypeMismatch, InvalidDigit)
        // and often the exact index where it failed!
        println!("Detailed Parser Result: {:#?}", result);
        result.unwrap();
    }

    /// Regression check for the `compat` module unification: under `std`,
    /// `delay_ms`/`Instant`/`elapsed_ms` are backed by `embassy-time`'s `std`
    /// timer queue (driven on its own thread) rather than `std::thread::sleep`,
    /// but must still behave like a normal delay under any executor (tokio
    /// here).
    #[tokio::test]
    async fn compat_delay_ms_elapses_at_least_the_requested_time() {
        let start = compat::Instant::now();
        compat::delay_ms(50).await;
        assert!(compat::elapsed_ms(start) >= 50);
    }
}
