//! Power on/off and factory-reset -- the analogue of ModemManager's
//! `org.freedesktop.ModemManager1.Modem` `Enable`/`Reset` calls.

use super::*;

impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
    async fn is_powered_on(&mut self) -> Result<(), ModemError> {
        // TODO: deal with the start URC, that has this format:
        //pub const CMD_POWER_ON_RES: &[u8] = b"\r\nRDY\r\n\r\nAPP RDY\r\n";
        // For now, just wait enought time for the modem to power on.
        compat::delay_secs(5).await;

        // Probe with AT until the modem answers. Right after boot it interleaves
        // RDY / APP RDY URCs, so the first few commands can time out.
        let mut alive = false;
        for _ in 0..3 {
            info!("Sending AT command");
            match self.client.send(&AT).await {
                Ok(_) => {
                    info!("Response Ok");
                    alive = true;
                    break;
                }
                Err(e) => {
                    error!("Response failed: {:?}", e);
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
                    info!("Echo off");
                    return Ok(());
                }
                Err(e) => {
                    error!("Echo off failed: {:?}", e);
                }
            }
            compat::delay_ms(500).await;
        }

        // Echo could not be turned off; the modem is still responsive but data
        // socket parsing will be unreliable. Surface it rather than silently
        // continuing.
        error!("Could not disable echo after modem power-on");
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
                info!("Modem powering down");
            }
            Err(e) => {
                error!("Modem not powered down: {:?}", e);
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
                info!("Modem powered down");
                Ok(())
            }
        }
    }

    pub async fn is_alive(&mut self) -> Result<(), ModemError> {
        match self.client.send(&AT).await {
            Ok(_) => {
                info!("Modem alive");
                Ok(())
            }
            Err(e) => {
                error!("Modem not alive: {:?}", e);
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

    /// Factory reset the modem
    ///
    /// This function should not be called very often because it erases the internal flash memory.
    pub async fn factory_reset(&mut self) -> Result<(), ModemError> {
        warn!("Factory Reset. This function should not be called very often.");

        match self.client.send(&ResetToFactoryDefault {}).await {
            Ok(_) => {}
            Err(e) => {
                error!("Factory reset not set: {:?}", e);
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
                error!("Restore configuration failed: {:?}", e);
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
}
