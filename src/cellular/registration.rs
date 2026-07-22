//! Network attach / registration -- the analogue of ModemManager's
//! `org.freedesktop.ModemManager1.Modem.Modem3gpp` `Register` call.
//!
//! Per-chip attach polling (which AT command to use, how to parse the
//! response) lives in [`crate::chip`] (see
//! [`ChipProfile::poll_attach_status`]); this module owns the chip-agnostic
//! polling loop and the (non-chip-specific) GPRS/EPS registration wait.

use super::*;

impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
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
                    info!("GPRS network registration status: {:?}", status);
                    match status.stat {
                        1 => {
                            let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                            info!("Registered (Home) after {} s", t.as_secs());
                            return Ok(t);
                        }
                        2 => {
                            debug!("Searching..."); // Searching
                            continue;
                        }
                        3 => {
                            error!("Registration denied");
                            return Err(ModemError::NoNetwork);
                        }
                        4 => {
                            error!("Registration failed");
                            return Err(ModemError::NoNetwork);
                        }
                        5 => {
                            let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                            info!("Registered (Roaming) after {} s", t.as_secs());
                            return Ok(t);
                        }
                        _ => {
                            error!("Unknown registration status");
                            return Err(ModemError::NoNetwork);
                        }
                    }
                }
                Err(e) => {
                    error!("GPRS network registration status not found: {:?}", e);
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
                    info!("EPS network registration status: {:?}", status);
                    match status.stat {
                        1 => {
                            let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                            info!("Registered (Home) after {} s", t.as_secs());
                            return Ok(t);
                        }
                        2 => {
                            debug!("Searching..."); // Searching
                            continue;
                        }
                        3 => {
                            error!("Registration denied");
                            return Err(ModemError::NoNetwork);
                        }
                        4 => {
                            error!("Registration failed");
                            return Err(ModemError::NoNetwork);
                        }
                        5 => {
                            let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                            info!("Registered (Roaming) after {} s", t.as_secs());
                            return Ok(t);
                        }
                        _ => {
                            error!("Unknown registration status");
                            return Err(ModemError::NoNetwork);
                        }
                    }
                }
                Err(e) => {
                    error!("EPS network registration status not found: {:?}", e);
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

            match ActiveChip::poll_attach_status(&mut self.client).await {
                Ok(Some(mode)) => {
                    self.mode = mode;
                    let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                    info!("Network attach complete after {} s", t.as_secs());
                    return Ok(t);
                }
                Ok(None) => continue,
                Err(e) => {
                    error!("Network info error: {:?}", e);
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
}
