//! PDP context (bearer) configuration and activation -- the analogue of
//! ModemManager's `org.freedesktop.ModemManager1.Bearer` interface.

use super::*;

impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
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
                info!("Context configuration set");
                Ok(())
            }
            Err(e) => {
                error!("Context configuration not set: {:?}", e);
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
                info!("Context deactivated");
            }
            Err(_e) => {}
        }

        match self
            .client
            .send(&ActivatePDPContext { context_id: 1 })
            .await
        {
            Ok(_) => {
                info!("Context activated");
            }
            Err(e) => {
                error!("Context not activated: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        match self.client.send(&GetPDPContextInfo {}).await {
            Ok(status) => {
                info!("Context status: {:?}", status);
                let t = core::time::Duration::from_millis(compat::elapsed_ms(now));
                info!(
                    "IP {:?} obtained after {} s",
                    status.ip_address,
                    t.as_secs()
                );
                Ok(t)
            }
            Err(e) => {
                error!("Context status not found: {:?}", e);
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
                info!("Context deactivated");
                Ok(())
            }
            Err(e) => {
                error!("Context not deactivated: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Dial the modem into PPP data mode on the PDP context most recently
    /// configured via `set_context_configuration` (`ATD*99#`), for use with
    /// [`crate::ppp`]'s `embassy-net` integration.
    ///
    /// The context must already be configured and the modem network-attached
    /// (`set_context_configuration` + `network_attach`), same as before
    /// opening an AT-command-driven socket via [`crate::tcp`]. Unlike that
    /// path, once this returns `Ok(())` **every subsequent byte on the UART
    /// is raw PPP framing, not AT traffic** -- do not send further AT
    /// commands through this driver afterwards. Reclaim the raw serial halves
    /// (see [`Self::client_mut`] and [`crate::ppp::Reclaimable`]) and hand
    /// them to `embassy-net-ppp` instead.
    ///
    /// This is a one-way trip for this driver instance: there is no API here
    /// to escape back to AT command mode (real hardware supports it via a
    /// `+++` guard sequence, but wiring that back up is out of scope).
    ///
    /// The modem's `CONNECT` reply is textually identical to an unrelated,
    /// pre-existing URC (`Urc::FileDataModeStarted` -- see
    /// [`crate::quectel_atat::DialPpp`]'s docs for why), and atat always
    /// resolves URC matches before command responses, so it can never be
    /// observed as this command's ordinary response. This subscribes to the
    /// URC channel *before* sending (subscribers only see URCs published
    /// after they subscribe) and waits for that URC instead.
    #[cfg(feature = "ppp")]
    pub async fn dial_ppp(&mut self) -> Result<(), ModemError> {
        info!("Dialing PPP...");

        let mut subscriber = self.urc_channel.subscribe().unwrap();

        if let Err(e) = self.client.send(&DialPpp).await {
            error!("PPP dial failed to send: {:?}", e);
            return Err(ModemError::NotResponding);
        }

        let timeout_duration = compat::Duration::from_secs(30);
        let wait_result = compat::with_timeout(timeout_duration, async {
            loop {
                if let Urc::FileDataModeStarted = subscriber.next_message_pure().await {
                    return;
                }
            }
        })
        .await;

        match wait_result {
            Ok(()) => {
                info!("PPP link established");
                Ok(())
            }
            Err(_) => {
                error!("PPP dial timed out waiting for CONNECT");
                Err(ModemError::NotResponding)
            }
        }
    }
}
