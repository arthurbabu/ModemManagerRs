// Test Quectel On
use std::{thread, time};

use esp_idf_sys as _; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported

use esp_idf_hal::delay;
use esp_idf_hal::gpio;
use esp_idf_hal::prelude::*;
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
use modem_manager_rs::quectel_atat::types::{
    EmtcBands, GnssOperatingMode, GsmBands, ModemConfiguration, NbIotBands, RadioAccessTechnology,
    SslAuthenticationMode, SslCipherSuiteEnum, SslConfiguration, SslVersion, AuthenticationMethod,
};
use modem_manager_rs::quectel_atat::urc::Urc;
use modem_manager_rs::ModemError;
use static_cell::StaticCell;

#[toml_cfg::toml_config]
struct Config {
    #[default("")]
    modem_apn: &'static str,
    #[default("")]
    modem_user: &'static str,
    #[default("")]
    modem_pass: &'static str,
    #[default(0)]
    modem_auth_method: u8,

    #[default("test.mosquitto.org")]
    mqtt_server: &'static str,
    #[default(1883)]
    mqtt_port: u16,
    #[default("")]
    mqtt_user: &'static str,
    #[default("")]
    mqtt_pass: &'static str,

    #[default(false)]
    mqtt_use_ssl: bool,
    #[default(2)]
    mqtt_ssl_context_id: u8,
    #[default("")]
    mqtt_ca_cert_filename: &'static str,
    #[default("tls12")]
    mqtt_ssl_version: &'static str,
    #[default("all")]
    mqtt_ssl_cipher_suite: &'static str,
    #[default("server")]
    mqtt_ssl_auth_mode: &'static str,
    #[default(true)]
    mqtt_ssl_sni_enable: bool,
    #[default(false)]
    mqtt_ssl_checkhost_enable: bool,
    #[default(true)]
    mqtt_ssl_ignore_localtime: bool,
}

fn main() {
    // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // or else some patches to the runtime implemented by esp-idf-sys might not link properly.

    esp_idf_sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();

    info!("Testing Quectel BG9X");
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

    let mut mm = match QuectelBG9X::new(modem_pwr_key, client, &URC_CHANNEL) {
        Ok(mm) => mm,
        Err(e) => {
            error!("Error initializing modem: {:?}", e);
            loop {
                thread::sleep(time::Duration::from_millis(1000));
            }
        }
    };

    let mut mm_config = ModemConfiguration::new();
    // Configuration for europe frequency bands and LTE-M -> 2G
    mm_config
        .set_bands(
            RadioAccessTechnology::GSM,
            &[GsmBands::Gsm900MHz, GsmBands::Gsm1800MHz],
        )
        .unwrap();
    mm_config
        .set_bands(
            RadioAccessTechnology::EMTC,
            &[EmtcBands::Band3, EmtcBands::Band8, EmtcBands::Band20],
        )
        .unwrap();
    mm_config
        .set_bands(
            RadioAccessTechnology::NbIoT,
            &[NbIotBands::Band3, NbIotBands::Band8, NbIotBands::Band20],
        )
        .unwrap();
    mm_config
        .set_rat_order(&[RadioAccessTechnology::EMTC, RadioAccessTechnology::GSM])
        .unwrap();
    mm.is_alive().unwrap();

    // Small delay to ensure modem is ready (+CME ERROR: 14 sometimes occurs otherwise)
    std::thread::sleep(std::time::Duration::from_millis(200));
    mm.test_sim().unwrap_or_else(|_| {
        println!("SIM test failed, stopping execution.");
        mm.context_deactivate().unwrap();
        mm.power_off().unwrap();
        modem_pwr_en.set_low().unwrap();
        loop {
            thread::sleep(time::Duration::from_secs(1));
        }
    });

    mm.set_modem_funcionality(false).unwrap();
    // mm.factory_reset().unwrap();
    mm.set_modem_configuration(mm_config).unwrap();

    mm.configure_gnss_priority().unwrap_or_else(|e| {
        warn!("Failed to configure GNSS priority: {:?}", e);
    });
    mm.set_modem_funcionality(true).unwrap();

    mm.network_attach().unwrap();
    let (_, signalq) = mm.get_signal_strength().unwrap();
    info!("Signal quality: {}%", signalq);
    
    // According to Quectel, context configuration may be set before registering to the network
    // in case of registration failure
    mm.set_context_configuration(
        CONFIG.modem_apn,
        CONFIG.modem_user,
        CONFIG.modem_pass,
        AuthenticationMethod::try_from(CONFIG.modem_auth_method)
            .unwrap_or(AuthenticationMethod::None),
    )
    .unwrap();


    let ts_nitz = mm.get_nitz_time();
    mm.context_activate().unwrap();
    let ts_ntp = mm.get_ntp_time("0.es.pool.ntp.org");

    info!("NITZ: {:?}", ts_nitz);
    info!("NTP: {:?}", ts_ntp);

    // Prepare SSL configuration if enabled
    let ssl_config = if CONFIG.mqtt_use_ssl {
        info!("Configuring SSL/TLS for MQTT...");

        let mut config = SslConfiguration::new();

        // Set context ID
        config.set_context_id(CONFIG.mqtt_ssl_context_id).unwrap();

        // Set CA certificate
        if !CONFIG.mqtt_ca_cert_filename.is_empty() {
            config.set_ca_cert(CONFIG.mqtt_ca_cert_filename).unwrap();
        }

        // Parse and set SSL version
        let ssl_version = match CONFIG.mqtt_ssl_version {
            "tls10" => SslVersion::Tls1_0,
            "tls11" => SslVersion::Tls1_1,
            "tls12" => SslVersion::Tls1_2,
            "all" => SslVersion::All,
            _ => SslVersion::Tls1_2,
        };
        config.set_ssl_version(ssl_version);

        // Parse and set cipher suite
        match CONFIG.mqtt_ssl_cipher_suite {
            "all" => config.set_cipher_suite_all(),
            "0x0035" => config.set_cipher_suite(SslCipherSuiteEnum::TlsRsaWithAes256CbcSha),
            "0x002F" => config.set_cipher_suite(SslCipherSuiteEnum::TlsRsaWithAes128CbcSha),
            "0x0005" => config.set_cipher_suite(SslCipherSuiteEnum::TlsRsaWithRc4_128Sha),
            "0x0004" => config.set_cipher_suite(SslCipherSuiteEnum::TlsRsaWithRc4_128Md5),
            "0x000A" => config.set_cipher_suite(SslCipherSuiteEnum::TlsRsaWith3desEdeCbcSha),
            "0x003D" => config.set_cipher_suite(SslCipherSuiteEnum::TlsRsaWithAes256CbcSha256),
            "0xC027" => config.set_cipher_suite(SslCipherSuiteEnum::TlsEcdheRsaWithAes128CbcSha256),
            "0xC028" => config.set_cipher_suite(SslCipherSuiteEnum::TlsEcdheRsaWithAes256CbcSha384),
            "0xC013" => config.set_cipher_suite(SslCipherSuiteEnum::TlsEcdheRsaWithAes128CbcSha),
            "0xC014" => config.set_cipher_suite(SslCipherSuiteEnum::TlsEcdheRsaWithAes256CbcSha),
            _ => config.set_cipher_suite_all(),
        };

        // Parse and set authentication mode
        let auth_mode = match CONFIG.mqtt_ssl_auth_mode {
            "none" => SslAuthenticationMode::None,
            "server" => SslAuthenticationMode::ServerOnly,
            "mutual" => SslAuthenticationMode::Mutual,
            _ => SslAuthenticationMode::ServerOnly,
        };
        config.set_auth_mode(auth_mode);

        // Set SNI, checkhost, and ignore_localtime
        config.set_sni(CONFIG.mqtt_ssl_sni_enable);
        config.set_check_host(CONFIG.mqtt_ssl_checkhost_enable);
        config.set_ignore_localtime(CONFIG.mqtt_ssl_ignore_localtime);

        Some(config)
    } else {
        None
    };

    // Connect to MQTT broker (SSL configuration is passed and applied if needed)
    if mm
        .mqtt_connect(
            CONFIG.mqtt_server,
            CONFIG.mqtt_port,
            "mqtt_id_12345",
            CONFIG.mqtt_user,
            CONFIG.mqtt_pass,
            ssl_config,
        )
        .is_ok()
    {
        info!("Connected to MQTT broker");
        mm.mqtt_publish("some/test", "some-payload", 0)
            .unwrap_or_else(|e| {
                error!("Failed to publish MQTT message: {:?}", e);
            });
        mm.mqtt_disconnect().unwrap_or_else(|e| {
            error!("Failed to disconnect MQTT: {:?}", e);
        });
    } else {
        error!("Failed to connect to MQTT broker");
    }

    mm.turn_on_gnss(GnssOperatingMode::StandAlone).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2000));

    for _ in 0..3 {
        match mm.get_gnss_location() {
            Ok(_) => {
                info!("Data retrieved");
                break;
            }
            Err(ModemError::GnssNotFixed) => {
                info!("GNSS not fixed yet, retrying...");
                std::thread::sleep(std::time::Duration::from_millis(1000));
            }
            Err(e) => {
                mm.power_off().unwrap();
                panic!("An error occured while getting GPS position ({:?})", e);
            }
        }
    }

    mm.turn_off_gnss().unwrap();
    mm.context_deactivate().unwrap();
    mm.power_off().unwrap();

    modem_pwr_en.set_low().unwrap();

    loop {
        thread::sleep(time::Duration::from_secs(1));
    }
}
