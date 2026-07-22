//! SIM status -- the analogue of ModemManager's
//! `org.freedesktop.ModemManager1.Sim` interface.

use super::*;

impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
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
                    info!("SIM status: {:?}", status);
                    if status.code.contains("READY") {
                        info!("SIM Ready");
                        if let Ok(res) = self.client.send(&GetIccid {}).await {
                            info!("ICCID: {:?}", res);
                        }
                        return Ok(());
                    } else if status.code.contains("SIM PIN") {
                        error!("SIM PIN required");
                        return Err(ModemError::SimError);
                    }
                }
                Err(e) => {
                    debug!(
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
                            error!("SIM not inserted");
                            return Err(ModemError::SimError);
                        }
                        11 => {
                            error!("SIM PIN required");
                            return Err(ModemError::SimError);
                        }
                        // 13 (SIM failure) and 14 (SIM busy) are commonly
                        // transient during SIM init: keep retrying.
                        13 | 14 => {
                            info!("SIM not ready yet (CME {}), retrying...", cme_error.err);
                        }
                        other => {
                            warn!("Unhandled SIM CME error {}, retrying...", other);
                        }
                    }
                }
            }

            compat::delay_ms(1000).await;
        }

        error!("SIM not ready after {} attempts", ATTEMPTS);
        Err(ModemError::SimErrorUnknown)
    }
}
