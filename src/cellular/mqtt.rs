//! MQTT client -- the analogue of a ModemManager-style bearer extension for
//! Quectel's modem-side MQTT stack (`AT+QMTOPEN`/`QMTCONN`/`QMTPUBEX`/
//! `QMTDISC`).

use super::*;

impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
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
                info!("Connected to MQTT broker");
            }
            Err(e) => {
                error!("MQTT broker not connected: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 10_000 {
            compat::delay_ms(500).await;

            match subscriber.try_next_message_pure() {
                Some(Urc::MqttOpen(mqtopen_response)) => {
                    info!("MQTT Open response: result={}", mqtopen_response.result);
                    match mqtopen_response.result {
                        0 => {
                            info!("Connection opened");
                            break;
                        }
                        -1 => {
                            error!("MQTT Open failed: network connection failed");
                            return Err(ModemError::NoNetwork);
                        }
                        1 => {
                            error!("MQTT Open failed: wrong parameter");
                            return Err(ModemError::MqttRequestFailed);
                        }
                        2 => {
                            error!("MQTT Open failed: MQTT identifier occupied");
                            return Err(ModemError::MqttRequestFailed);
                        }
                        3 => {
                            error!("MQTT Open failed: PDP activation failed");
                            return Err(ModemError::NoContext);
                        }
                        4 => {
                            error!("MQTT Open failed: DNS parse failed");
                            if port == 8883 {
                                return Err(ModemError::SslHostnameMismatch);
                            }
                            return Err(ModemError::MqttRequestFailed);
                        }
                        5 => {
                            error!("MQTT Open failed: network disconnection");
                            if port == 8883 {
                                return Err(ModemError::SslCertificateInvalid);
                            }
                            return Err(ModemError::NoNetwork);
                        }
                        _ => {
                            error!(
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
                    debug!("Received other URC, waiting for MQTT Open");
                }
                None => {
                    debug!("Waiting for response...");
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
                info!("Connected to MQTT broker");
            }
            Err(e) => {
                error!("MQTT broker not connected: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 5_000 {
            compat::delay_ms(500).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::MqttConnect(mqtconnect_response)) => {
                    info!(
                        "MQTT Connect response: result={}, ret_code={}",
                        mqtconnect_response.result, mqtconnect_response.ret_code
                    );
                    match mqtconnect_response.result {
                        0 => {
                            // Packet sent successfully, now check ret_code
                            match mqtconnect_response.ret_code {
                                0 => {
                                    info!("Client connected");
                                    break;
                                }
                                1 => {
                                    error!("Connection refused: unacceptable protocol version");
                                    return Err(ModemError::MqttRequestFailed);
                                }
                                2 => {
                                    error!("Connection refused: identifier rejected");
                                    return Err(ModemError::MqttRequestFailed);
                                }
                                3 => {
                                    error!("Connection refused: server unavailable");
                                    return Err(ModemError::MqttRequestFailed);
                                }
                                4 => {
                                    error!("Connection refused: bad user name or password");
                                    return Err(ModemError::MqttRequestFailed);
                                }
                                5 => {
                                    error!("Connection refused: not authorized");
                                    return Err(ModemError::MqttRequestFailed);
                                }
                                _ => {
                                    error!(
                                        "Connection refused with unknown ret_code={}",
                                        mqtconnect_response.ret_code
                                    );
                                    return Err(ModemError::MqttRequestFailed);
                                }
                            }
                        }
                        _ => {
                            error!(
                                "MQTT Connect failed with result={}",
                                mqtconnect_response.result
                            );
                            return Err(ModemError::MqttRequestFailed);
                        }
                    }
                }
                Some(_) => {
                    debug!("Received other URC, waiting for MQTT Connect");
                }
                None => {
                    debug!("Waiting for response...");
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
                info!("Disconnected from MQTT broker");
            }
            Err(e) => {
                error!("MQTT broker not disconnected: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 10_000 {
            compat::delay_ms(500).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::MqttDisconnect(mqtdisconnect_response)) => {
                    info!(
                        "MQTT Disconnect response: result={}",
                        mqtdisconnect_response.result
                    );
                    match mqtdisconnect_response.result {
                        0 => {
                            info!("Client disconnected");
                            break;
                        }
                        _ => {
                            error!(
                                "MQTT Disconnect failed with result={}",
                                mqtdisconnect_response.result
                            );
                            return Err(ModemError::MqttRequestFailed);
                        }
                    }
                }
                Some(_) => {
                    debug!("Received other URC, waiting for MQTT Disconnect");
                }
                None => {
                    debug!("Waiting for response...");
                }
            }
        }

        if ActiveChip::needs_explicit_mqtt_close(self.rev) {
            let now = compat::Instant::now();
            while compat::elapsed_ms(now) < 5_000 {
                compat::delay_ms(500).await;
                match subscriber.try_next_message_pure() {
                    Some(Urc::MqttStatus(mqttstatus_response)) => {
                        info!("MQTT Status response: err={}", mqttstatus_response.err);
                        match mqttstatus_response.err {
                            5 => {
                                info!("Client disconnected");
                                return Ok(());
                            }
                            _ => {
                                error!("MQTT Status failed with err={}", mqttstatus_response.err);
                                return Err(ModemError::MqttRequestFailed);
                            }
                        }
                    }
                    Some(_) => {
                        debug!("Received other URC, waiting for MQTT Status");
                    }
                    None => {
                        debug!("Waiting for response...");
                    }
                }
            }

            match self.client.send(&MqttClose { tcp_connect_id: 0 }).await {
                Ok(_) => {
                    info!("Disconnected from MQTT broker");
                }
                Err(e) => {
                    error!("MQTT broker not disconnected: {:?}", e);
                    return Err(ModemError::NotResponding);
                }
            }

            let now = compat::Instant::now();
            while compat::elapsed_ms(now) < 5_000 {
                compat::delay_ms(500).await;
                match subscriber.try_next_message_pure() {
                    Some(Urc::MqttClose(mqtclose_response)) => {
                        info!("MQTT Close response: result={}", mqtclose_response.result);
                        match mqtclose_response.result {
                            0 => {
                                info!("Connection closed");
                                return Ok(());
                            }
                            _ => {
                                error!(
                                    "MQTT Close failed with result={}",
                                    mqtclose_response.result
                                );
                                return Err(ModemError::MqttRequestFailed);
                            }
                        }
                    }
                    Some(_) => {
                        debug!("Received other URC, waiting for MQTT Close");
                    }
                    None => {
                        debug!("Waiting for response...");
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
                info!("Published to MQTT broker");
            }
            Err(e) => {
                error!("MQTT broker not published: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 10_000 {
            compat::delay_ms(500).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::MqttPublish(mqtpublish_response)) => {
                    info!(
                        "MQTT Publish response: result={}",
                        mqtpublish_response.result
                    );
                    match mqtpublish_response.result {
                        0 => {
                            info!("Publishing successful");
                            break;
                        }
                        _ => {
                            error!(
                                "Publishing failed with result={}",
                                mqtpublish_response.result
                            );
                            return Err(ModemError::MqttRequestFailed);
                        }
                    }
                }
                Some(_) => {
                    debug!("Received other URC, waiting for MQTT Publish");
                }
                None => {
                    debug!("Waiting for response...");
                }
            }
        }

        Ok(())
    }
}
