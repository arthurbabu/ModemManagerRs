//! Shared BG95/BG96 behavior.
//!
//! Both chips send identical AT command sequences
//! (`AT+QCFG="band"/"nwscanseq"/"nwscanmode"/"servicedomain"/"iotopmode"` and
//! `AT+QNWINFO`) -- they differ only in which bands are valid, which
//! [`super::bg95::Bg95`] and [`super::bg96::Bg96`] supply as their own
//! `ChipProfile::EMTC_ALL_BANDS_MASK`/`NB_ALL_BANDS_MASK`.

#[cfg(feature = "defmt")]
use defmt::*;

#[cfg(not(feature = "defmt"))]
use log::*;

use crate::cellular::{ModemMode, INGRESS_BUF_SIZE};
use crate::quectel_atat::types::*;
use crate::quectel_atat::*;
use crate::ModemError;

use atat::asynch::{AtatClient, Client};
use atat::heapless::String as HeaplessString;
use atat::heapless_bytes::Bytes as HeaplessBytes;
use embedded_io_async::Write;

/// Cat-M / NB-IoT specific (`AT+QCFG="iotopmode"`); not applicable to EG916U.
fn get_iotop_mode(configuration: &ModemConfiguration) -> Result<u8, ModemError> {
    let rat = configuration.get_rat_order();
    let rat_order = rat.as_str();

    match (rat_order.contains("02"), rat_order.contains("03")) {
        (true, false) => Ok(0), // only EMTC
        (false, true) => Ok(1), // only NB-IoT
        (true, true) => Ok(2),  // both
        (false, false) => Err(ModemError::NotSupported),
    }
}

pub(crate) async fn configure_bg9x_modem<W: Write>(
    client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
    configuration: &ModemConfiguration,
) -> Result<(), ModemError> {
    match client
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
            error!("Modem configuration not set: {:?}", e);
            return Err(ModemError::NotResponding);
        }
    }

    match client
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
            error!("Modem configuration not set: {:?}", e);
            return Err(ModemError::NotResponding);
        }
    };

    match client
        .send(&ConfigureRatSearchingMode {
            param: HeaplessString::try_from("nwscanmode").unwrap(),
            rat_searching_mode: 0, // Automatic: GSM and LTE
            effect: ConfigurationEffect::Immediately,
        })
        .await
    {
        Ok(_) => {}
        Err(e) => {
            error!("Modem configuration not set: {:?}", e);
            return Err(ModemError::NotResponding);
        }
    };

    match client
        .send(&ConfigureServiceDomain {
            param: HeaplessString::try_from("servicedomain").unwrap(),
            service_domain: 1, // PS: Packet Switched
            effect: ConfigurationEffect::Immediately,
        })
        .await
    {
        Ok(_) => {}
        Err(e) => {
            error!("Modem configuration not set: {:?}", e);
            return Err(ModemError::NotResponding);
        }
    };

    match client
        .send(&ConfigureIotOpMode {
            param: HeaplessString::try_from("iotopmode").unwrap(),
            mode: get_iotop_mode(configuration)?,
            effect: ConfigurationEffect::Immediately,
        })
        .await
    {
        Ok(_) => {}
        Err(e) => {
            error!("Modem configuration not set: {:?}", e);
            return Err(ModemError::NotResponding);
        }
    };

    info!("Modem configuration set");
    Ok(())
}

pub(crate) async fn poll_bg9x_attach<W: Write>(
    client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
) -> Result<Option<ModemMode>, ModemError> {
    match client.send(&GetNetworkInfo).await {
        Ok(info) => {
            info!("Network info: {:?}", info);

            let act = info.act.as_str();

            if act == "SEARCH" || act.contains("No Service") {
                debug!("Searching...");
                return Ok(None);
            }

            match act {
                a if a.contains("LTE") => {
                    info!("Using LTE");
                    Ok(Some(ModemMode::LTEM))
                }
                a if a.contains("GSM") || a.contains("GPRS") || a.contains("EDGE") => {
                    info!("Using 2G");
                    Ok(Some(ModemMode::EDGE))
                }
                a if a.contains("NBIoT") => {
                    info!("Using NB-IoT");
                    Ok(Some(ModemMode::NBIoT))
                }
                _ => {
                    warn!("Unknown or unstable technology: {}", act);
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
