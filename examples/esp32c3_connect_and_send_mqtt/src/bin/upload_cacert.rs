// Upload CA certificate to Quectel BG9X on ESP32-C3
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
use modem_manager_rs::cellular::{
    QuectelBG9X, INGRESS_BUF_SIZE, URC_CAPACITY, URC_SUBSCRIBERS,
};
use modem_manager_rs::quectel_atat::urc::Urc;
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

// CA Certificate (hivemq) content as a constant
// Replace this with your actual CA certificate
const CACERT_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIFazCCA1OgAwIBAgIRAIIQz7DSQONZRGPgu2OCiwAwDQYJKoZIhvcNAQELBQAw
TzELMAkGA1UEBhMCVVMxKTAnBgNVBAoTIEludGVybmV0IFNlY3VyaXR5IFJlc2Vh
cmNoIEdyb3VwMRUwEwYDVQQDEwxJU1JHIFJvb3QgWDEwHhcNMTUwNjA0MTEwNDM4
WhcNMzUwNjA0MTEwNDM4WjBPMQswCQYDVQQGEwJVUzEpMCcGA1UEChMgSW50ZXJu
ZXQgU2VjdXJpdHkgUmVzZWFyY2ggR3JvdXAxFTATBgNVBAMTDElTUkcgUm9vdCBY
MTCCAiIwDQYJKoZIhvcNAQEBBQADggIPADCCAgoCggIBAK3oJHP0FDfzm54rVygc
h77ct984kIxuPOZXoHj3dcKi/vVqbvYATyjb3miGbESTtrFj/RQSa78f0uoxmyF+
0TM8ukj13Xnfs7j/EvEhmkvBioZxaUpmZmyPfjxwv60pIgbz5MDmgK7iS4+3mX6U
A5/TR5d8mUgjU+g4rk8Kb4Mu0UlXjIB0ttov0DiNewNwIRt18jA8+o+u3dpjq+sW
T8KOEUt+zwvo/7V3LvSye0rgTBIlDHCNAymg4VMk7BPZ7hm/ELNKjD+Jo2FR3qyH
B5T0Y3HsLuJvW5iB4YlcNHlsdu87kGJ55tukmi8mxdAQ4Q7e2RCOFvu396j3x+UC
B5iPNgiV5+I3lg02dZ77DnKxHZu8A/lJBdiB3QW0KtZB6awBdpUKD9jf1b0SHzUv
KBds0pjBqAlkd25HN7rOrFleaJ1/ctaJxQZBKT5ZPt0m9STJEadao0xAH0ahmbWn
OlFuhjuefXKnEgV4We0+UXgVCwOPjdAvBbI+e0ocS3MFEvzG6uBQE3xDk3SzynTn
jh8BCNAw1FtxNrQHusEwMFxIt4I7mKZ9YIqioymCzLq9gwQbooMDQaHWBfEbwrbw
qHyGO0aoSCqI3Haadr8faqU9GY/rOPNk3sgrDQoo//fb4hVC1CLQJ13hef4Y53CI
rU7m2Ys6xt0nUW7/vGT1M0NPAgMBAAGjQjBAMA4GA1UdDwEB/wQEAwIBBjAPBgNV
HRMBAf8EBTADAQH/MB0GA1UdDgQWBBR5tFnme7bl5AFzgAiIyBpY9umbbjANBgkq
hkiG9w0BAQsFAAOCAgEAVR9YqbyyqFDQDLHYGmkgJykIrGF1XIpu+ILlaS/V9lZL
ubhzEFnTIZd+50xx+7LSYK05qAvqFyFWhfFQDlnrzuBZ6brJFe+GnY+EgPbk6ZGQ
3BebYhtF8GaV0nxvwuo77x/Py9auJ/GpsMiu/X1+mvoiBOv/2X/qkSsisRcOj/KK
NFtY2PwByVS5uCbMiogziUwthDyC3+6WVwW6LLv3xLfHTjuCvjHIInNzktHCgKQ5
ORAzI4JMPJ+GslWYHb4phowim57iaztXOoJwTdwJx4nLCgdNbOhdjsnvzqvHu7Ur
TkXWStAmzOVyyghqpZXjFaH3pO3JLF+l+/+sKAIuvtd7u+Nxe5AW0wdeRlN8NwdC
jNPElpzVmbUq4JUagEiuTDkHzsxHpFKVK7q4+63SM1N95R1NbdWhscdCb+ZAJzVc
oyi3B43njTOQ5yOf+1CceWxG1bQVs5ZufpsMljq4Ui0/1lvh+wjChP4kqKOJ2qxq
4RgqsahDYVvTH9w7jXbyLeiNdd8XM2w9U/t7y0Ff/9yi0GE44Za4rF2LN9d11TPA
mRGunUHBcnWEvgJBQl9nJEiU0Zsnvgc/ubhPgXRR4Xq37Z0j4r7g1SgEEzwxA57d
emyPxgcYxn/eR44/KJ4EBs+lVDR3veyJm+kXQ99b21/+jh5Xos1AnX5iItreGCc=
-----END CERTIFICATE-----
"#;

fn main() {
    esp_idf_sys::link_patches();
    EspLogger::initialize_default();

    const CACERT_FILE: &str = "cacert.pem";

    let peripherals = Peripherals::take().unwrap();

    // Configure watchdog timer - 20 seconds to accommodate file operations
    let twdt_config = esp_idf_hal::task::watchdog::TWDTConfig {
        duration: time::Duration::from_secs(20),
        panic_on_trigger: true,
        subscribed_idle_tasks: Default::default(),
    };
    let mut twdt_driver = TWDTDriver::new(peripherals.twdt, &twdt_config).unwrap();
    let mut twdt_subscription = twdt_driver.watch_current_task().unwrap();

    // Setup GPIO and UART for modem
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

    // Power cycle modem
    modem_pwr_en.set_low().unwrap();
    thread::sleep(time::Duration::from_millis(500));
    modem_pwr_en.set_high().unwrap();

    // Initialize ATAT
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

    // Spawn UART reader thread
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

    // Initialize modem
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

    // Check if cacert.pem already exists
    if let Ok((filename, size)) = modem.get_file_meta_from_internal_flash(CACERT_FILE) {
        info!("Found existing {} with size {} bytes", filename, size);
        info!("Deleting old certificate...");
        modem.delete_file_from_internal_flash(CACERT_FILE).unwrap();
        twdt_subscription.feed().unwrap();
        info!("Old certificate deleted successfully");
    } else {
        info!("No existing {} found", CACERT_FILE);
    }

    // Upload the CA certificate
    info!("Uploading {} ({} bytes)...", CACERT_FILE, CACERT_PEM.len());
    twdt_subscription.feed().unwrap();

    // You can use upload_file_to_internal_flash or write_file_to_internal_flash here
    match modem.upload_file_to_internal_flash(CACERT_FILE, CACERT_PEM.as_bytes()) {
        Ok(_) => {
            info!("Successfully uploaded {}", CACERT_FILE);
        }
        Err(e) => {
            error!("✗ Failed to upload {}: {:?}", CACERT_FILE, e);
            loop {
                thread::sleep(time::Duration::from_secs(1));
                twdt_subscription.feed().unwrap();
            }
        }
    }

    twdt_subscription.feed().unwrap();

    // Verify the upload by checking file metadata
    match modem.get_file_meta_from_internal_flash(CACERT_FILE) {
        Ok((filename, size)) => {
            info!("Verification: {} exists with {} bytes", filename, size);

            // Optionally read back and display the content
            info!("Reading back certificate content...");
            let mut cert_buffer = vec![0u8; size as usize];
            match modem.read_file_from_internal_flash(CACERT_FILE, &mut cert_buffer) {
                Ok(bytes_read) => {
                    info!("Read {} bytes", bytes_read);
                    let content = core::str::from_utf8(&cert_buffer[..bytes_read]).unwrap();
                    info!("Certificate content:\n{}", content);
                }
                Err(e) => {
                    error!("✗ Failed to read back certificate: {:?}", e);
                }
            }
        }
        Err(e) => {
            error!("✗ Verification failed: {:?}", e);
        }
    }

    twdt_subscription.feed().unwrap();

    info!("Certificate upload process complete!");
    modem.power_off().unwrap();
    modem_pwr_en.set_low().unwrap();

    loop {
        thread::sleep(time::Duration::from_secs(1));
        twdt_subscription.feed().unwrap();
    }
}
