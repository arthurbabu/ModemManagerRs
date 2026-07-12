use std::{env, thread, time};

use quectel_bg9x_eh_driver::cellular::{
    QuectelBG9X, INGRESS_BUF_SIZE, URC_CAPACITY, URC_SUBSCRIBERS,
};
use quectel_bg9x_eh_driver::quectel_atat::urc::Urc;

use atat::blocking::Client;
use atat::AtatIngress;
use atat::DefaultDigester;
use atat::Ingress;
use atat::{Config as AtatConfig, ResponseSlot, UrcChannel};

use embedded_hal_mock::eh1::digital::{
    Mock as PinMock, State as PinState, Transaction as PinTransaction,
};
use embedded_io::Read;
use static_cell::StaticCell;

#[toml_cfg::toml_config]
struct Config {
    #[default("")]
    modem_apn: &'static str,
    #[default("")]
    modem_user: &'static str,
    #[default("")]
    modem_pass: &'static str,

    #[default("test.mosquitto.org")]
    mqtt_server: &'static str,
    #[default(1883)]
    mqtt_port: u16,
    #[default("")]
    mqtt_user: &'static str,
    #[default("")]
    mqtt_pass: &'static str,

    #[default("")]
    serial_port: &'static str,
}

fn main() {
    let serial_port = env::args().nth(1).expect("Usage: cargo run <device>");
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    // Configure expectations
    let expectations = [
        PinTransaction::set(PinState::High),
        PinTransaction::set(PinState::Low),
    ];
    // Create pin
    let mut modem_pwr_key = PinMock::new(&expectations);

    // Open serial port
    let serial_tx = serialport::new(serial_port, 115_200)
        .timeout(std::time::Duration::from_millis(1000))
        .open()
        .expect("Could not open serial port");
    let serial_rx = serial_tx.try_clone().expect("Could not clone serial port");

    let serial_tx = embedded_io_adapters::std::FromStd::new(serial_tx);
    let mut serial_rx = embedded_io_adapters::std::FromStd::new(serial_rx);

    static INGRESS_BUF: StaticCell<[u8; INGRESS_BUF_SIZE]> = StaticCell::new();
    static RES_SLOT: ResponseSlot<INGRESS_BUF_SIZE> = ResponseSlot::new();
    static URC_CHANNEL: UrcChannel<Urc, URC_CAPACITY, URC_SUBSCRIBERS> = UrcChannel::new();
    let mut ingress = Ingress::new(
        DefaultDigester::<Urc>::default(),
        INGRESS_BUF.init([0; INGRESS_BUF_SIZE]),
        &RES_SLOT,
        &URC_CHANNEL,
    );

    static BUF: StaticCell<[u8; 1024]> = StaticCell::new();
    let buf = BUF.init([0; 1024]);

    let client = Client::new(serial_tx, &RES_SLOT, buf, AtatConfig::default());

    log::info!("Starting ATAT loop...");
    let _ = std::thread::spawn(move || loop {
        let buf = ingress.write_buf();
        match serial_rx.read(buf) {
            Ok(len) => {
                if len != 0 {
                    // let s = std::str::from_utf8(&buf[..len]).unwrap();
                    // log::debug!("Read: ({}) {}", len, s);
                }
                match ingress.try_advance(len) {
                    Ok(_) => {}
                    Err(e) => {
                        log::info!("Error advancing ingress {:?}", e);
                        ingress.clear();
                    }
                }
            }
            Err(e) => {
                if e.kind() != std::io::ErrorKind::TimedOut {
                    log::info!("Error reading from UART: {:?}", e);
                }
                ingress.clear();
            }
        }
    });
    // end of atat initialization

    log::info!("Starting ATAT client...");
    let mut modem = match QuectelBG9X::new(modem_pwr_key.clone(), client, &URC_CHANNEL) {
        Ok(modem) => modem,
        Err(e) => {
            log::error!("Error initializing modem: {:?}", e);
            loop {
                thread::sleep(time::Duration::from_millis(1000));
            }
        }
    };

    let files = modem.get_all_files_list_from_internal_flash().unwrap();

    println!("Files in internal flash:");
    for (filename, size) in &files {
        println!("File: {}, Size: {} bytes", filename, size);
    }
    println!("\n");

    if let Some((_, size)) = files.iter().find(|(name, _)| *name == "cert.pem") {
        println!("Found cert.pem with size {} bytes", size);
        let mut file_data = vec![0u8; *size as usize];
        modem
            .read_file_from_internal_flash("cert.pem", &mut file_data)
            .unwrap();
        let content = std::str::from_utf8(&file_data).unwrap();
        println!("cert.pem content: {}", content);

        println!("Deleting cert.pem...");
        modem.delete_file_from_internal_flash("cert.pem").unwrap();
    } else {
        println!("cert.pem not found in internal flash.");
    }

    let files = modem.get_all_files_list_from_internal_flash().unwrap();

    println!("Files in internal flash:");
    for (filename, size) in &files {
        println!("File: {}, Size: {} bytes", filename, size);
    }
    println!("\n");

    println!("Uploading cert.pem...");
    let cert: &[u8] = "Adios, rios; adios, fontes;".as_bytes();
    modem
        .upload_file_to_internal_flash("cert.pem", cert)
        .unwrap();

    let files = modem.get_all_files_list_from_internal_flash().unwrap();

    println!("Files in internal flash:");
    for (filename, size) in &files {
        println!("File: {}, Size: {} bytes", filename, size);
    }
    println!("\n");

    // Demonstrate write_file_to_internal_flash
    println!("Writing data to test_write.txt using write_file_to_internal_flash...");
    let write_data = b"Hello from write_file_to_internal_flash!\nThis is line 2.\nThis is line 3.";
    modem
        .write_file_to_internal_flash("test_write.txt", write_data)
        .unwrap();
    println!("Successfully wrote {} bytes to test_write.txt", write_data.len());

    // List files again to confirm
    let files = modem.get_all_files_list_from_internal_flash().unwrap();
    println!("\nFiles in internal flash after write:");
    for (filename, size) in &files {
        println!("File: {}, Size: {} bytes", filename, size);
    }
    println!("\n");

    // Demonstrate read_file_from_internal_flash
    println!("Reading test_write.txt using read_file_from_internal_flash...");
    let mut read_buffer = vec![0u8; 256]; // Allocate buffer for reading
    let bytes_read = modem
        .read_file_from_internal_flash("test_write.txt", &mut read_buffer)
        .unwrap();
    println!("Successfully read {} bytes from test_write.txt", bytes_read);
    let content = std::str::from_utf8(&read_buffer[..bytes_read]).unwrap();
    println!("Content:\n{}", content);
    println!("\n");

    // Append more data using write_file_to_internal_flash
    println!("Appending more data to test_write.txt...");
    let append_data = b"\nAppended line 4.\nAppended line 5.";
    modem
        .write_file_to_internal_flash("test_write.txt", append_data)
        .unwrap();
    println!("Successfully appended {} bytes", append_data.len());

    // Read the file again to see the appended content
    println!("Reading test_write.txt again after append...");
    let mut read_buffer2 = vec![0u8; 512];
    let bytes_read2 = modem
        .read_file_from_internal_flash("test_write.txt", &mut read_buffer2)
        .unwrap();
    println!("Successfully read {} bytes", bytes_read2);
    let content2 = std::str::from_utf8(&read_buffer2[..bytes_read2]).unwrap();
    println!("Content after append:\n{}", content2);
    println!("\n");

    // Read cert.pem using read_file_from_internal_flash
    println!("Reading cert.pem using read_file_from_internal_flash...");
    let mut cert_buffer = vec![0u8; 128];
    let cert_bytes_read = modem
        .read_file_from_internal_flash("cert.pem", &mut cert_buffer)
        .unwrap();
    println!("Successfully read {} bytes from cert.pem", cert_bytes_read);
    let cert_content = std::str::from_utf8(&cert_buffer[..cert_bytes_read]).unwrap();
    println!("cert.pem content: {}", cert_content);
    println!("\n");

    // Clean up - delete test_write.txt
    println!("Cleaning up - deleting test_write.txt...");
    modem.delete_file_from_internal_flash("test_write.txt").unwrap();

    // Final file listing
    let files = modem.get_all_files_list_from_internal_flash().unwrap();
    println!("\nFinal files in internal flash:");
    for (filename, size) in &files {
        println!("File: {}, Size: {} bytes", filename, size);
    }
    println!("\n");

    modem.power_off().unwrap();

    modem_pwr_key.done();
}
