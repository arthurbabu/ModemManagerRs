// Tested with:
// BG95M3LAR02A03_01.012.01.012
// BG95M3LAR02A03_01.200.01.200

// To be tested with:
// BG95M3LAR02A03_01.014.01.014
// BG96MAR02A07M1G_01.018.01.018 (TLS does not work)
// BG95M3LAR02A03_A0.301.A0.301 (seems to work. TLS works)

//! Cellular driver for the Quectel BG9X family.
//!
//! The driver logic is written once in `async` form. The [`maybe_async_cfg`]
//! macro generates a **blocking** implementation when the `std` feature is
//! enabled and an **async** implementation when the `embassy` feature is
//! enabled. All timing (delays and timeouts) goes through the private
//! [`compat`] module, which is backed by `std::thread`/`std::time` for `std`
//! and by `embassy-time` for `embassy`.

use log::*;

use embedded_hal::digital::OutputPin;
use time;

use crate::quectel_atat::responses::GnssPositionInformationResponse;
use crate::quectel_atat::types::*;
use crate::quectel_atat::urc::Urc;
use crate::quectel_atat::*;

// The AT client and the `Write` bound differ between runtimes: the blocking
// runtime uses `embedded_io::Write`, the async runtime uses
// `embedded_io_async::Write`. Selecting them here keeps the rest of the module
// runtime-agnostic (the identifiers `Client`, `AtatClient` and `Write` resolve
// to the right types for the active feature).
#[cfg(feature = "std")]
use atat::blocking::{AtatClient, Client};
#[cfg(feature = "std")]
use embedded_io::Write;

#[cfg(feature = "embassy")]
use atat::asynch::{AtatClient, Client};
#[cfg(feature = "embassy")]
use embedded_io_async::Write;

use atat::heapless::String as HeaplessString;
use atat::heapless_bytes::Bytes as HeaplessBytes;
use atat::UrcChannel;

use crate::ModemError;

/// Runtime timing primitives, selected by the active feature.
///
/// `delay_ms` / `delay_secs` are `async` under `embassy` and blocking under
/// `std`; the `maybe_async_cfg`-generated code awaits them in the async variant
/// and calls them directly in the blocking variant. `Instant` / `elapsed_ms`
/// provide a monotonic clock for timeout loops that behaves identically in both
/// worlds (elapsed time expressed as whole milliseconds).
#[cfg(feature = "std")]
mod compat {
    pub use std::time::Instant;

    pub fn delay_ms(ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    pub fn delay_secs(secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }

    pub fn elapsed_ms(since: Instant) -> u64 {
        since.elapsed().as_millis() as u64
    }
}

#[cfg(feature = "embassy")]
mod compat {
    pub use embassy_time::Instant;
    use embassy_time::{Duration, Timer};

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

#[derive(Debug, PartialEq)]
enum ModemRevision {
    R200,
    R018,
    R014,
    R012,
    /// Quectel EG916U (LTE Cat 1bis), e.g. "EG916QGLLGR01A05M04_...".
    Eg916u,
    Unknown,
}

pub const INGRESS_BUF_SIZE: usize = 1024;
pub const URC_CAPACITY: usize = 128;
pub const URC_SUBSCRIBERS: usize = 3;

/// Custom-success digester hook for the binary `+QSSLRECV` data frame.
///
/// atat's `DefaultDigester` is line/token oriented: it frames a response by
/// scanning for `\r\nOK\r\n`, a `>`/`@` prompt, or an error token. That works
/// for ordinary AT replies, but the `AT+QSSLRECV` response is
/// `+QSSLRECV: <len>\r\n<len raw bytes>\r\n\r\nOK\r\n` where `<len raw bytes>`
/// is **arbitrary binary** (HTML, JS, TLS-decrypted payload…). While such a
/// frame is still streaming into the ingress buffer (before its trailing
/// `\r\nOK\r\n` has arrived) the default digester's `take_until("\r\nOK\r\n")`
/// fails as a hard no-match and control falls through to the generic prompt
/// parser, which treats a lone `>` (extremely common in HTML/JS) followed by
/// end-of-buffer as a data prompt. That mis-consumes bytes and corrupts the
/// frame, surfacing as `QSSLRECV failed: InvalidResponse`.
///
/// This hook is tried *before* the generic success/prompt/error parsers (see
/// `AtDigester::digest`). It recognises the length prefix and consumes exactly
/// `<len>` payload bytes plus the terminator, returning [`ParseError::Incomplete`]
/// (so the digester waits for more bytes instead of running the fragile generic
/// parsers) until the whole frame is buffered. For any response that is not a
/// `+QSSLRECV` data frame it returns [`ParseError::NoMatch`], leaving every
/// other command on the default parsers.
///
/// Wire it into the ingress digester in place of a bare `DefaultDigester`:
/// ```ignore
/// let digester = DefaultDigester::<Urc>::default()
///     .with_custom_success(ssl_recv_digest_hook);
/// let ingress = Ingress::new(digester, buf, &RES_SLOT, &URC_CHANNEL);
/// ```
pub fn ssl_recv_digest_hook(buf: &[u8]) -> Result<(&[u8], usize), atat::digest::ParseError> {
    use atat::digest::ParseError;

    const HDR: &[u8] = b"+QSSLRECV: ";

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || hay.len() < needle.len() {
            return None;
        }
        hay.windows(needle.len()).position(|w| w == needle)
    }

    // Locate the header. Note `+QSSLRECV: ` (colon + space) does not match the
    // command echo `AT+QSSLRECV=…` (equals), so a stray echo is ignored. If the
    // header is absent this is not a QSSLRECV data frame — defer to the default
    // parsers.
    let hdr = find(buf, HDR).ok_or(ParseError::NoMatch)?;
    let after_hdr = &buf[hdr + HDR.len()..];

    // Decimal length runs up to the first CRLF. Header seen but length not yet
    // terminated => wait for more.
    let nl = find(after_hdr, b"\r\n").ok_or(ParseError::Incomplete)?;
    let len = core::str::from_utf8(&after_hdr[..nl])
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .ok_or(ParseError::NoMatch)?;

    // Payload starts right after that CRLF and is exactly `len` raw bytes.
    let payload_start = hdr + HDR.len() + nl + 2;
    let payload_end = payload_start + len;
    if buf.len() < payload_end {
        return Err(ParseError::Incomplete);
    }

    // The terminating `OK\r\n` follows the payload (after an intervening CRLF).
    // Search only *after* the binary payload so payload bytes can't spoof it.
    let ok = find(&buf[payload_end..], b"OK\r\n").ok_or(ParseError::Incomplete)?;
    let consumed = payload_end + ok + b"OK\r\n".len();

    // Hand `SslRecv::parse` the header + length + payload; it re-locates the
    // header itself, so a leading CRLF is harmless.
    Ok((&buf[hdr..payload_end], consumed))
}

// Cat-M / NB-IoT specific (AT+QCFG="iotopmode"); not applicable to EG916U.
#[cfg(not(feature = "eg916u"))]
fn get_iotop_mode(configuration: ModemConfiguration) -> Result<u8, ModemError> {
    let rat = configuration.get_rat_order();
    let rat_order = rat.as_str();

    match (rat_order.contains("02"), rat_order.contains("03")) {
        (true, false) => Ok(0), // only EMTC
        (false, true) => Ok(1), // only NB-IoT
        (true, true) => Ok(2),  // both
        (false, false) => Err(ModemError::NotSupported),
    }
}

pub struct QuectelBG9X<W: Write, OutputPinGeneric: OutputPin> {
    pwr_key_pin: OutputPinGeneric,
    client: Client<'static, W, INGRESS_BUF_SIZE>,
    urc_channel: &'static UrcChannel<Urc, URC_CAPACITY, URC_SUBSCRIBERS>,
    imei: [u8; 15],
    mode: ModemMode,
    rev: ModemRevision,
    ssl_configured: bool,
}

#[maybe_async_cfg::maybe(keep_self, sync(feature = "std"), async(feature = "embassy"))]
impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
    pub async fn new(
        power_gpio: OutputPinGeneric,
        client: Client<'static, W, INGRESS_BUF_SIZE>,
        urc_channel: &'static UrcChannel<Urc, URC_CAPACITY, URC_SUBSCRIBERS>,
    ) -> Result<Self, ModemError> {
        log::info!("Initializing Quectel BG9X modem");

        let mut driver = QuectelBG9X {
            pwr_key_pin: power_gpio,
            client,
            urc_channel,
            imei: [0; 15],
            mode: ModemMode::Unknown,
            rev: ModemRevision::Unknown,
            ssl_configured: false,
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
                        log::debug!(
                            "Empty modem version (attempt {}/{}), retrying...",
                            attempt + 1,
                            ATTEMPTS
                        );
                        compat::delay_ms(500).await;
                        continue;
                    }

                    let version_str = core::str::from_utf8(version_code).unwrap_or("");
                    log::info!("Modem version: {}", version_str);

                    self.rev = match version_str {
                        s if s.contains("BG95M3LAR02A03_01.200.01.200") => ModemRevision::R200,
                        s if s.contains("BG96MAR02A07M1G_01.018.00.000") => ModemRevision::R018,
                        s if s.contains("BG96MAR02A07M1G_01.018.01.018") => ModemRevision::R018,
                        s if s.contains("BG95M3LAR02A03_01.014.01.014") => ModemRevision::R014,
                        s if s.contains("BG95M3LAR02A03_01.012.01.012") => ModemRevision::R012,
                        s if s.contains("EG916") => ModemRevision::Eg916u,
                        _ => {
                            log::warn!("Unknown modem revision: {}", version_str);
                            ModemRevision::Unknown
                        }
                    };
                    return Ok(());
                }
                Err(e) => {
                    log::debug!(
                        "AT+QGMR attempt {}/{} failed ({:?}), retrying...",
                        attempt + 1,
                        ATTEMPTS,
                        e
                    );
                    compat::delay_ms(500).await;
                }
            }
        }

        log::error!("Could not read modem version after {} attempts", ATTEMPTS);
        Err(ModemError::NotResponding)
    }

    async fn update_imei(&mut self) -> Result<(), ModemError> {
        let imei = match self.client.send(&GetImei).await {
            Ok(imei) => imei.imei,
            Err(e) => {
                log::error!("IMEI not found: {:?}", e);
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
                log::info!("Echo off");
                Ok(())
            }
            Err(e) => {
                log::error!("Echo off failed: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    async fn is_powered_on(&mut self) -> Result<(), ModemError> {
        // TODO: deal with the start URC, that has this format:
        //pub const CMD_POWER_ON_RES: &[u8] = b"\r\nRDY\r\n\r\nAPP RDY\r\n";
        // For now, just wait enought time for the modem to power on.
        compat::delay_secs(5).await;

        // Probe with AT until the modem answers. Right after boot it interleaves
        // RDY / APP RDY URCs, so the first few commands can time out.
        let mut alive = false;
        for _ in 0..3 {
            log::info!("Sending AT command");
            match self.client.send(&AT).await {
                Ok(_) => {
                    log::info!("Response Ok");
                    alive = true;
                    break;
                }
                Err(e) => {
                    log::error!("Response failed: {:?}", e);
                }
            }
            compat::delay_secs(5).await;
        }

        if !alive {
            return Err(ModemError::NotResponding);
        }

        // Disable command echo (ATE0). This MUST be off: with echo on the modem
        // echoes back raw payloads (e.g. the QSSLSEND body), which then get
        // interleaved into the next command's response and corrupt parsing
        // (notably QSSLRECV -> InvalidResponse). Do it only once the modem is
        // confirmed alive, and retry since a single ATE0 can still race the boot
        // URCs.
        for _ in 0..3 {
            match self.client.send(&SetEcho { on: EchoOn::Off }).await {
                Ok(_) => {
                    log::info!("Echo off");
                    return Ok(());
                }
                Err(e) => {
                    log::error!("Echo off failed: {:?}", e);
                }
            }
            compat::delay_ms(500).await;
        }

        // Echo could not be turned off; the modem is still responsive but data
        // socket parsing will be unreliable. Surface it rather than silently
        // continuing.
        log::error!("Could not disable echo after modem power-on");
        Err(ModemError::NotResponding)
    }

    async fn is_powered_off(&mut self) -> Result<(), ModemError> {
        let mut subscriber = self.urc_channel.subscribe().unwrap();

        match self
            .client
            .send(&PowerDown {
                mode: PowerDownMode::Normal,
            })
            .await
        {
            Ok(_) => {
                log::info!("Modem powering down");
            }
            Err(e) => {
                log::error!("Modem not powered down: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        for _ in 0..10 {
            match subscriber.try_next_message_pure() {
                Some(Urc::PowerDown) => {
                    break;
                }
                _ => {
                    compat::delay_secs(1).await;
                }
            }
        }

        // Double check if the modem is powered down
        match self.client.send(&AT).await {
            Ok(_) => Err(ModemError::OperationTimeout),
            Err(_) => {
                log::info!("Modem powered down");
                Ok(())
            }
        }
    }

    pub async fn is_alive(&mut self) -> Result<(), ModemError> {
        match self.client.send(&AT).await {
            Ok(_) => {
                log::info!("Modem alive");
                Ok(())
            }
            Err(e) => {
                log::error!("Modem not alive: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    pub async fn power_on(&mut self) -> Result<(), ModemError> {
        self.send_power_key().await;
        self.is_powered_on().await?;

        Ok(())
    }

    pub async fn power_off(&mut self) -> Result<(), ModemError> {
        self.is_powered_off().await?;

        Ok(())
    }

    pub async fn test_sim(&mut self) -> Result<(), ModemError> {
        // Right after power-on the SIM can transiently report "SIM failure"
        // (CME 13) or "SIM busy" (CME 14) while it initialises, so poll
        // `AT+CPIN?` a few times before declaring failure.
        //
        // Note: when the SIM is not READY the modem replies with
        // `+CME ERROR: <n>`, which atat routes to the URC channel (it is a
        // registered URC), so the `AT+CPIN?` command itself times out and we
        // must inspect the URC to classify the error.
        const ATTEMPTS: usize = 5;
        let mut subscriber = self.urc_channel.subscribe().unwrap();

        for attempt in 0..ATTEMPTS {
            match self.client.send(&GetSimStatus).await {
                Ok(status) => {
                    log::info!("SIM status: {:?}", status);
                    if status.code.contains("READY") {
                        log::info!("SIM Ready");
                        if let Ok(res) = self.client.send(&GetIccid {}).await {
                            log::info!("ICCID: {:?}", res.iccid);
                        }
                        return Ok(());
                    } else if status.code.contains("SIM PIN") {
                        log::error!("SIM PIN required");
                        return Err(ModemError::SimError);
                    }
                }
                Err(e) => {
                    log::debug!(
                        "AT+CPIN? attempt {} returned no direct response ({:?})",
                        attempt + 1,
                        e
                    );
                }
            }

            // Drain URCs looking for a CME error to classify.
            compat::delay_ms(500).await;
            while let Some(urc) = subscriber.try_next_message_pure() {
                if let Urc::CmeError(cme_error) = urc {
                    match cme_error.err {
                        10 => {
                            log::error!("SIM not inserted");
                            return Err(ModemError::SimError);
                        }
                        11 => {
                            log::error!("SIM PIN required");
                            return Err(ModemError::SimError);
                        }
                        // 13 (SIM failure) and 14 (SIM busy) are commonly
                        // transient during SIM init: keep retrying.
                        13 | 14 => {
                            log::info!("SIM not ready yet (CME {}), retrying...", cme_error.err);
                        }
                        other => {
                            log::warn!("Unhandled SIM CME error {}, retrying...", other);
                        }
                    }
                }
            }

            compat::delay_ms(1000).await;
        }

        log::error!("SIM not ready after {} attempts", ATTEMPTS);
        Err(ModemError::SimErrorUnknown)
    }

    pub async fn set_modem_funcionality(&mut self, on: bool) -> Result<(), ModemError> {
        match self
            .client
            .send(&SetUeFunctionality {
                fun: match on {
                    true => FunctionalityLevelOfUE::Full,
                    false => FunctionalityLevelOfUE::Minimum,
                },
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("Modem functionality not set: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Factory reset the modem
    ///
    /// This function should not be called very often because it erases the internal flash memory.
    pub async fn factory_reset(&mut self) -> Result<(), ModemError> {
        warn!("Factory Reset. This function should not be called very often.");

        match self.client.send(&ResetToFactoryDefault {}).await {
            Ok(_) => {}
            Err(e) => {
                log::error!("Factory reset not set: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        match self.client.send(&RestoreFactoryConfiguration {}).await {
            Ok(_) => {
                // Factory reset clears SSL configuration on the modem
                self.reset_ssl_context();
                Ok(())
            }
            Err(e) => {
                log::error!("Restore configuration failed: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Reset SSL context configuration state.
    ///
    /// This method clears the internal SSL configuration flag, allowing the SSL context
    /// to be reconfigured on the next MQTT connection. It does not send any AT commands
    /// to the modem - it only updates the internal state tracking.
    ///
    /// This should be called after factory reset or when you want to force SSL
    /// reconfiguration on the next connection.
    pub fn reset_ssl_context(&mut self) {
        info!("Resetting SSL context state");
        self.ssl_configured = false;
    }

    pub async fn set_modem_configuration(
        &mut self,
        configuration: ModemConfiguration,
    ) -> Result<(), ModemError> {
        #[cfg(feature = "eg916u")]
        {
            // EG916U (LTE Cat 1bis + GSM): the BG-family QCFG="band" (three
            // per-RAT masks), "iotopmode" and "nwscanseq" layout does NOT apply
            // to this chip and would be rejected, so we don't send them. Band /
            // RAT selection is left at the modem default (automatic); we only
            // pin the service domain to Packet-Switched for data, best-effort.
            //
            // TODO(eg916u): once the EG916U AT manual is available, set
            // AT+QCFG="band" with the LTE band layout and the correct
            // "nwscanseq" RAT codes here instead of relying on defaults.
            let _ = &configuration;

            if let Err(e) = self
                .client
                .send(&ConfigureServiceDomain {
                    param: HeaplessString::try_from("servicedomain").unwrap(),
                    service_domain: 1, // PS: Packet Switched
                    effect: ConfigurationEffect::Immediately,
                })
                .await
            {
                // Not fatal on EG916U: fall back to the modem default.
                log::warn!(
                    "EG916U: could not set service domain ({:?}); using default",
                    e
                );
            }

            log::info!("EG916U modem configuration set (bands left at modem default)");
            return Ok(());
        }

        #[cfg(not(feature = "eg916u"))]
        {
            match self
                .client
                .send(&ConfigureBands {
                    param: HeaplessString::try_from("band").unwrap(),
                    gsm_band_mask: HeaplessBytes::try_from(
                        configuration
                            .get_band_string(RadioAccessTechnology::GSM)
                            .unwrap()
                            .as_bytes(),
                    )
                    .unwrap(),
                    emtc_band_mask: HeaplessBytes::try_from(
                        configuration
                            .get_band_string(RadioAccessTechnology::EMTC)
                            .unwrap()
                            .as_bytes(),
                    )
                    .unwrap(),
                    nbiot_band_mask: HeaplessBytes::try_from(
                        configuration
                            .get_band_string(RadioAccessTechnology::NbIoT)
                            .unwrap()
                            .as_bytes(),
                    )
                    .unwrap(),
                    effect: ConfigurationEffect::Immediately,
                })
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    log::error!("Modem configuration not set: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            }

            match self
                .client
                .send(&ConfigureRatSearchingSequence {
                    param: HeaplessString::try_from("nwscanseq").unwrap(),
                    rat_searching_sequence: HeaplessBytes::try_from(
                        configuration.get_rat_order().as_bytes(),
                    )
                    .unwrap(),
                    effect: ConfigurationEffect::Immediately,
                })
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    log::error!("Modem configuration not set: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            };

            match self
                .client
                .send(&ConfigureRatSearchingMode {
                    param: HeaplessString::try_from("nwscanmode").unwrap(),
                    rat_searching_mode: 0, // Automatic: GSM and LTE
                    effect: ConfigurationEffect::Immediately,
                })
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    log::error!("Modem configuration not set: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            };

            match self
                .client
                .send(&ConfigureServiceDomain {
                    param: HeaplessString::try_from("servicedomain").unwrap(),
                    service_domain: 1, // PS: Packet Switched
                    effect: ConfigurationEffect::Immediately,
                })
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    log::error!("Modem configuration not set: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            };

            match self
                .client
                .send(&ConfigureIotOpMode {
                    param: HeaplessString::try_from("iotopmode").unwrap(),
                    mode: get_iotop_mode(configuration)?,
                    effect: ConfigurationEffect::Immediately,
                })
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    log::error!("Modem configuration not set: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            };

            log::info!("Modem configuration set");
            Ok(())
        }
    }

    pub async fn get_nitz_time(&mut self) -> Result<i64, ModemError> {
        match self.client.send(&GetNetworkNitzTime { mode: 1 }).await {
            Ok(network_time_info) => {
                log::info!("Network time: {:?}", network_time_info);
                get_timestamp_from_nitz_response(&network_time_info.time_and_dst)
            }
            Err(e) => {
                log::error!("Network time not found: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    pub async fn get_ntp_time(&mut self, ntp_server: &str) -> Result<i64, ModemError> {
        match self
            .client
            .send(&GetNetworkNtpTime {
                context_id: 1,
                server: HeaplessString::try_from(ntp_server).unwrap(),
            })
            .await
        {
            Ok(_) => {
                log::info!("NTP request sent");
            }
            Err(e) => {
                log::error!("Network time not found: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        let mut subscriber = self.urc_channel.subscribe().unwrap();
        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 10_000 {
            compat::delay_ms(500).await;

            match subscriber.try_next_message_pure() {
                Some(Urc::NtpTime(res)) => {
                    match res.err {
                        0 => {}
                        _ => {
                            log::error!("NTP failed");
                            return Err(ModemError::NtpRequestFailed);
                        }
                    }

                    log::info!("Network time: {:?}", res.time);
                    return get_timestamp_from_ntp_response(&res.time);
                }
                Some(e) => {
                    log::error!("Unknown URC {:?}", e);
                }
                None => {
                    log::debug!("Waiting for response...");
                }
            }
        }

        Err(ModemError::NotResponding)
    }

    /// Synchronise the clock over NTP and return it as a [`chrono::DateTime<Utc>`].
    ///
    /// Convenience wrapper over [`get_ntp_time`](Self::get_ntp_time): it issues
    /// the same `AT+QNTP` request (a PDP context must be active) and converts the
    /// resulting Unix timestamp into a `chrono` UTC datetime.
    ///
    /// ```ignore
    /// let now = mm.get_ntp_datetime("0.pool.ntp.org").await?;
    /// log::info!("UTC now: {}", now); // e.g. 2026-07-12 13:43:47 UTC
    /// ```
    pub async fn get_ntp_datetime(
        &mut self,
        ntp_server: &str,
    ) -> Result<chrono::DateTime<chrono::Utc>, ModemError> {
        let ts = self.get_ntp_time(ntp_server).await?;
        chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0).ok_or(ModemError::NtpRequestFailed)
    }

    pub async fn get_signal_strength(&mut self) -> Result<(i16, u8), ModemError> {
        match self.client.send(&GetSignalStrength).await {
            Ok(signal_strength) => {
                if let Some(rssi) = signal_strength.rssi {
                    let signal = rssi.clamp(-140, -30);
                    let signal = -100 * (signal + 140) / (-140 + 30);
                    log::info!("RSSI: {}dB ({}%)", rssi, signal);
                    Ok((rssi, signal as u8))
                } else {
                    Err(ModemError::NoNetwork)
                }
            }
            Err(e) => {
                log::error!("Signal strength not found: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    async fn register_gprs(
        &mut self,
        timeout: core::time::Duration,
    ) -> Result<core::time::Duration, ModemError> {
        let now = compat::Instant::now();

        while compat::elapsed_ms(now) < timeout.as_millis() as u64 {
            compat::delay_ms(500).await;

            match self
                .client
                .send(&GetEGPRSNetworkRegistrationStatus {})
                .await
            {
                Ok(status) => {
                    log::info!("GPRS network registration status: {:?}", status);
                    match status.stat {
                        1 => {
                            let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                            log::info!("Registered (Home) after {} s", t.as_secs());
                            return Ok(t);
                        }
                        2 => {
                            log::debug!("Searching..."); // Searching
                            continue;
                        }
                        3 => {
                            log::error!("Registration denied");
                            return Err(ModemError::NoNetwork);
                        }
                        4 => {
                            log::error!("Registration failed");
                            return Err(ModemError::NoNetwork);
                        }
                        5 => {
                            let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                            log::info!("Registered (Roaming) after {} s", t.as_secs());
                            return Ok(t);
                        }
                        _ => {
                            log::error!("Unknown registration status");
                            return Err(ModemError::NoNetwork);
                        }
                    }
                }
                Err(e) => {
                    log::error!("GPRS network registration status not found: {:?}", e);
                }
            }
        }

        Err(ModemError::OperationTimeout)
    }

    async fn register_eps(
        &mut self,
        timeout: core::time::Duration,
    ) -> Result<core::time::Duration, ModemError> {
        let now = compat::Instant::now();

        while compat::elapsed_ms(now) < timeout.as_millis() as u64 {
            compat::delay_ms(500).await;

            match self.client.send(&GetEPSNetworkRegistrationStatus {}).await {
                Ok(status) => {
                    log::info!("EPS network registration status: {:?}", status);
                    match status.stat {
                        1 => {
                            let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                            log::info!("Registered (Home) after {} s", t.as_secs());
                            return Ok(t);
                        }
                        2 => {
                            log::debug!("Searching..."); // Searching
                            continue;
                        }
                        3 => {
                            log::error!("Registration denied");
                            return Err(ModemError::NoNetwork);
                        }
                        4 => {
                            log::error!("Registration failed");
                            return Err(ModemError::NoNetwork);
                        }
                        5 => {
                            let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                            log::info!("Registered (Roaming) after {} s", t.as_secs());
                            return Ok(t);
                        }
                        _ => {
                            log::error!("Unknown registration status");
                            return Err(ModemError::NoNetwork);
                        }
                    }
                }
                Err(e) => {
                    log::error!("EPS network registration status not found: {:?}", e);
                }
            }
        }

        Err(ModemError::OperationTimeout)
    }

    async fn network_attach_info(
        &mut self,
        timeout: core::time::Duration,
    ) -> Result<core::time::Duration, ModemError> {
        let now = compat::Instant::now();

        info!("Attaching...");
        while compat::elapsed_ms(now) < timeout.as_millis() as u64 {
            compat::delay_ms(1000).await;

            #[cfg(not(feature = "eg916u"))]
            {
                match self.client.send(&GetNetworkInfo).await {
                    Ok(info) => {
                        log::info!("Network info: {:?}", info);

                        let act = info.act.as_str();

                        // 2. Handle "SEARCH" or "No Service"
                        if act == "SEARCH" || act.contains("No Service") {
                            log::debug!("Searching...");
                            continue;
                        }

                        // 3. Map the technology strings
                        match act {
                            a if a.contains("LTE") => {
                                self.mode = ModemMode::LTEM;
                                let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                                log::info!("Using LTE after {} s", t.as_secs());
                                return Ok(t);
                            }
                            a if a.contains("GSM") || a.contains("GPRS") || a.contains("EDGE") => {
                                self.mode = ModemMode::EDGE;
                                let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                                log::info!("Using 2G after {} s", t.as_secs());
                                return Ok(t);
                            }
                            a if a.contains("NBIoT") => {
                                self.mode = ModemMode::NBIoT;
                                let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                                log::info!("Using NB-IoT after {} s", t.as_secs());
                                return Ok(t);
                            }
                            _ => {
                                log::warn!("Unknown or unstable technology: {}", act);
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Network info error: {:?}", e);
                    }
                }
            }

            #[cfg(feature = "eg916u")]
            {
                match self.client.send(&GetCopsInfo).await {
                    Ok(info) => {
                        log::info!("Network info: {:?}", info);

                        // 1. Normalize the integer technology code into a string
                        // 0,3 = GSM/2G | 7,8 = LTE/Cat-M1 | 9 = NB-IoT
                        let act = match info.act {
                            Some(7) | Some(8) => "LTE",
                            Some(9) => "NBIoT",
                            Some(0) | Some(3) => "GSM",
                            _ => "SEARCH",
                        };

                        // 2. Handle "SEARCH" or "No Service"
                        if act == "SEARCH" {
                            log::debug!("Searching...");
                            continue;
                        }

                        // 3. Map the technology strings
                        match act {
                            "LTE" => {
                                self.mode = ModemMode::LTEM;
                                let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                                log::info!("Using LTE after {} s", t.as_secs());
                                return Ok(t);
                            }
                            "GSM" => {
                                self.mode = ModemMode::EDGE;
                                let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                                log::info!("Using 2G after {} s", t.as_secs());
                                return Ok(t);
                            }
                            "NBIoT" => {
                                self.mode = ModemMode::NBIoT;
                                let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                                log::info!("Using NB-IoT after {} s", t.as_secs());
                                return Ok(t);
                            }
                            _ => {
                                log::warn!("Unknown or unstable technology code: {}", act);
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Network info error: {:?}", e);
                    }
                }
            }
        }

        Err(ModemError::OperationTimeout)
    }

    pub async fn network_attach(&mut self) -> Result<core::time::Duration, ModemError> {
        let timeout = core::time::Duration::from_secs(60);

        let attach_duration = self.network_attach_info(timeout).await?;
        let reg_duration = match self.mode {
            ModemMode::EDGE => self.register_gprs(timeout - attach_duration).await,
            ModemMode::GPRS => self.register_gprs(timeout - attach_duration).await,
            ModemMode::LTEM => self.register_eps(timeout - attach_duration).await,
            ModemMode::NBIoT => self.register_eps(timeout - attach_duration).await,
            ModemMode::Unknown => Err(ModemError::NoNetwork),
        }?;

        Ok(attach_duration + reg_duration)
    }

    pub async fn set_context_configuration(
        &mut self,
        modem_apn: &str,
        modem_user: &str,
        modem_pass: &str,
        auth_method: AuthenticationMethod,
    ) -> Result<(), ModemError> {
        info!("Configuring context...");

        match self
            .client
            .send(&ConfigureContext {
                context_id: 1,
                context_type: 1, // IPV4
                apn: HeaplessString::try_from(modem_apn).unwrap(),
                username: HeaplessString::try_from(modem_user).unwrap(),
                password: HeaplessString::try_from(modem_pass).unwrap(),
                authentication: auth_method as u8,
            })
            .await
        {
            Ok(_) => {
                log::info!("Context configuration set");
                Ok(())
            }
            Err(e) => {
                log::error!("Context configuration not set: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    pub async fn context_activate(&mut self) -> Result<core::time::Duration, ModemError> {
        info!("Activating context...");

        let now = compat::Instant::now();

        match self
            .client
            .send(&DeactivatePDPContext { context_id: 1 })
            .await
        {
            Ok(_) => {
                log::info!("Context deactivated");
            }
            Err(e) => {}
        }

        match self
            .client
            .send(&ActivatePDPContext { context_id: 1 })
            .await
        {
            Ok(_) => {
                log::info!("Context activated");
            }
            Err(e) => {
                log::error!("Context not activated: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        match self.client.send(&GetPDPContextInfo {}).await {
            Ok(status) => {
                log::info!("Context status: {:?}", status);
                let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                log::info!(
                    "IP {:?} obtained after {} s",
                    status.ip_address,
                    t.as_secs()
                );
                Ok(t)
            }
            Err(e) => {
                log::error!("Context status not found: {:?}", e);
                Err(ModemError::NoContext)
            }
        }
    }

    pub async fn context_deactivate(&mut self) -> Result<(), ModemError> {
        info!("Deactivating context...");

        match self
            .client
            .send(&DeactivatePDPContext { context_id: 1 })
            .await
        {
            Ok(_) => {
                log::info!("Context deactivated");
                Ok(())
            }
            Err(e) => {
                log::error!("Context not deactivated: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Connect to an MQTT broker.
    ///
    /// # Arguments
    ///
    /// * `url` - MQTT broker URL/hostname
    /// * `port` - MQTT broker port
    /// * `id` - MQTT client ID
    /// * `user` - MQTT username (can be empty)
    /// * `pass` - MQTT password (can be empty)
    /// * `ssl_config` - Optional SSL configuration. If provided, SSL will be configured and enabled
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Without SSL
    /// mm.mqtt_connect("broker.example.com", 1883, "client_id", "", "", None)?;
    ///
    /// // With SSL
    /// let mut ssl_config = SslConfiguration::new();
    /// ssl_config.set_ca_cert("cacert.pem").unwrap();
    /// mm.mqtt_connect("broker.example.com", 8883, "client_id", "", "", Some(ssl_config))?;
    /// ```
    pub async fn mqtt_connect(
        &mut self,
        url: &str,
        port: u16,
        id: &str,
        user: &str,
        pass: &str,
        ssl_config: Option<SslConfiguration>,
    ) -> Result<(), ModemError> {
        info!("Connecting to MQTT broker...");

        // Configure SSL if provided
        if let Some(config) = ssl_config {
            let ctx_id = config.get_context_id();

            if !self.ssl_configured {
                info!("SSL requested, configuring SSL context {}...", ctx_id);
                self.configure_ssl_context(config).await?;
            } else {
                info!("SSL already configured, skipping configuration");
            }

            // Enable SSL for MQTT
            match self
                .client
                .send(&ConfigureMqttSsl {
                    subcommand: HeaplessString::try_from("ssl").unwrap(),
                    tcp_connect_id: 0,
                    ssl_enable: MqttSslEnable::True,
                    ssl_ctx_id: ctx_id,
                })
                .await
            {
                Ok(_) => {
                    info!("MQTT SSL enabled with context {}", ctx_id);
                }
                Err(e) => {
                    error!("Failed to enable MQTT SSL: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            }
        }

        let mut subscriber = self.urc_channel.subscribe().unwrap();

        match self
            .client
            .send(&MqttOpen {
                tcp_connect_id: 0,
                server: HeaplessString::try_from(url).unwrap(),
                port,
            })
            .await
        {
            Ok(_) => {
                log::info!("Connected to MQTT broker");
            }
            Err(e) => {
                log::error!("MQTT broker not connected: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 10_000 {
            compat::delay_ms(500).await;

            match subscriber.try_next_message_pure() {
                Some(Urc::MqttOpen(mqtopen_response)) => {
                    log::info!("MQTT Open response: result={}", mqtopen_response.result);
                    match mqtopen_response.result {
                        0 => {
                            log::info!("Connection opened");
                            break;
                        }
                        -1 => {
                            log::error!("MQTT Open failed: network connection failed");
                            return Err(ModemError::NoNetwork);
                        }
                        1 => {
                            log::error!("MQTT Open failed: wrong parameter");
                            return Err(ModemError::MqttRequestFailed);
                        }
                        2 => {
                            log::error!("MQTT Open failed: MQTT identifier occupied");
                            return Err(ModemError::MqttRequestFailed);
                        }
                        3 => {
                            log::error!("MQTT Open failed: PDP activation failed");
                            return Err(ModemError::NoContext);
                        }
                        4 => {
                            log::error!("MQTT Open failed: DNS parse failed");
                            if port == 8883 {
                                return Err(ModemError::SslHostnameMismatch);
                            }
                            return Err(ModemError::MqttRequestFailed);
                        }
                        5 => {
                            log::error!("MQTT Open failed: network disconnection");
                            if port == 8883 {
                                return Err(ModemError::SslCertificateInvalid);
                            }
                            return Err(ModemError::NoNetwork);
                        }
                        _ => {
                            log::error!(
                                "MQTT Open failed with unknown result={}",
                                mqtopen_response.result
                            );
                            if port == 8883 {
                                return Err(ModemError::SslCipherNegotiationFailed);
                            }
                            return Err(ModemError::MqttRequestFailed);
                        }
                    }
                }
                Some(_) => {
                    log::debug!("Received other URC, waiting for MQTT Open");
                }
                None => {
                    log::debug!("Waiting for response...");
                }
            }
        }

        match self
            .client
            .send(&MqttConnect {
                tcp_connect_id: 0,
                client_id: HeaplessString::try_from(id).unwrap(),
                username: Some(HeaplessString::try_from(user).unwrap()),
                password: Some(HeaplessString::try_from(pass).unwrap()),
            })
            .await
        {
            Ok(_) => {
                log::info!("Connected to MQTT broker");
            }
            Err(e) => {
                log::error!("MQTT broker not connected: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 5_000 {
            compat::delay_ms(500).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::MqttConnect(mqtconnect_response)) => {
                    log::info!(
                        "MQTT Connect response: result={}, ret_code={}",
                        mqtconnect_response.result,
                        mqtconnect_response.ret_code
                    );
                    match mqtconnect_response.result {
                        0 => {
                            // Packet sent successfully, now check ret_code
                            match mqtconnect_response.ret_code {
                                0 => {
                                    log::info!("Client connected");
                                    break;
                                }
                                1 => {
                                    log::error!(
                                        "Connection refused: unacceptable protocol version"
                                    );
                                    return Err(ModemError::MqttRequestFailed);
                                }
                                2 => {
                                    log::error!("Connection refused: identifier rejected");
                                    return Err(ModemError::MqttRequestFailed);
                                }
                                3 => {
                                    log::error!("Connection refused: server unavailable");
                                    return Err(ModemError::MqttRequestFailed);
                                }
                                4 => {
                                    log::error!("Connection refused: bad user name or password");
                                    return Err(ModemError::MqttRequestFailed);
                                }
                                5 => {
                                    log::error!("Connection refused: not authorized");
                                    return Err(ModemError::MqttRequestFailed);
                                }
                                _ => {
                                    log::error!(
                                        "Connection refused with unknown ret_code={}",
                                        mqtconnect_response.ret_code
                                    );
                                    return Err(ModemError::MqttRequestFailed);
                                }
                            }
                        }
                        _ => {
                            log::error!(
                                "MQTT Connect failed with result={}",
                                mqtconnect_response.result
                            );
                            return Err(ModemError::MqttRequestFailed);
                        }
                    }
                }
                Some(_) => {
                    log::debug!("Received other URC, waiting for MQTT Connect");
                }
                None => {
                    log::debug!("Waiting for response...");
                }
            }
        }
        Ok(())
    }

    pub async fn mqtt_disconnect(&mut self) -> Result<(), ModemError> {
        info!("Disconnecting from MQTT broker...");

        let mut subscriber = self.urc_channel.subscribe().unwrap();

        match self
            .client
            .send(&MqttDisconnect { tcp_connect_id: 0 })
            .await
        {
            Ok(_) => {
                log::info!("Disconnected from MQTT broker");
            }
            Err(e) => {
                log::error!("MQTT broker not disconnected: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 10_000 {
            compat::delay_ms(500).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::MqttDisconnect(mqtdisconnect_response)) => {
                    log::info!(
                        "MQTT Disconnect response: result={}",
                        mqtdisconnect_response.result
                    );
                    match mqtdisconnect_response.result {
                        0 => {
                            log::info!("Client disconnected");
                            break;
                        }
                        _ => {
                            log::error!(
                                "MQTT Disconnect failed with result={}",
                                mqtdisconnect_response.result
                            );
                            return Err(ModemError::MqttRequestFailed);
                        }
                    }
                }
                Some(_) => {
                    log::debug!("Received other URC, waiting for MQTT Disconnect");
                }
                None => {
                    log::debug!("Waiting for response...");
                }
            }
        }

        if self.rev == ModemRevision::R200 {
            let now = compat::Instant::now();
            while compat::elapsed_ms(now) < 5_000 {
                compat::delay_ms(500).await;
                match subscriber.try_next_message_pure() {
                    Some(Urc::MqttStatus(mqttstatus_response)) => {
                        log::info!("MQTT Status response: err={}", mqttstatus_response.err);
                        match mqttstatus_response.err {
                            5 => {
                                log::info!("Client disconnected");
                                return Ok(());
                            }
                            _ => {
                                log::error!(
                                    "MQTT Status failed with err={}",
                                    mqttstatus_response.err
                                );
                                return Err(ModemError::MqttRequestFailed);
                            }
                        }
                    }
                    Some(_) => {
                        log::debug!("Received other URC, waiting for MQTT Status");
                    }
                    None => {
                        log::debug!("Waiting for response...");
                    }
                }
            }

            match self.client.send(&MqttClose { tcp_connect_id: 0 }).await {
                Ok(_) => {
                    log::info!("Disconnected from MQTT broker");
                }
                Err(e) => {
                    log::error!("MQTT broker not disconnected: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            }

            let now = compat::Instant::now();
            while compat::elapsed_ms(now) < 5_000 {
                compat::delay_ms(500).await;
                match subscriber.try_next_message_pure() {
                    Some(Urc::MqttClose(mqtclose_response)) => {
                        log::info!("MQTT Close response: result={}", mqtclose_response.result);
                        match mqtclose_response.result {
                            0 => {
                                log::info!("Connection closed");
                                return Ok(());
                            }
                            _ => {
                                log::error!(
                                    "MQTT Close failed with result={}",
                                    mqtclose_response.result
                                );
                                return Err(ModemError::MqttRequestFailed);
                            }
                        }
                    }
                    Some(_) => {
                        log::debug!("Received other URC, waiting for MQTT Close");
                    }
                    None => {
                        log::debug!("Waiting for response...");
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn mqtt_publish(
        &mut self,
        topic: &str,
        payload: &str,
        qos: u8,
    ) -> Result<(), ModemError> {
        info!("Publishing to {}", topic);
        let msg_id = if qos == 0 { 0 } else { 1 };

        let mut subscriber = self.urc_channel.subscribe().unwrap();

        match self
            .client
            .send(&MqttPublishExtended {
                tcp_connect_id: 0,
                msg_id,
                qos,
                retain: 0,
                topic: HeaplessString::try_from(topic).unwrap(),
                payload: HeaplessString::try_from(payload).unwrap(),
            })
            .await
        {
            Ok(_) => {
                log::info!("Published to MQTT broker");
            }
            Err(e) => {
                log::error!("MQTT broker not published: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 10_000 {
            compat::delay_ms(500).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::MqttPublish(mqtpublish_response)) => {
                    log::info!(
                        "MQTT Publish response: result={}",
                        mqtpublish_response.result
                    );
                    match mqtpublish_response.result {
                        0 => {
                            log::info!("Publishing successful");
                            break;
                        }
                        _ => {
                            log::error!(
                                "Publishing failed with result={}",
                                mqtpublish_response.result
                            );
                            return Err(ModemError::MqttRequestFailed);
                        }
                    }
                }
                Some(_) => {
                    log::debug!("Received other URC, waiting for MQTT Publish");
                }
                None => {
                    log::debug!("Waiting for response...");
                }
            }
        }

        Ok(())
    }

    /// Open a TCP+TLS socket to `host:port` using SSL context `ssl_ctx_id`.
    ///
    /// The SSL context (CA cert, and for mTLS the client cert + key) must have
    /// been configured first with [`configure_ssl_context`](Self::configure_ssl_context),
    /// and a PDP context must be active ([`context_activate`](Self::context_activate)).
    /// The socket is opened in buffer access mode: use
    /// [`ssl_socket_recv`](Self::ssl_socket_recv) to read and
    /// [`ssl_socket_send`](Self::ssl_socket_send) to write.
    ///
    /// `host` is passed to the modem verbatim, so it is used for SNI / hostname
    /// verification when those are enabled on the SSL context.
    pub async fn ssl_socket_open(
        &mut self,
        client_id: u8,
        ssl_ctx_id: u8,
        host: &str,
        port: u16,
    ) -> Result<(), ModemError> {
        info!("Opening SSL socket {} to {}:{}", client_id, host, port);

        let mut subscriber = self.urc_channel.subscribe().unwrap();

        match self
            .client
            .send(&SslOpen {
                pdp_ctx_id: 1,
                ssl_ctx_id,
                client_id,
                host: HeaplessString::try_from(host).map_err(|_| ModemError::NotSupported)?,
                port,
                access_mode: 0, // buffer access mode
            })
            .await
        {
            Ok(_) => {}
            Err(e) => {
                error!("QSSLOPEN command failed: {:?}", e);
                return Err(ModemError::SocketOpenFailed);
            }
        }

        // The TLS handshake result arrives as the +QSSLOPEN URC (can take a
        // while, especially for mTLS).
        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 30_000 {
            compat::delay_ms(200).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::SslOpen(r)) if r.client_id == client_id => {
                    if r.err == 0 {
                        info!("SSL socket {} opened", client_id);
                        return Ok(());
                    }
                    error!("QSSLOPEN failed for socket {}: err={}", client_id, r.err);
                    return Err(ModemError::SocketOpenFailed);
                }
                _ => {}
            }
        }

        error!("Timed out waiting for QSSLOPEN result");
        Err(ModemError::OperationTimeout)
    }

    /// Send `data` on an open SSL socket.
    ///
    /// Data larger than one AT payload chunk (256 bytes) is split across
    /// multiple `AT+QSSLSEND` operations.
    pub async fn ssl_socket_send(&mut self, client_id: u8, data: &[u8]) -> Result<(), ModemError> {
        if data.is_empty() {
            return Ok(());
        }
        if data.len() > u16::MAX as usize {
            return Err(ModemError::NotSupported);
        }

        for chunk in data.chunks(256) {
            // The `>` prompt after QSSLSEND is not a regular AT response, so an
            // error here is expected and ignored (same pattern as file upload).
            let _ = self
                .client
                .send(&SslSend {
                    client_id,
                    length: chunk.len() as u16,
                })
                .await;

            compat::delay_ms(100).await;

            match self
                .client
                .send(&SendRawContents {
                    bytes: HeaplessBytes::try_from(chunk).unwrap(),
                })
                .await
            {
                Ok(_) => {
                    log::trace!("Sent {} bytes on socket {}", chunk.len(), client_id);
                }
                Err(e) => {
                    error!("QSSLSEND payload failed on socket {}: {:?}", client_id, e);
                    return Err(ModemError::SocketSendFailed);
                }
            }
        }

        Ok(())
    }

    /// Read currently-buffered data from an open SSL socket into `buf`.
    ///
    /// Returns the number of bytes read, which is `0` when the modem has no
    /// data buffered right now (callers typically wait for the
    /// `+QSSLURC: "recv",<id>` URC and retry). At most 512 bytes are returned
    /// per call regardless of `buf` length.
    pub async fn ssl_socket_recv(
        &mut self,
        client_id: u8,
        buf: &mut [u8],
    ) -> Result<usize, ModemError> {
        let want = core::cmp::min(buf.len(), 512);
        if want == 0 {
            return Ok(0);
        }

        match self
            .client
            .send(&SslRecv {
                client_id,
                length: want as u16,
            })
            .await
        {
            Ok(resp) => {
                let n = core::cmp::min(resp.length as usize, resp.data.len());
                let n = core::cmp::min(n, buf.len());
                buf[..n].copy_from_slice(&resp.data[..n]);
                Ok(n)
            }
            Err(e) => {
                error!("QSSLRECV failed on socket {}: {:?}", client_id, e);
                Err(ModemError::SocketRecvFailed)
            }
        }
    }

    /// Close an open SSL socket.
    pub async fn ssl_socket_close(&mut self, client_id: u8) -> Result<(), ModemError> {
        info!("Closing SSL socket {}", client_id);
        match self
            .client
            .send(&SslClose {
                client_id,
                timeout: 10,
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("QSSLCLOSE failed on socket {}: {:?}", client_id, e);
                Err(ModemError::SocketCloseFailed)
            }
        }
    }

    pub async fn configure_gnss_priority(&mut self) -> Result<(), ModemError> {
        match self.client.send(&ConfigureGnssPriorityMode).await {
            Ok(_) => {}
            Err(e) => {
                log::error!("Setting GNSS priority failed ({:?})", e);
                return Err(ModemError::NotResponding);
            }
        }

        match self
            .client
            .send(&SetGnssConstellation {
                param: HeaplessString::try_from("gnssconfig").unwrap(),
                constellation: GnssConstellation::Galileo,
            })
            .await
        {
            Ok(_) => {}
            Err(e) => {
                log::error!("Setting GNSS constellation failed ({:?})", e);
                return Err(ModemError::NotResponding);
            }
        }

        Ok(())
    }

    pub async fn turn_on_gnss(
        &mut self,
        operating_mode: GnssOperatingMode,
    ) -> Result<(), ModemError> {
        match self
            .client
            .send(&TurnOnGnss {
                mode: operating_mode,
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("Unable to turn on GNSS ({:?})", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    pub async fn turn_off_gnss(&mut self) -> Result<(), ModemError> {
        match self.client.send(&TurnOffGnss).await {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("Unable to turn on GNSS ({:?})", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    pub async fn get_gnss_location(
        &mut self,
    ) -> Result<GnssPositionInformationResponse, ModemError> {
        match self.client.send(&GetGgaNmeaSentence).await {
            Ok(response) => {
                if response.quality == 0 {
                    return Err(ModemError::GnssNotFixed);
                }
            }
            Err(_) => {
                return Err(ModemError::NotResponding);
            }
        }

        match self.client.send(&GetGnssPositionInformation).await {
            Ok(data) => Ok(data),
            Err(e) => {
                log::error!("Unexpected error occurred: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    pub async fn delete_file_from_internal_flash(
        &mut self,
        filename: &str,
    ) -> Result<(), ModemError> {
        match self
            .client
            .send(&DeleteFileFromInternalFlash {
                file_path: HeaplessString::try_from(filename).unwrap(),
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("Unable to delete file ({:?})", e);
                Err(ModemError::FileDeletionFailed)
            }
        }
    }

    /// Configure SSL context for secure connections.
    ///
    /// This method configures the SSL/TLS context used for secure connections.
    /// It validates that the CA certificate exists before configuration and executes
    /// all SSL configuration commands in the proper sequence.
    ///
    /// # Arguments
    ///
    /// * `ssl_config` - SSL configuration struct containing all SSL/TLS parameters
    ///
    /// # Returns
    ///
    /// * `Ok(())` - SSL context configured successfully
    /// * `Err(ModemError)` - Configuration failed (cert not found, command error, etc.)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut ssl_config = SslConfiguration::new();
    /// ssl_config
    ///     .set_ca_cert("cacert.pem")
    ///     .set_ssl_version(SslVersion::Tls1_2)
    ///     .set_auth_mode(SslAuthenticationMode::ServerOnly);
    /// mm.configure_ssl_context(ssl_config)?;
    /// ```
    pub async fn configure_ssl_context(
        &mut self,
        ssl_config: SslConfiguration,
    ) -> Result<(), ModemError> {
        let context_id = ssl_config.get_context_id();
        let ca_cert_filename = ssl_config.get_ca_cert_filename();
        let client_cert_filename = ssl_config.get_client_cert_filename();
        let client_key_filename = ssl_config.get_client_key_filename();
        let ssl_version = ssl_config.get_ssl_version();
        let cipher_suite = ssl_config.get_cipher_suite();
        let security_level = ssl_config.get_auth_mode();
        let sni_enable = ssl_config.get_sni_enable();
        let checkhost_enable = ssl_config.get_checkhost_enable();
        let ignore_localtime = ssl_config.get_ignore_localtime();

        debug!("Configuring SSL context {}...", context_id);

        // Configure cert if provided
        if ca_cert_filename.is_empty() {
            debug!("CA certificate filename is empty");
        } else {
            // Validate that CA certificate exists
            match self
                .get_file_meta_from_internal_flash(ca_cert_filename)
                .await
            {
                Ok((filename, size)) => {
                    debug!(
                        "Found CA certificate: {} ({} bytes)",
                        filename.as_str(),
                        size
                    );
                }
                Err(_) => {
                    error!("CA certificate not found: {}", ca_cert_filename);
                    return Err(ModemError::SslCertificateNotFound);
                }
            }

            let cert_path = HeaplessString::try_from(ca_cert_filename).unwrap();

            // Configure CA certificate
            match self
                .client
                .send(&ConfigureSslCaCertificate {
                    subcommand: HeaplessString::try_from("cacert").unwrap(),
                    context_id,
                    ca_cert_path: cert_path,
                })
                .await
            {
                Ok(_) => {
                    debug!("CA certificate configured");
                }
                Err(e) => {
                    error!("Failed to configure CA certificate: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            }
        }

        // Configure the client certificate + private key for mutual TLS.
        // Both must be present; a client cert without its key (or vice versa) is
        // a misconfiguration.
        if !client_cert_filename.is_empty() || !client_key_filename.is_empty() {
            if client_cert_filename.is_empty() || client_key_filename.is_empty() {
                error!("mTLS requires both a client certificate and a client key");
                return Err(ModemError::SslCertificateInvalid);
            }

            // Validate both files exist in UFS before referencing them.
            for file in [client_cert_filename, client_key_filename] {
                if self.get_file_meta_from_internal_flash(file).await.is_err() {
                    error!("Client credential not found: {}", file);
                    return Err(ModemError::SslCertificateNotFound);
                }
            }

            match self
                .client
                .send(&ConfigureSslClientCertificate {
                    subcommand: HeaplessString::try_from("clientcert").unwrap(),
                    context_id,
                    client_cert_path: HeaplessString::try_from(client_cert_filename).unwrap(),
                })
                .await
            {
                Ok(_) => debug!("Client certificate configured"),
                Err(e) => {
                    error!("Failed to configure client certificate: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            }

            match self
                .client
                .send(&ConfigureSslClientPrivateKey {
                    subcommand: HeaplessString::try_from("clientkey").unwrap(),
                    context_id,
                    client_key_path: HeaplessString::try_from(client_key_filename).unwrap(),
                })
                .await
            {
                Ok(_) => debug!("Client private key configured"),
                Err(e) => {
                    error!("Failed to configure client private key: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            }
        }

        // Configure security level
        match self
            .client
            .send(&ConfigureSslSecurityLevel {
                subcommand: HeaplessString::try_from("seclevel").unwrap(),
                context_id,
                security_level,
            })
            .await
        {
            Ok(_) => {
                debug!("Security level configured");
            }
            Err(e) => {
                error!("Failed to configure security level: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        // Configure SSL version
        match self
            .client
            .send(&ConfigureSslVersion {
                subcommand: HeaplessString::try_from("sslversion").unwrap(),
                context_id,
                ssl_version,
            })
            .await
        {
            Ok(_) => {
                debug!("SSL version configured");
            }
            Err(e) => {
                error!("Failed to configure SSL version: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        // Configure cipher suite (default to SupportAll if not specified)
        let cipher_suites =
            cipher_suite.unwrap_or_else(|| SslCipherSuiteEnum::SupportAll.to_bytes());

        match self
            .client
            .send(&ConfigureSslCipherSuites {
                subcommand: HeaplessString::try_from("ciphersuite").unwrap(),
                context_id,
                cipher_suites,
            })
            .await
        {
            Ok(_) => {
                debug!("Cipher suites configured");
            }
            Err(e) => {
                error!("Failed to configure cipher suites: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        // Configure SNI (Server Name Indication)
        match self
            .client
            .send(&ConfigureSslSni {
                subcommand: HeaplessString::try_from("sni").unwrap(),
                context_id,
                sni_enable,
            })
            .await
        {
            Ok(_) => {
                debug!("SNI configured");
            }
            Err(e) => {
                error!("Failed to configure SNI: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        // Configure hostname validation
        /*
        match self
            .client
            .send(&ConfigureSslCheckHost {
                subcommand: HeaplessString::try_from("checkhost").unwrap(),
                context_id,
                checkhost_enable,
            })
            .await
        {
            Ok(_) => {
                debug!("Hostname validation configured");
            }
            Err(e) => {
                error!("Failed to configure hostname validation: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }
        */

        // Configure ignore local time
        match self
            .client
            .send(&ConfigureSslIgnoreLocalTime {
                subcommand: HeaplessString::try_from("ignorelocaltime").unwrap(),
                context_id,
                ignore_local_time: ignore_localtime,
            })
            .await
        {
            Ok(_) => {
                debug!("Local time validation configured");
            }
            Err(e) => {
                error!("Failed to configure local time validation: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        self.ssl_configured = true;
        info!("SSL context {} configured successfully", context_id);
        Ok(())
    }

    /// List all files from internal flash storage using AT+QFLST command.
    ///
    /// This function retrieves the list of all files stored in the UFS (User File Storage).
    /// It returns a vector of tuples containing the filename and file size in bytes.
    ///
    /// # Returns
    ///
    /// * `Ok(heapless::Vec<(HeaplessString<80>, u32), 5>)` - A vector of tuples with filename and size
    /// * `Err(ModemError)` - If the command fails or no files are found
    pub async fn get_all_files_list_from_internal_flash(
        &mut self,
    ) -> Result<atat::heapless::Vec<(HeaplessString<80>, u32), 5>, ModemError> {
        // Use "*" pattern to list all files in UFS
        match self
            .client
            .send(&ListFilesFromInternalFlash {
                name_pattern: HeaplessString::try_from("*").unwrap(),
            })
            .await
        {
            Ok(response) => {
                // Convert the response entries to a heapless Vec of (name, size) tuples
                let mut files: atat::heapless::Vec<(HeaplessString<80>, u32), 5> =
                    atat::heapless::Vec::new();
                for entry in response.files.iter() {
                    // The response Vec has the same capacity, so pushing cannot overflow.
                    let _ = files.push((entry.filename.clone(), entry.file_size));
                }
                Ok(files)
            }
            Err(e) => {
                log::error!("Unable to list files ({:?})", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Get metadata for a single file from internal flash storage using AT+QFLST command.
    ///
    /// This function retrieves the metadata (filename and size) for a specific file
    /// stored in the UFS (User File Storage).
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the file to query
    ///
    /// # Returns
    ///
    /// * `Ok((HeaplessString<80>, u32))` - A tuple with filename and size in bytes
    /// * `Err(ModemError)` - If the command fails or the file is not found
    pub async fn get_file_meta_from_internal_flash(
        &mut self,
        filename: &str,
    ) -> Result<(HeaplessString<80>, u32), ModemError> {
        match self
            .client
            .send(&ListFilesFromInternalFlash {
                name_pattern: HeaplessString::try_from(filename).unwrap(),
            })
            .await
        {
            Ok(response) => {
                // Check if we got exactly one file
                if response.files.is_empty() {
                    log::error!("File not found: {}", filename);
                    return Err(ModemError::FileUploadFailed);
                }

                // Return the first (and should be only) file's metadata
                let entry = &response.files[0];
                Ok((entry.filename.clone(), entry.file_size))
            }
            Err(e) => {
                log::error!("Unable to get file metadata ({:?})", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Upload a file to internal flash storage using AT+QFUPL command.
    ///
    /// This function uploads a binary file to the UFS (User File Storage) of the modem.
    /// It sends the file in raw binary mode after initiating the upload command.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the file to upload to internal storage
    /// * `data` - The binary data of the file to upload
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the file is uploaded successfully
    /// * `Err(ModemError)` - If the command fails or the upload is unsuccessful
    pub async fn upload_file_to_internal_flash(
        &mut self,
        filename: &str,
        data: &[u8],
    ) -> Result<(), ModemError> {
        let len = data.len() as u32;
        const TIMEOUT: u16 = 3;

        match self
            .client
            .send(&FileUploadToInternalFlash {
                file_path: HeaplessString::try_from(filename).unwrap(),
                file_size: len,
                timeout: TIMEOUT.into(),
                ack_mode: None,
            })
            .await
        {
            Ok(_) => {}
            Err(_e) => {
                // log::error!("Unable to start file upload ({:?})", e);
                // return Err(ModemError::NotResponding);
            }
        }

        // TODO: deal with the CONNECT URC.
        // For now, just wait enought time for the modem to be ready.
        compat::delay_ms(300).await;

        // Uploading file contents. It must be done in chunks of less than INGRESS_BUFFER_SIZE.
        // We don't know the exact size of the ingress buffer, so we use 128 bytes as a safe value.
        log::trace!("Uploading file...");
        for chunk in data.chunks(256) {
            match self
                .client
                .send(&SendRawContents {
                    bytes: HeaplessBytes::try_from(chunk).unwrap(),
                })
                .await
            {
                Ok(_) => {
                    log::trace!("Uploaded {} bytes", chunk.len());
                }
                Err(e) => {
                    log::error!("Error uploading file chunk ({:?})", e);
                    return Err(ModemError::FileUploadFailed);
                }
            }
        }

        //
        let mut subscriber = self.urc_channel.subscribe().unwrap();
        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < (TIMEOUT as u64) * 1000 {
            compat::delay_ms(500).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::FileUploadDone(upload_response)) => {
                    log::debug!("File Upload response: {:?}", upload_response);
                    if upload_response.upload_size != len {
                        log::error!("Upload size mismatch");
                        return Err(ModemError::FileUploadFailed);
                    }
                }
                Some(e) => {
                    log::error!("Unknown URC {:?}", e);
                }
                None => {
                    log::debug!("Waiting for response...");
                }
            }
        }

        Ok(())
    }

    /// Download a file from internal flash storage using AT+QFDWL command.
    ///
    /// This function retrieves a file stored in the UFS (User File Storage).
    /// The modem responds with CONNECT, then outputs the binary data, and finally
    /// sends +QFDWL response with download_size and checksum.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the file to download from internal storage
    /// * `buffer` - A mutable buffer to store the downloaded file data
    ///
    /// # Returns
    ///
    /// * `Ok(usize)` - The number of bytes downloaded
    /// * `Err(ModemError)` - If the command fails or the file cannot be downloaded
    ///
    /// # Note
    ///
    /// This implementation has limitations with the current atat framework for handling
    /// binary data mode. The actual binary data reading would need to be implemented
    /// at a lower level to properly capture the data between CONNECT and +QFDWL response.
    pub async fn work_in_progress_download_file_from_internal_flash(
        &mut self,
        filename: &str,
        buffer: &mut [u8],
    ) -> Result<usize, ModemError> {
        // Send the download command - the response includes the +QFDWL line with size and checksum
        match self
            .client
            .send(&DownloadFileFromInternalFlash {
                file_path: HeaplessString::try_from(filename).unwrap(),
            })
            .await
        {
            Ok(response) => {
                log::info!(
                    "File download completed: size={}, checksum={}",
                    response.download_size,
                    response.checksum
                );

                let downloaded_size = response.download_size as usize;

                if downloaded_size > buffer.len() {
                    log::error!("Buffer too small for downloaded file");
                    return Err(ModemError::FileUploadFailed);
                }

                // TODO: The modem sends CONNECT, then binary data, then +QFDWL response.
                // With the current atat framework, we cannot easily capture the raw binary data
                // between CONNECT and the +QFDWL response. A proper implementation would need to:
                // 1. Detect CONNECT response
                // 2. Read exactly download_size bytes of binary data from the serial port
                // 3. Validate the checksum
                //
                // For now, we just return the expected size from the +QFDWL response.
                // The binary data is currently being consumed by the atat ingress but not captured.
                Ok(downloaded_size)
            }
            Err(e) => {
                log::error!("Unable to download file ({:?})", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Read a file from internal flash storage using AT+QFOPEN and AT+QFREAD commands.
    ///
    /// This function opens a file, reads its contents in fixed-size blocks (1024 bytes),
    /// and closes it. The binary data is copied to the provided buffer.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the file to read from internal storage
    /// * `buffer` - A mutable buffer to store the file data
    ///
    /// # Returns
    ///
    /// * `Ok(usize)` - The number of bytes read
    /// * `Err(ModemError)` - If the command fails or the file cannot be read
    pub async fn read_file_from_internal_flash(
        &mut self,
        filename: &str,
        buffer: &mut [u8],
    ) -> Result<usize, ModemError> {
        // Open the file in read-only mode (mode 2)
        let filehandle = match self
            .client
            .send(&OpenFile {
                filename: HeaplessString::try_from(filename).unwrap(),
                mode: Some(2), // Read only
            })
            .await
        {
            Ok(response) => {
                log::info!("File opened with handle: {}", response.filehandle);
                response.filehandle
            }
            Err(e) => {
                log::error!("Unable to open file ({:?})", e);
                return Err(ModemError::FileUploadFailed);
            }
        };

        let mut total_bytes_read = 0usize;
        const BLOCK_SIZE: usize = 128;

        // Read file in blocks
        while total_bytes_read < buffer.len() {
            let remaining = buffer.len() - total_bytes_read;
            let read_size = core::cmp::min(remaining, BLOCK_SIZE);

            // Read a block from the file
            match self
                .client
                .send(&ReadFile {
                    filehandle,
                    length: Some(read_size as u32),
                })
                .await
            {
                Ok(response) => {
                    log::info!("Read {} bytes from file", response.read_length);

                    // Check if we reached end of file
                    if response.read_length == 0 {
                        break;
                    }

                    // Copy data to user buffer
                    let bytes_to_copy =
                        core::cmp::min(response.read_length as usize, response.data.len());
                    let bytes_to_copy = core::cmp::min(bytes_to_copy, remaining);

                    buffer[total_bytes_read..total_bytes_read + bytes_to_copy]
                        .copy_from_slice(&response.data[..bytes_to_copy]);

                    total_bytes_read += bytes_to_copy;

                    // If we read less than requested, we've reached end of file
                    if response.read_length < read_size as u32 {
                        break;
                    }
                }
                Err(e) => {
                    log::error!("Unable to read file ({:?})", e);
                    // Close the file even if read failed
                    let _ = self.client.send(&CloseFile { filehandle }).await;
                    return Err(ModemError::FileUploadFailed);
                }
            }
        }

        // Close the file
        match self.client.send(&CloseFile { filehandle }).await {
            Ok(_) => {
                log::info!("File closed, read {} bytes total", total_bytes_read);
            }
            Err(e) => {
                log::error!("Unable to close file ({:?})", e);
                return Err(ModemError::FileUploadFailed);
            }
        }

        Ok(total_bytes_read)
    }

    /// Write data to a file in internal flash storage using AT+QFOPEN and AT+QFWRITE commands.
    ///
    /// This function opens a file (creating it if it doesn't exist or overwriting if it does),
    /// writes the data, and closes it.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the file to write to internal storage
    /// * `data` - The data to write to the file
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the file was written successfully
    /// * `Err(ModemError)` - If the command fails or the file cannot be written
    pub async fn write_file_to_internal_flash(
        &mut self,
        filename: &str,
        data: &[u8],
    ) -> Result<(), ModemError> {
        // Open the file in create/overwrite mode (mode 1)
        let filehandle = match self
            .client
            .send(&OpenFile {
                filename: HeaplessString::try_from(filename).unwrap(),
                mode: Some(1), // Create/overwrite
            })
            .await
        {
            Ok(response) => {
                log::debug!("File opened with handle: {}", response.filehandle);
                response.filehandle
            }
            Err(e) => {
                log::error!("Unable to open file ({:?})", e);
                return Err(ModemError::FileUploadFailed);
            }
        };

        // Send write command - it will respond with CONNECT but we ignore errors
        // because the command won't complete until we send the data
        match self
            .client
            .send(&WriteFile {
                filehandle,
                length: data.len() as u32,
                timeout: Some(10), // 10 seconds timeout
            })
            .await
        {
            Ok(_) => {}
            Err(_) => {
                // Expected to timeout or get an error since CONNECT is sent as URC
                // and the command is waiting for data
            }
        }

        // Wait a bit for the modem to enter data mode after CONNECT
        compat::delay_ms(300).await;

        // Uploading file contents. It must be done in chunks of less than INGRESS_BUFFER_SIZE.
        // We don't know the exact size of the ingress buffer, so we use 128 bytes as a safe value.
        log::trace!("Uploading file...");
        for chunk in data.chunks(256) {
            match self
                .client
                .send(&SendRawContents {
                    bytes: HeaplessBytes::try_from(chunk).unwrap(),
                })
                .await
            {
                Ok(_) => {
                    log::trace!("Uploaded {} bytes", chunk.len());
                }
                Err(e) => {
                    log::error!("Error uploading file chunk ({:?})", e);
                    return Err(ModemError::FileUploadFailed);
                }
            }
        }

        // Wait for the write completion URC
        let mut subscriber = self.urc_channel.subscribe().unwrap();
        let now = compat::Instant::now();
        let timeout_ms = 10_000u64;

        while compat::elapsed_ms(now) < timeout_ms {
            compat::delay_ms(100).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::FileWriteDone(write_response)) => {
                    log::info!(
                        "File write completed: written={}, total={}",
                        write_response.written_length,
                        write_response.total_length
                    );

                    if write_response.written_length != data.len() as u32 {
                        log::error!("Write size mismatch");
                        let _ = self.client.send(&CloseFile { filehandle }).await;
                        return Err(ModemError::FileUploadFailed);
                    }
                    break;
                }
                Some(e) => {
                    log::debug!("Received URC: {:?}", e);
                }
                None => {
                    // Continue waiting
                }
            }
        }

        // Close the file
        match self.client.send(&CloseFile { filehandle }).await {
            Ok(_) => {
                log::debug!("File closed");
                Ok(())
            }
            Err(e) => {
                log::error!("Unable to close file ({:?})", e);
                Err(ModemError::FileUploadFailed)
            }
        }
    }
}

/// Convert NTP datetime string to Unix timestamp.
fn get_timestamp_from_ntp_response(dt_str: &str) -> Result<i64, ModemError> {
    // Ignore timezone (indicates the difference, expressed in quarters of an hour, between the local time and GMT)
    // Turn "1970/01/01,00:00:00+00" into "1970-01-01T00:00:00Z"
    let fd = time::macros::format_description!(
        "[year]/[month]/[day],[hour]:[minute]:[second][ignore count:1][ignore count:2]"
    );
    let dt = time::PrimitiveDateTime::parse(dt_str, fd).map_err(|e| {
        log::error!("Failed to parse date time string: {:?}", e);
        ModemError::NtpRequestFailed
    })?;
    let dt = dt.assume_offset(time::UtcOffset::UTC);

    let ts = dt.unix_timestamp();
    log::info!("Timestamp: {}", ts);

    Ok(ts)
}

/// Convert NITZ datetime string to Unix timestamp.
fn get_timestamp_from_nitz_response(nitz_str: &str) -> Result<i64, ModemError> {
    // Ignore timezone (indicates the difference, expressed in quarters of an hour, between the local time and GMT)
    // Ignore Daylight saving time
    // Turn "1970/01/01,00:00:00+00,0" into "1970-01-01T00:00:00Z"
    let fd = time::macros::format_description!(
        "[year]/[month]/[day],[hour]:[minute]:[second][ignore count:1][ignore count:2],[ignore count:1]"
    );
    let dt = time::PrimitiveDateTime::parse(nitz_str, fd).unwrap();
    let dt = dt.assume_offset(time::UtcOffset::UTC);

    let ts = dt.unix_timestamp();
    log::info!("Timestamp: {}", ts);
    Ok(ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_ntp_time_to_unix_timestamp() {
        let dt_str = "1970/01/01,00:00:00+00";
        let ts = get_timestamp_from_ntp_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 0);

        let dt_str = "2025/11/11,19:39:05+00";
        let ts = get_timestamp_from_ntp_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);
    }

    #[test]
    fn test_convert_ntp_time_to_unix_timestamp_ignore_timezone() {
        let dt_str = "2025/11/11,19:39:05+04";
        let ts = get_timestamp_from_ntp_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);

        let dt_str = "2025/11/11,19:39:05-04";
        let ts = get_timestamp_from_ntp_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);
    }

    #[test]
    fn test_convert_nitz_time_to_unix_timestamp() {
        let dt_str = "1970/01/01,00:00:00+00,0";
        let ts = get_timestamp_from_nitz_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 0);

        let dt_str = "2025/11/11,19:39:05+00,1";
        let ts = get_timestamp_from_nitz_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);
    }

    #[test]
    fn test_convert_nitz_time_to_unix_timestamp_ignore_timezone() {
        let dt_str = "2025/11/11,19:39:05+04,0";
        let ts = get_timestamp_from_nitz_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);

        let dt_str = "2025/11/11,19:39:05-04,1";
        let ts = get_timestamp_from_nitz_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);
    }

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
    }
}
