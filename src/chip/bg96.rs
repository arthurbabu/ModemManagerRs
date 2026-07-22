//! Quectel BG96 (Cat-M / NB-IoT / GSM) chip profile.

use super::{ChipProfile, ModemRevision};
use crate::cellular::{ModemMode, INGRESS_BUF_SIZE};
use crate::quectel_atat::types::ModemConfiguration;
use crate::ModemError;

use atat::asynch::Client;
use embedded_io_async::Write;

pub(crate) struct Bg96;

impl ChipProfile for Bg96 {
    const NAME: &'static str = "BG96";
    // See `Band for EmtcBands`/`Band for NbIotBands` in
    // `quectel_atat::types` for how this combines with the enum's
    // `#[cfg(feature = "bg96")]`-gated per-variant band list.
    const EMTC_ALL_BANDS_MASK: u128 = 0xB0E189F;
    const NB_ALL_BANDS_MASK: u128 = 0xB0E189F;

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
            s if s.contains("BG96MAR02A07M1G_01.018.00.000") => ModemRevision::R018,
            s if s.contains("BG96MAR02A07M1G_01.018.01.018") => ModemRevision::R018,
            _ => ModemRevision::Unknown,
        }
    }

    // R018 firmware never needed the R200-only explicit-MQTT-close quirk;
    // uses the trait default (`false`).
}
