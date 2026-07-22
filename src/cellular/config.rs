//! Modem functionality, band/RAT configuration and network time -- the
//! analogue of ModemManager's `Modem.SetPowerState`/`Modem.Time` interfaces.
//!
//! Per-chip band/RAT/service-domain AT commands live in
//! [`crate::chip`] (see [`ChipProfile::configure_modem`]); this module only
//! owns the chip-agnostic orchestration and network-time parsing.

use super::*;

impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
    pub async fn set_modem_funcionality(&mut self, on: bool) -> Result<(), ModemError> {
        match self
            .client
            .send(&SetUeFunctionality {
                fun: match on {
                    true => FunctionalityLevelOfUE::Full,
                    false => FunctionalityLevelOfUE::Minimum,
                },
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Modem functionality not set: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    pub async fn set_modem_configuration(
        &mut self,
        configuration: ModemConfiguration,
    ) -> Result<(), ModemError> {
        ActiveChip::configure_modem(&mut self.client, &configuration).await
    }

    pub async fn get_nitz_time(&mut self) -> Result<i64, ModemError> {
        match self.client.send(&GetNetworkNitzTime { mode: 1 }).await {
            Ok(network_time_info) => {
                info!("Network time: {:?}", network_time_info);
                get_timestamp_from_nitz_response(&network_time_info.time_and_dst)
            }
            Err(e) => {
                error!("Network time not found: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    pub async fn get_ntp_time(&mut self, ntp_server: &str) -> Result<(i64, u64), ModemError> {
        match self
            .client
            .send(&GetNetworkNtpTime {
                context_id: 1,
                server: HeaplessString::try_from(ntp_server).unwrap(),
            })
            .await
        {
            Ok(_) => {
                info!("NTP request sent");
            }
            Err(e) => {
                error!("Network time not found: {:?}", e);
                return Err(ModemError::NotResponding);
            }
        }

        let mut subscriber = self.urc_channel.subscribe().unwrap();

        // Wrap the await in a 10-second timeout.
        let timeout_duration = compat::Duration::from_secs(10);

        let wait_result = compat::with_timeout(timeout_duration, async {
            loop {
                // Task sleeps here with zero CPU usage until a message arrives.
                let msg = subscriber.next_message_pure().await;

                // CAPTURE TIME IMMEDIATELY for highest precision
                let exact_cpt = compat::Instant::now().as_ticks();

                match msg {
                    Urc::NtpTime(res) => {
                        if res.err != 0 {
                            error!("NTP failed");
                            return Err(ModemError::NtpRequestFailed);
                        }

                        info!("Network time: {:?}", res.time);
                        let ts = get_timestamp_from_ntp_response(&res.time)?;
                        return Ok((ts, exact_cpt));
                    }
                    e => {
                        // Ignore unknown URCs and let the loop await the next message
                        error!("Unknown URC {:?}", e);
                    }
                }
            }
        })
        .await;

        // Handle the result of the timeout wrapper
        match wait_result {
            Ok(Ok(val)) => return Ok(val), // Success: received NTP and parsed correctly
            Ok(Err(e)) => return Err(e),   // Error: NTP explicitly failed (e.g. NtpRequestFailed)
            Err(_) => {
                // Error: the 10-second timeout elapsed
                error!("NTP wait timed out");
            }
        }

        Err(ModemError::NotResponding)
    }

    pub async fn get_signal_strength(&mut self) -> Result<(i16, u8), ModemError> {
        match self.client.send(&GetSignalStrength).await {
            Ok(signal_strength) => {
                if let Some(rssi) = signal_strength.rssi {
                    let signal = rssi.clamp(-140, -30);
                    let signal = -100 * (signal + 140) / (-140 + 30);
                    info!("RSSI: {}dB ({}%)", rssi, signal);
                    Ok((rssi, signal as u8))
                } else {
                    Err(ModemError::NoNetwork)
                }
            }
            Err(e) => {
                error!("Signal strength not found: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }
}

/// Convert NTP datetime string to Unix timestamp.
fn get_timestamp_from_ntp_response(dt_str: &str) -> Result<i64, ModemError> {
    // Ignore timezone (indicates the difference, expressed in quarters of an hour, between the local time and GMT)
    // Turn "1970/01/01,00:00:00+00" into "1970-01-01T00:00:00Z"
    let fd = time::macros::format_description!(
        "[year]/[month]/[day],[hour]:[minute]:[second][ignore count:1][ignore count:2]"
    );
    let dt = time::PrimitiveDateTime::parse(dt_str, fd).map_err(|e| {
        #[cfg(feature = "defmt")]
        error!("Failed to parse date time string: {:#?}", Debug2Format(&e));

        #[cfg(not(feature = "defmt"))]
        error!("Failed to parse date time string: {:?}", e);

        ModemError::NtpRequestFailed
    })?;
    let dt = dt.assume_offset(time::UtcOffset::UTC);

    let ts = dt.unix_timestamp();
    info!("Timestamp: {}", ts);

    Ok(ts)
}

/// Convert NITZ datetime string to Unix timestamp.
fn get_timestamp_from_nitz_response(nitz_str: &str) -> Result<i64, ModemError> {
    // Ignore timezone (indicates the difference, expressed in quarters of an hour, between the local time and GMT)
    // Ignore Daylight saving time
    // Turn "1970/01/01,00:00:00+00,0" into "1970-01-01T00:00:00Z"
    let fd = time::macros::format_description!(
        "[year]/[month]/[day],[hour]:[minute]:[second][ignore count:1][ignore count:2],[ignore count:1]"
    );
    let dt = time::PrimitiveDateTime::parse(nitz_str, fd).unwrap();
    let dt = dt.assume_offset(time::UtcOffset::UTC);

    let ts = dt.unix_timestamp();
    info!("Timestamp: {}", ts);
    Ok(ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_ntp_time_to_unix_timestamp() {
        let dt_str = "1970/01/01,00:00:00+00";
        let ts = get_timestamp_from_ntp_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 0);

        let dt_str = "2025/11/11,19:39:05+00";
        let ts = get_timestamp_from_ntp_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);
    }

    #[test]
    fn test_convert_ntp_time_to_unix_timestamp_ignore_timezone() {
        let dt_str = "2025/11/11,19:39:05+04";
        let ts = get_timestamp_from_ntp_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);

        let dt_str = "2025/11/11,19:39:05-04";
        let ts = get_timestamp_from_ntp_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);
    }

    #[test]
    fn test_convert_nitz_time_to_unix_timestamp() {
        let dt_str = "1970/01/01,00:00:00+00,0";
        let ts = get_timestamp_from_nitz_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 0);

        let dt_str = "2025/11/11,19:39:05+00,1";
        let ts = get_timestamp_from_nitz_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);
    }

    #[test]
    fn test_convert_nitz_time_to_unix_timestamp_ignore_timezone() {
        let dt_str = "2025/11/11,19:39:05+04,0";
        let ts = get_timestamp_from_nitz_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);

        let dt_str = "2025/11/11,19:39:05-04,1";
        let ts = get_timestamp_from_nitz_response(dt_str);
        assert!(ts.is_ok());
        assert_eq!(ts.unwrap(), 1762889945);
    }
}
