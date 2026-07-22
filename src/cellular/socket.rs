//! Plain-TCP and TLS socket primitives (`AT+QIOPEN`/`QISEND`/`QIRD`/`QICLOSE`
//! and `AT+QSSLOPEN`/`QSSLSEND`/`QSSLRECV`/`QSSLCLOSE`), plus SSL context
//! setup. Wrapped by [`crate::tcp`] for the `embedded-nal-async`/
//! `embedded-io-async` surface most callers should use instead of these
//! directly.

use super::*;

impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
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

        // Subscribe *before* sending so the +QSSLOPEN URC can't be missed. This
        // subscription is kept for the socket's lifetime so subsequent
        // "recv"/"closed" URCs are seen too (see `socket_sub`).
        self.socket_sub = Some(
            self.urc_channel
                .subscribe()
                .map_err(|_| ModemError::NotResponding)?,
        );

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
            if let Some(sub) = self.socket_sub.as_mut() {
                if let Some(Urc::SslOpen(r)) = sub.try_next_message_pure() {
                    if r.client_id == client_id {
                        if r.err == 0 {
                            info!("SSL socket {} opened", client_id);
                            return Ok(());
                        }
                        error!("QSSLOPEN failed for socket {}: err={}", client_id, r.err);
                        return Err(ModemError::SocketOpenFailed);
                    }
                }
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
                    trace!("Sent {} bytes on socket {}", chunk.len(), client_id);
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

    /// Open a plain (non-TLS) TCP socket to `host:port`.
    ///
    /// A PDP context must be active ([`context_activate`](Self::context_activate)).
    /// The socket is opened in buffer access mode: use
    /// [`tcp_socket_recv`](Self::tcp_socket_recv) to read and
    /// [`tcp_socket_send`](Self::tcp_socket_send) to write. This is the
    /// counterpart of [`ssl_socket_open`](Self::ssl_socket_open) for connections
    /// that do not need modem-terminated TLS.
    pub async fn tcp_socket_open(
        &mut self,
        client_id: u8,
        host: &str,
        port: u16,
    ) -> Result<(), ModemError> {
        info!("Opening TCP socket {} to {}:{}", client_id, host, port);

        let mut subscriber = self.urc_channel.subscribe().unwrap();

        match self
            .client
            .send(&TcpOpen {
                ctx_id: 1,
                connect_id: client_id,
                service_type: HeaplessString::try_from("TCP").unwrap(),
                host: HeaplessString::try_from(host).map_err(|_| ModemError::NotSupported)?,
                port,
                local_port: 0,
                access_mode: 0, // buffer access mode
            })
            .await
        {
            Ok(_) => {}
            Err(e) => {
                error!("QIOPEN command failed: {:?}", e);
                return Err(ModemError::SocketOpenFailed);
            }
        }

        // The connect result arrives as the +QIOPEN URC.
        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < 30_000 {
            compat::delay_ms(200).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::TcpOpen(r)) if r.connect_id == client_id => {
                    if r.err == 0 {
                        info!("TCP socket {} opened", client_id);
                        return Ok(());
                    }
                    error!("QIOPEN failed for socket {}: err={}", client_id, r.err);
                    return Err(ModemError::SocketOpenFailed);
                }
                _ => {}
            }
        }

        error!("Timed out waiting for QIOPEN result");
        Err(ModemError::OperationTimeout)
    }

    /// Send `data` on an open plain-TCP socket.
    ///
    /// Data larger than one AT payload chunk (256 bytes) is split across multiple
    /// `AT+QISEND` operations.
    pub async fn tcp_socket_send(&mut self, client_id: u8, data: &[u8]) -> Result<(), ModemError> {
        if data.is_empty() {
            return Ok(());
        }
        if data.len() > u16::MAX as usize {
            return Err(ModemError::NotSupported);
        }

        for chunk in data.chunks(256) {
            // The `>` prompt after QISEND is not a regular AT response, so an
            // error here is expected and ignored (same pattern as QSSLSEND).
            let _ = self
                .client
                .send(&TcpSend {
                    connect_id: client_id,
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
                    trace!("Sent {} bytes on TCP socket {}", chunk.len(), client_id);
                }
                Err(e) => {
                    error!("QISEND payload failed on socket {}: {:?}", client_id, e);
                    return Err(ModemError::SocketSendFailed);
                }
            }
        }

        Ok(())
    }

    /// Read currently-buffered data from an open plain-TCP socket into `buf`.
    ///
    /// Returns the number of bytes read, which is `0` when the modem has no data
    /// buffered right now (callers typically wait for the
    /// `+QIURC: "recv",<id>` URC and retry). At most 512 bytes are returned per
    /// call regardless of `buf` length.
    pub async fn tcp_socket_recv(
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
            .send(&TcpRecv {
                connect_id: client_id,
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
                error!("QIRD failed on socket {}: {:?}", client_id, e);
                Err(ModemError::SocketRecvFailed)
            }
        }
    }

    /// Close an open plain-TCP socket.
    pub async fn tcp_socket_close(&mut self, client_id: u8) -> Result<(), ModemError> {
        info!("Closing TCP socket {}", client_id);
        match self
            .client
            .send(&TcpClose {
                connect_id: client_id,
                timeout: 10,
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("QICLOSE failed on socket {}: {:?}", client_id, e);
                Err(ModemError::SocketCloseFailed)
            }
        }
    }

    /// Non-blocking drain of the URC channel for a socket `"closed"` event.
    ///
    /// Returns `true` if a `+QIURC: "closed",<id>` (plain TCP) or
    /// `+QSSLURC: "closed",<id>` (TLS) event for `client_id` is currently
    /// queued. Used by the socket wrappers to surface peer-close as EOF.
    ///
    /// NOTE: this consumes URCs from a freshly-created subscriber, so it only
    /// observes events that arrive while a subscriber exists. It is best-effort:
    /// a `"closed"` event delivered between calls (with no subscriber alive) is
    /// missed. The socket wrappers therefore also use an idle timeout as a
    /// backstop.
    pub async fn socket_poll_closed(&mut self, client_id: u8) -> bool {
        let mut subscriber = match self.urc_channel.subscribe() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let mut closed = false;
        while let Some(urc) = subscriber.try_next_message_pure() {
            match urc {
                Urc::TcpUrc(u) if u.connect_id == client_id && u.urc_type.contains("closed") => {
                    closed = true;
                }
                Urc::SslUrc(u) if u.client_id == client_id && u.urc_type.contains("closed") => {
                    closed = true;
                }
                _ => {}
            }
        }
        closed
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
}
