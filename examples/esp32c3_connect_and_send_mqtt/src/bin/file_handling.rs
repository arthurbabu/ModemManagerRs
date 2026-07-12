// File handling example for Quectel BG9X on ESP32-C3
use std::{thread, time};

use esp_idf_sys as _; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported

use esp_idf_hal::delay;
use esp_idf_hal::gpio;
use esp_idf_hal::prelude::*;
use esp_idf_hal::task::watchdog::TWDTDriver;
use esp_idf_hal::uart;
use esp_idf_svc::log::EspLogger;

use log::*;

use atat::blocking::Client;
use atat::AtatIngress;
use atat::DefaultDigester;
use atat::Ingress;
use atat::{Config as AtatConfig, ResponseSlot, UrcChannel};
use quectel_bg9x_eh_driver::cellular::{
    QuectelBG9X, INGRESS_BUF_SIZE, URC_CAPACITY, URC_SUBSCRIBERS,
};
use quectel_bg9x_eh_driver::quectel_atat::urc::Urc;
use static_cell::StaticCell;

extern crate alloc;
use alloc::vec;

#[toml_cfg::toml_config]
struct Config {
    #[default("")]
    modem_apn: &'static str,
    #[default("")]
    modem_user: &'static str,
    #[default("")]
    modem_pass: &'static str,
}

fn main() {
    // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // or else some patches to the runtime implemented by esp-idf-sys might not link properly.

    esp_idf_sys::link_patches();
    EspLogger::initialize_default();

    // File names
    const CERT_FILE: &str = "cert.pem";
    const TEST_FILE: &str = "test_write.txt";

    let peripherals = Peripherals::take().unwrap();

    // Configure and subscribe to the watchdog timer
    // Set to 20 seconds to accommodate file operations that may take up to 10 seconds
    let twdt_config = esp_idf_hal::task::watchdog::TWDTConfig {
        duration: time::Duration::from_secs(20),
        panic_on_trigger: true,
        subscribed_idle_tasks: Default::default(),
    };
    let mut twdt_driver = TWDTDriver::new(peripherals.twdt, &twdt_config).unwrap();

    // Subscribe current task to watchdog
    let mut twdt_subscription = twdt_driver.watch_current_task().unwrap();

    info!("Testing Quectel BG9X File Handling");
    let mut modem_pwr_en = gpio::PinDriver::output(peripherals.pins.gpio3).unwrap();
    let modem_pwr_key = gpio::PinDriver::output(peripherals.pins.gpio2).unwrap();
    let modem_tx = peripherals.pins.gpio0;
    let modem_rx = peripherals.pins.gpio1;
    let uart_conf = uart::UartConfig::new()
        .baudrate(115_200.Hz())
        .source_clock(uart::config::SourceClock::RTC);
    let uart = uart::UartDriver::new(
        peripherals.uart1,
        modem_tx,
        modem_rx,
        Option::<gpio::AnyIOPin>::None,
        Option::<gpio::AnyIOPin>::None,
        &uart_conf,
    )
    .unwrap();

    // Make sure modem starts disabled
    modem_pwr_en.set_low().unwrap();
    thread::sleep(time::Duration::from_millis(500));

    // Enable modem back
    modem_pwr_en.set_high().unwrap();

    // atat init
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

    let (tx, rx) = uart.into_split();
    let client = Client::new(tx, &RES_SLOT, buf, AtatConfig::default());

    let builder = thread::Builder::new().stack_size(8 * 1024);
    let _ = builder.spawn(move || loop {
        let buf = ingress.write_buf();
        match rx.read(
            buf,
            delay::TickType::from(time::Duration::from_millis(50)).0,
        ) {
            Ok(len) => {
                if len != 0 {
                    let s = std::str::from_utf8(&buf[..len]).unwrap();
                    log::info!("Read: ({}) {}", len, s);
                }
                match ingress.try_advance(len) {
                    Ok(_) => {}
                    Err(e) => {
                        log::info!("Error advancing ingress {:?}", e);
                        ingress.clear();
                    }
                }
            }
            Err(_e) => {
                // log::info!("Error reading from UART {:?}", e);
                ingress.clear();
            }
        }
    });
    // end of atat initialization

    twdt_subscription.feed().unwrap();

    let mut modem = match QuectelBG9X::new(modem_pwr_key, client, &URC_CHANNEL) {
        Ok(modem) => modem,
        Err(e) => {
            error!("Error initializing modem: {:?}", e);
            loop {
                thread::sleep(time::Duration::from_millis(1000));
            }
        }
    };

    twdt_subscription.feed().unwrap();

    if let Ok((filename, size)) = modem.get_file_meta_from_internal_flash(CERT_FILE) {
        info!("Found {} with size {} bytes", filename, size);
        let mut file_data = vec![0u8; size as usize];
        modem
            .read_file_from_internal_flash(CERT_FILE, &mut file_data)
            .unwrap();
        let content = core::str::from_utf8(&file_data).unwrap();
        info!("cert.pem content: {}", content);

        info!("Deleting {} from internal flash...", CERT_FILE);
        modem.delete_file_from_internal_flash(CERT_FILE).unwrap();
        twdt_subscription.feed().unwrap();
    } else {
        info!("{} not found in internal flash.", CERT_FILE);
    }

    // This makes the device panic because of stack overflow if the stack size is not increased
    // info!("Uploading cert.pem...");
    // let cert: &[u8] = "Adios, rios; adios, fontes;".as_bytes();
    // modem
    //     .upload_file_to_internal_flash("cert.pem", cert)
    //     .unwrap();

    // // Demonstrate write_file_to_internal_flash
    twdt_subscription.feed().unwrap();
    info!("Writing data to test_write.txt using write_file_to_internal_flash...");
    let write_data = b"Hello from write_file_to_internal_flash!\nThis is line 2.\nThis is line 3.";
    modem
        .write_file_to_internal_flash(TEST_FILE, write_data)
        .unwrap();
    twdt_subscription.feed().unwrap();
    info!(
        "Successfully wrote {} bytes to test_write.txt",
        write_data.len()
    );

    // This makes the device panic because of stack overflow if the stack size is not increased
    // List files again to confirm
    let files = modem.get_all_files_list_from_internal_flash().unwrap();
    info!("Files in internal flash after write:");
    for (filename, size) in &files {
        info!("File: {}, Size: {} bytes", filename, size);
    }
    info!("");

    match modem.get_file_meta_from_internal_flash(TEST_FILE) {
        Ok(r) => {
            info!("{} of size {} bytes found", TEST_FILE, r.1);
        }
        Err(_) => {
            warn!("{} not found in internal flash.", TEST_FILE);
        }
    }

    // Demonstrate read_file_from_internal_flash
    info!(
        "Reading {} using read_file_from_internal_flash...",
        TEST_FILE
    );
    let mut read_buffer = vec![0u8; 256]; // Allocate buffer for reading
    let bytes_read = modem
        .read_file_from_internal_flash(TEST_FILE, &mut read_buffer)
        .unwrap();
    info!("Successfully read {} bytes from {}", bytes_read, TEST_FILE);
    let content = core::str::from_utf8(&read_buffer[..bytes_read]).unwrap();
    info!("Content:\n{}", content);
    info!("");

    // Append more data using write_file_to_internal_flash
    info!("Appending more data to {}...", TEST_FILE);
    let append_data = b"\nAppended line 4.\nAppended line 5.";
    modem
        .write_file_to_internal_flash(TEST_FILE, append_data)
        .unwrap();
    info!("Successfully appended {} bytes", append_data.len());

    // Read the file again to see the appended content
    info!("Reading test_write.txt again after append...");
    let mut read_buffer2 = vec![0u8; 512];
    let bytes_read2 = modem
        .read_file_from_internal_flash(TEST_FILE, &mut read_buffer2)
        .unwrap();
    info!("Successfully read {} bytes", bytes_read2);
    let content2 = core::str::from_utf8(&read_buffer2[..bytes_read2]).unwrap();
    info!("Content after append:\n{}", content2);
    info!("");

    // This part is commented out because it causes stack overflow panics
    // // Read cert.pem using read_file_from_internal_flash
    // info!("Reading cert.pem using read_file_from_internal_flash...");
    // // First get the file size
    // let (_, cert_size) = modem.get_file_meta_from_internal_flash(CERT_FILE).unwrap();
    // let mut cert_buffer = vec![0u8; cert_size as usize];
    // let cert_bytes_read = modem
    //     .read_file_from_internal_flash(CERT_FILE, &mut cert_buffer)
    //     .unwrap();
    // info!("Successfully read {} bytes from cert.pem", cert_bytes_read);
    // let cert_content = core::str::from_utf8(&cert_buffer[..cert_bytes_read]).unwrap();
    // info!("cert.pem content: {}", cert_content);
    // info!("");

    // // Clean up - delete files
    info!("Cleaning up - deleting {}...", TEST_FILE);
    modem.delete_file_from_internal_flash(TEST_FILE).unwrap();
    twdt_subscription.feed().unwrap();

    info!("deleting {}...", CERT_FILE);
    match modem.delete_file_from_internal_flash(CERT_FILE) {
        Ok(_) => {
            info!("{} deleted successfully (it existed).", CERT_FILE);
        }
        Err(e) => {
            warn!("Error deleting {}: {:?}", CERT_FILE, e);
        }
    }
    twdt_subscription.feed().unwrap();

    // Final file listing
    let files = modem.get_all_files_list_from_internal_flash().unwrap();
    info!("Final files in internal flash:");
    for (filename, size) in &files {
        info!("File: {}, Size: {} bytes", filename, size);
    }
    info!("");

    twdt_subscription.feed().unwrap();
    modem.power_off().unwrap();

    modem_pwr_en.set_low().unwrap();

    loop {
        thread::sleep(time::Duration::from_secs(1)); // Sleep forever
        twdt_subscription.feed().unwrap(); // Keep feeding the watchdog
    }
}
