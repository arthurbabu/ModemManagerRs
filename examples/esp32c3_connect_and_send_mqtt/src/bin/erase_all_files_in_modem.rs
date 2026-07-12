// Erase all files in Quectel BG9X internal flash example for ESP32-C3
use std::{thread, time};

use esp_idf_hal::delay;
use esp_idf_hal::gpio;
use esp_idf_hal::prelude::*;
use esp_idf_hal::task::watchdog::TWDTDriver;
use esp_idf_hal::uart;
use esp_idf_svc::log::EspLogger;
use esp_idf_sys as _;

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
    esp_idf_sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();

    let twdt_config = esp_idf_hal::task::watchdog::TWDTConfig {
        duration: time::Duration::from_secs(20),
        panic_on_trigger: true,
        subscribed_idle_tasks: Default::default(),
    };
    let mut twdt_driver = TWDTDriver::new(peripherals.twdt, &twdt_config).unwrap();
    let mut twdt_subscription = twdt_driver.watch_current_task().unwrap();

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

    modem_pwr_en.set_low().unwrap();
    thread::sleep(time::Duration::from_millis(500));
    modem_pwr_en.set_high().unwrap();

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
                ingress.clear();
            }
        }
    });

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

    // List all files in internal flash
    let files = match modem.get_all_files_list_from_internal_flash() {
        Ok(files) => files,
        Err(e) => {
            error!("Error listing files: {:?}", e);
            modem.power_off().unwrap();
            modem_pwr_en.set_low().unwrap();
            return;
        }
    };

    if files.is_empty() {
        info!("No files found in modem internal flash.");
    } else {
        info!("Found {} files. Erasing...", files.len());
        for (filename, _size) in &files {
            info!("Deleting {}...", filename);
            match modem.delete_file_from_internal_flash(filename) {
                Ok(_) => info!("{} deleted successfully.", filename),
                Err(e) => warn!("Error deleting {}: {:?}", filename, e),
            }
            twdt_subscription.feed().unwrap();
        }
    }

    // Final file listing
    let files = modem.get_all_files_list_from_internal_flash().unwrap();
    info!("Final files in internal flash:");
    for (filename, size) in &files {
        info!("File: {}, Size: {} bytes", filename, size);
    }
    if files.is_empty() {
        info!("All files erased successfully.");
    }
    info!("");

    twdt_subscription.feed().unwrap();
    modem.power_off().unwrap();
    modem_pwr_en.set_low().unwrap();

    loop {
        thread::sleep(time::Duration::from_secs(1));
        twdt_subscription.feed().unwrap();
    }
}
