//! GNSS control -- the analogue of ModemManager's
//! `org.freedesktop.ModemManager1.Modem.Location` interface.

use super::*;

impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
    pub async fn configure_gnss_priority(&mut self) -> Result<(), ModemError> {
        match self.client.send(&ConfigureGnssPriorityMode).await {
            Ok(_) => {}
            Err(e) => {
                error!("Setting GNSS priority failed ({:?})", e);
                return Err(ModemError::NotResponding);
            }
        }

        match self
            .client
            .send(&SetGnssConstellation {
                param: HeaplessString::try_from("gnssconfig").unwrap(),
                constellation: GnssConstellation::Galileo,
            })
            .await
        {
            Ok(_) => {}
            Err(e) => {
                error!("Setting GNSS constellation failed ({:?})", e);
                return Err(ModemError::NotResponding);
            }
        }

        Ok(())
    }

    pub async fn turn_on_gnss(
        &mut self,
        operating_mode: GnssOperatingMode,
    ) -> Result<(), ModemError> {
        match self
            .client
            .send(&TurnOnGnss {
                mode: operating_mode,
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Unable to turn on GNSS ({:?})", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    pub async fn turn_off_gnss(&mut self) -> Result<(), ModemError> {
        match self.client.send(&TurnOffGnss).await {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Unable to turn on GNSS ({:?})", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    pub async fn get_gnss_location(
        &mut self,
    ) -> Result<GnssPositionInformationResponse, ModemError> {
        match self.client.send(&GetGgaNmeaSentence).await {
            Ok(response) => {
                if response.quality == 0 {
                    return Err(ModemError::GnssNotFixed);
                }
            }
            Err(_) => {
                return Err(ModemError::NotResponding);
            }
        }

        match self.client.send(&GetGnssPositionInformation).await {
            Ok(data) => Ok(data),
            Err(e) => {
                error!("Unexpected error occurred: {:?}", e);
                Err(ModemError::NotResponding)
            }
        }
    }
}
