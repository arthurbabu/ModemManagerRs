//! Internal (UFS) flash file operations -- used to upload/read/write/delete
//! SSL CA certs and client credentials. No ModemManager equivalent (MM has no
//! notion of modem-side file storage); this is Quectel-specific.

use super::*;

impl<W: Write, OutputPinGeneric: OutputPin> QuectelBG9X<W, OutputPinGeneric> {
    pub async fn delete_file_from_internal_flash(
        &mut self,
        filename: &str,
    ) -> Result<(), ModemError> {
        match self
            .client
            .send(&DeleteFileFromInternalFlash {
                file_path: HeaplessString::try_from(filename).unwrap(),
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Unable to delete file ({:?})", e);
                Err(ModemError::FileDeletionFailed)
            }
        }
    }

    /// List all files from internal flash storage using AT+QFLST command.
    ///
    /// This function retrieves the list of all files stored in the UFS (User File Storage).
    /// It returns a vector of tuples containing the filename and file size in bytes.
    ///
    /// # Returns
    ///
    /// * `Ok(heapless::Vec<(HeaplessString<80>, u32), 5>)` - A vector of tuples with filename and size
    /// * `Err(ModemError)` - If the command fails or no files are found
    pub async fn get_all_files_list_from_internal_flash(
        &mut self,
    ) -> Result<atat::heapless::Vec<(HeaplessString<80>, u32), 5>, ModemError> {
        // Use "*" pattern to list all files in UFS
        match self
            .client
            .send(&ListFilesFromInternalFlash {
                name_pattern: HeaplessString::try_from("*").unwrap(),
            })
            .await
        {
            Ok(response) => {
                // Convert the response entries to a heapless Vec of (name, size) tuples
                let mut files: atat::heapless::Vec<(HeaplessString<80>, u32), 5> =
                    atat::heapless::Vec::new();
                for entry in response.files.iter() {
                    // The response Vec has the same capacity, so pushing cannot overflow.
                    let _ = files.push((entry.filename.clone(), entry.file_size));
                }
                Ok(files)
            }
            Err(e) => {
                error!("Unable to list files ({:?})", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Get metadata for a single file from internal flash storage using AT+QFLST command.
    ///
    /// This function retrieves the metadata (filename and size) for a specific file
    /// stored in the UFS (User File Storage).
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the file to query
    ///
    /// # Returns
    ///
    /// * `Ok((HeaplessString<80>, u32))` - A tuple with filename and size in bytes
    /// * `Err(ModemError)` - If the command fails or the file is not found
    pub async fn get_file_meta_from_internal_flash(
        &mut self,
        filename: &str,
    ) -> Result<(HeaplessString<80>, u32), ModemError> {
        match self
            .client
            .send(&ListFilesFromInternalFlash {
                name_pattern: HeaplessString::try_from(filename).unwrap(),
            })
            .await
        {
            Ok(response) => {
                // Check if we got exactly one file
                if response.files.is_empty() {
                    error!("File not found: {}", filename);
                    return Err(ModemError::FileUploadFailed);
                }

                // Return the first (and should be only) file's metadata
                let entry = &response.files[0];
                Ok((entry.filename.clone(), entry.file_size))
            }
            Err(e) => {
                error!("Unable to get file metadata ({:?})", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Upload a file to internal flash storage using AT+QFUPL command.
    ///
    /// This function uploads a binary file to the UFS (User File Storage) of the modem.
    /// It sends the file in raw binary mode after initiating the upload command.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the file to upload to internal storage
    /// * `data` - The binary data of the file to upload
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the file is uploaded successfully
    /// * `Err(ModemError)` - If the command fails or the upload is unsuccessful
    pub async fn upload_file_to_internal_flash(
        &mut self,
        filename: &str,
        data: &[u8],
    ) -> Result<(), ModemError> {
        let len = data.len() as u32;
        const TIMEOUT: u16 = 3;

        match self
            .client
            .send(&FileUploadToInternalFlash {
                file_path: HeaplessString::try_from(filename).unwrap(),
                file_size: len,
                timeout: TIMEOUT.into(),
                ack_mode: None,
            })
            .await
        {
            Ok(_) => {}
            Err(_e) => {
                // error!("Unable to start file upload ({:?})", e);
                // return Err(ModemError::NotResponding);
            }
        }

        // TODO: deal with the CONNECT URC.
        // For now, just wait enought time for the modem to be ready.
        compat::delay_ms(300).await;

        // Uploading file contents. It must be done in chunks of less than INGRESS_BUFFER_SIZE.
        // We don't know the exact size of the ingress buffer, so we use 128 bytes as a safe value.
        trace!("Uploading file...");
        for chunk in data.chunks(256) {
            match self
                .client
                .send(&SendRawContents {
                    bytes: HeaplessBytes::try_from(chunk).unwrap(),
                })
                .await
            {
                Ok(_) => {
                    trace!("Uploaded {} bytes", chunk.len());
                }
                Err(e) => {
                    error!("Error uploading file chunk ({:?})", e);
                    return Err(ModemError::FileUploadFailed);
                }
            }
        }

        //
        let mut subscriber = self.urc_channel.subscribe().unwrap();
        let now = compat::Instant::now();
        while compat::elapsed_ms(now) < (TIMEOUT as u64) * 1000 {
            compat::delay_ms(500).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::FileUploadDone(upload_response)) => {
                    debug!("File Upload response: {:?}", upload_response);
                    if upload_response.upload_size != len {
                        error!("Upload size mismatch");
                        return Err(ModemError::FileUploadFailed);
                    }
                }
                Some(e) => {
                    error!("Unknown URC {:?}", e);
                }
                None => {
                    debug!("Waiting for response...");
                }
            }
        }

        Ok(())
    }

    /// Download a file from internal flash storage using AT+QFDWL command.
    ///
    /// This function retrieves a file stored in the UFS (User File Storage).
    /// The modem responds with CONNECT, then outputs the binary data, and finally
    /// sends +QFDWL response with download_size and checksum.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the file to download from internal storage
    /// * `buffer` - A mutable buffer to store the downloaded file data
    ///
    /// # Returns
    ///
    /// * `Ok(usize)` - The number of bytes downloaded
    /// * `Err(ModemError)` - If the command fails or the file cannot be downloaded
    ///
    /// # Note
    ///
    /// This implementation has limitations with the current atat framework for handling
    /// binary data mode. The actual binary data reading would need to be implemented
    /// at a lower level to properly capture the data between CONNECT and +QFDWL response.
    pub async fn work_in_progress_download_file_from_internal_flash(
        &mut self,
        filename: &str,
        buffer: &mut [u8],
    ) -> Result<usize, ModemError> {
        // Send the download command - the response includes the +QFDWL line with size and checksum
        match self
            .client
            .send(&DownloadFileFromInternalFlash {
                file_path: HeaplessString::try_from(filename).unwrap(),
            })
            .await
        {
            Ok(response) => {
                info!(
                    "File download completed: size={}, checksum={}",
                    response.download_size, response.checksum
                );

                let downloaded_size = response.download_size as usize;

                if downloaded_size > buffer.len() {
                    error!("Buffer too small for downloaded file");
                    return Err(ModemError::FileUploadFailed);
                }

                // TODO: The modem sends CONNECT, then binary data, then +QFDWL response.
                // With the current atat framework, we cannot easily capture the raw binary data
                // between CONNECT and the +QFDWL response. A proper implementation would need to:
                // 1. Detect CONNECT response
                // 2. Read exactly download_size bytes of binary data from the serial port
                // 3. Validate the checksum
                //
                // For now, we just return the expected size from the +QFDWL response.
                // The binary data is currently being consumed by the atat ingress but not captured.
                Ok(downloaded_size)
            }
            Err(e) => {
                error!("Unable to download file ({:?})", e);
                Err(ModemError::NotResponding)
            }
        }
    }

    /// Read a file from internal flash storage using AT+QFOPEN and AT+QFREAD commands.
    ///
    /// This function opens a file, reads its contents in fixed-size blocks (1024 bytes),
    /// and closes it. The binary data is copied to the provided buffer.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the file to read from internal storage
    /// * `buffer` - A mutable buffer to store the file data
    ///
    /// # Returns
    ///
    /// * `Ok(usize)` - The number of bytes read
    /// * `Err(ModemError)` - If the command fails or the file cannot be read
    pub async fn read_file_from_internal_flash(
        &mut self,
        filename: &str,
        buffer: &mut [u8],
    ) -> Result<usize, ModemError> {
        // Open the file in read-only mode (mode 2)
        let filehandle = match self
            .client
            .send(&OpenFile {
                filename: HeaplessString::try_from(filename).unwrap(),
                mode: Some(2), // Read only
            })
            .await
        {
            Ok(response) => {
                info!("File opened with handle: {}", response.filehandle);
                response.filehandle
            }
            Err(e) => {
                error!("Unable to open file ({:?})", e);
                return Err(ModemError::FileUploadFailed);
            }
        };

        let mut total_bytes_read = 0usize;
        const BLOCK_SIZE: usize = 128;

        // Read file in blocks
        while total_bytes_read < buffer.len() {
            let remaining = buffer.len() - total_bytes_read;
            let read_size = core::cmp::min(remaining, BLOCK_SIZE);

            // Read a block from the file
            match self
                .client
                .send(&ReadFile {
                    filehandle,
                    length: Some(read_size as u32),
                })
                .await
            {
                Ok(response) => {
                    info!("Read {} bytes from file", response.read_length);

                    // Check if we reached end of file
                    if response.read_length == 0 {
                        break;
                    }

                    // Copy data to user buffer
                    let bytes_to_copy =
                        core::cmp::min(response.read_length as usize, response.data.len());
                    let bytes_to_copy = core::cmp::min(bytes_to_copy, remaining);

                    buffer[total_bytes_read..total_bytes_read + bytes_to_copy]
                        .copy_from_slice(&response.data[..bytes_to_copy]);

                    total_bytes_read += bytes_to_copy;

                    // If we read less than requested, we've reached end of file
                    if response.read_length < read_size as u32 {
                        break;
                    }
                }
                Err(e) => {
                    error!("Unable to read file ({:?})", e);
                    // Close the file even if read failed
                    let _ = self.client.send(&CloseFile { filehandle }).await;
                    return Err(ModemError::FileUploadFailed);
                }
            }
        }

        // Close the file
        match self.client.send(&CloseFile { filehandle }).await {
            Ok(_) => {
                info!("File closed, read {} bytes total", total_bytes_read);
            }
            Err(e) => {
                error!("Unable to close file ({:?})", e);
                return Err(ModemError::FileUploadFailed);
            }
        }

        Ok(total_bytes_read)
    }

    /// Write data to a file in internal flash storage using AT+QFOPEN and AT+QFWRITE commands.
    ///
    /// This function opens a file (creating it if it doesn't exist or overwriting if it does),
    /// writes the data, and closes it.
    ///
    /// # Arguments
    ///
    /// * `filename` - The name of the file to write to internal storage
    /// * `data` - The data to write to the file
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the file was written successfully
    /// * `Err(ModemError)` - If the command fails or the file cannot be written
    pub async fn write_file_to_internal_flash(
        &mut self,
        filename: &str,
        data: &[u8],
    ) -> Result<(), ModemError> {
        // Open the file in create/overwrite mode (mode 1)
        let filehandle = match self
            .client
            .send(&OpenFile {
                filename: HeaplessString::try_from(filename).unwrap(),
                mode: Some(1), // Create/overwrite
            })
            .await
        {
            Ok(response) => {
                debug!("File opened with handle: {}", response.filehandle);
                response.filehandle
            }
            Err(e) => {
                error!("Unable to open file ({:?})", e);
                return Err(ModemError::FileUploadFailed);
            }
        };

        // Send write command - it will respond with CONNECT but we ignore errors
        // because the command won't complete until we send the data
        match self
            .client
            .send(&WriteFile {
                filehandle,
                length: data.len() as u32,
                timeout: Some(10), // 10 seconds timeout
            })
            .await
        {
            Ok(_) => {}
            Err(_) => {
                // Expected to timeout or get an error since CONNECT is sent as URC
                // and the command is waiting for data
            }
        }

        // Wait a bit for the modem to enter data mode after CONNECT
        compat::delay_ms(300).await;

        // Uploading file contents. It must be done in chunks of less than INGRESS_BUFFER_SIZE.
        // We don't know the exact size of the ingress buffer, so we use 128 bytes as a safe value.
        trace!("Uploading file...");
        for chunk in data.chunks(256) {
            match self
                .client
                .send(&SendRawContents {
                    bytes: HeaplessBytes::try_from(chunk).unwrap(),
                })
                .await
            {
                Ok(_) => {
                    trace!("Uploaded {} bytes", chunk.len());
                }
                Err(e) => {
                    error!("Error uploading file chunk ({:?})", e);
                    return Err(ModemError::FileUploadFailed);
                }
            }
        }

        // Wait for the write completion URC
        let mut subscriber = self.urc_channel.subscribe().unwrap();
        let now = compat::Instant::now();
        let timeout_ms = 10_000u64;

        while compat::elapsed_ms(now) < timeout_ms {
            compat::delay_ms(100).await;
            match subscriber.try_next_message_pure() {
                Some(Urc::FileWriteDone(write_response)) => {
                    info!(
                        "File write completed: written={}, total={}",
                        write_response.written_length, write_response.total_length
                    );

                    if write_response.written_length != data.len() as u32 {
                        error!("Write size mismatch");
                        let _ = self.client.send(&CloseFile { filehandle }).await;
                        return Err(ModemError::FileUploadFailed);
                    }
                    break;
                }
                Some(e) => {
                    debug!("Received URC: {:?}", e);
                }
                None => {
                    // Continue waiting
                }
            }
        }

        // Close the file
        match self.client.send(&CloseFile { filehandle }).await {
            Ok(_) => {
                debug!("File closed");
                Ok(())
            }
            Err(e) => {
                error!("Unable to close file ({:?})", e);
                Err(ModemError::FileUploadFailed)
            }
        }
    }
}
