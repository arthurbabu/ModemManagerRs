//! Quectel BG95 (Cat-M / NB-IoT / GSM) chip profile.

use super::{ChipProfile, ModemRevision};
use crate::cellular::{ModemMode, INGRESS_BUF_SIZE};
use crate::quectel_atat::types::ModemConfiguration;
use crate::ModemError;

use atat::asynch::Client;
use embedded_io_async::Write;

pub(crate) struct Bg95;

impl ChipProfile for Bg95 {
    const NAME: &'static str = "BG95";
    // See `Band for EmtcBands`/`Band for NbIotBands` in
    // `quectel_atat::types` for how this combines with the enum's
    // `#[cfg(feature = "bg95")]`-gated per-variant band list.
    const EMTC_ALL_BANDS_MASK: u128 = 0x100182000000004F0E189F;
    const NB_ALL_BANDS_MASK: u128 = 0x1001C200000000490E189F;

    async fn configure_modem<W: Write>(
        client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
        config: &ModemConfiguration,
    ) -> Result<(), ModemError> {
        super::classic::configure_bg9x_modem(client, config).await
    }

    async fn poll_attach_status<W: Write>(
        client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
    ) -> Result<Option<ModemMode>, ModemError> {
        super::classic::poll_bg9x_attach(client).await
    }

    fn classify_revision(version: &str) -> ModemRevision {
        match version {
            s if s.contains("BG95M3LAR02A03_01.200.01.200") => ModemRevision::R200,
            s if s.contains("BG95M3LAR02A03_01.014.01.014") => ModemRevision::R014,
            s if s.contains("BG95M3LAR02A03_01.012.01.012") => ModemRevision::R012,
            _ => ModemRevision::Unknown,
        }
    }

    fn needs_explicit_mqtt_close(rev: ModemRevision) -> bool {
        // AT+QMTDISC alone isn't reliable on R200 firmware: it needs an
        // extra MqttStatus-URC wait plus an explicit AT+QMTCLO.
        rev == ModemRevision::R200
    }
}
