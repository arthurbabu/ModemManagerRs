//! Quectel EG916U (LTE Cat 1bis + GSM) chip profile.
//!
//! EG916U is not a Cat-M/NB-IoT part like BG95/BG96, so it doesn't share
//! [`super::classic`]'s AT command sequence: band/RAT/service-domain
//! configuration and network-attach polling both use different commands and
//! response shapes. See the `TODO(eg916u)` markers below and in
//! `quectel_atat::types` -- the band masks and band/RAT AT-command layout are
//! provisional, not yet verified against the EG916U datasheet.

#[cfg(feature = "defmt")]
use defmt::*;

#[cfg(not(feature = "defmt"))]
use log::*;

use super::{ChipProfile, ModemRevision};
use crate::cellular::{ModemMode, INGRESS_BUF_SIZE};
use crate::quectel_atat::types::*;
use crate::quectel_atat::*;
use crate::ModemError;

use atat::asynch::{AtatClient, Client};
use atat::heapless::String as HeaplessString;
use embedded_io_async::Write;

pub(crate) struct Eg916u;

impl ChipProfile for Eg916u {
    const NAME: &'static str = "EG916U";
    // TODO(eg916u): confirm the LTE Cat 1bis band mask against the EG916U
    // datasheet. This reuses the EMTC field of AT+QCFG="band" to carry the
    // LTE band mask. Provisional value covers common EU LTE-FDD bands
    // 1/3/5/8/20/28.
    const EMTC_ALL_BANDS_MASK: u128 = 0x800800B5;
    // TODO(eg916u): EG916U (Cat 1bis) has no NB-IoT RAT. Left at 0 until the
    // EG916U band configuration is confirmed; selecting NB-IoT `Any` on this
    // chip therefore requests no bands.
    const NB_ALL_BANDS_MASK: u128 = 0x0;

    async fn configure_modem<W: Write>(
        client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
        _config: &ModemConfiguration,
    ) -> Result<(), ModemError> {
        // EG916U: the BG-family QCFG="band" (three per-RAT masks),
        // "iotopmode" and "nwscanseq" layout does NOT apply to this chip and
        // would be rejected, so we don't send them. Band / RAT selection is
        // left at the modem default (automatic); we only pin the service
        // domain to Packet-Switched for data, best-effort.
        //
        // TODO(eg916u): once the EG916U AT manual is available, set
        // AT+QCFG="band" with the LTE band layout and the correct
        // "nwscanseq" RAT codes here instead of relying on defaults.
        if let Err(e) = client
            .send(&ConfigureServiceDomain {
                param: HeaplessString::try_from("servicedomain").unwrap(),
                service_domain: 1, // PS: Packet Switched
                effect: ConfigurationEffect::Immediately,
            })
            .await
        {
            // Not fatal on EG916U: fall back to the modem default.
            warn!(
                "EG916U: could not set service domain ({:?}); using default",
                e
            );
        }

        info!("EG916U modem configuration set (bands left at modem default)");
        Ok(())
    }

    async fn poll_attach_status<W: Write>(
        client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
    ) -> Result<Option<ModemMode>, ModemError> {
        match client.send(&GetCopsInfo).await {
            Ok(info) => {
                info!("Network info: {:?}", info);

                // 0,3 = GSM/2G | 7,8 = LTE/Cat-M1 | 9 = NB-IoT
                let act = match info.act {
                    Some(7) | Some(8) => "LTE",
                    Some(9) => "NBIoT",
                    Some(0) | Some(3) => "GSM",
                    _ => "SEARCH",
                };

                match act {
                    "SEARCH" => {
                        debug!("Searching...");
                        Ok(None)
                    }
                    "LTE" => {
                        info!("Using LTE");
                        Ok(Some(ModemMode::LTEM))
                    }
                    "GSM" => {
                        info!("Using 2G");
                        Ok(Some(ModemMode::EDGE))
                    }
                    "NBIoT" => {
                        info!("Using NB-IoT");
                        Ok(Some(ModemMode::NBIoT))
                    }
                    _ => {
                        warn!("Unknown or unstable technology code: {}", act);
                        Ok(None)
                    }
                }
            }
            Err(e) => {
                error!("Network info error: {:?}", e);
                Ok(None)
            }
        }
    }

    fn classify_revision(version: &str) -> ModemRevision {
        if version.contains("EG916") {
            ModemRevision::Eg916u
        } else {
            ModemRevision::Unknown
        }
    }

    // EG916U has no known R200-style MQTT-disconnect quirk; uses the trait
    // default (`false`).
}
