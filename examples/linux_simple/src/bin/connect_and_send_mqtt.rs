
use std::{env, thread, time};

use modem_manager_rs::cellular::{
    QuectelBG9X, INGRESS_BUF_SIZE, URC_CAPACITY, URC_SUBSCRIBERS,
};
use modem_manager_rs::quectel_atat::types::{
    EmtcBands, GsmBands, ModemConfiguration, NbIotBands, RadioAccessTechnology,
    SslAuthenticationMode, SslCipherSuiteEnum, SslConfiguration, SslVersion, AuthenticationMethod,
};
use modem_manager_rs::quectel_atat::urc::Urc;

use atat::blocking::Client;
use atat::AtatIngress;
use atat::DefaultDigester;
use atat::Ingress;
use atat::{Config as AtatConfig, ResponseSlot, UrcChannel};
use static_cell::StaticCell;
use embedded_io::Read;
use embedded_hal_mock::eh1::digital::{
    Mock as PinMock,
    State as PinState,
    Transaction as PinTransaction,
};

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
    let modem_pwr_key = PinMock::new(&expectations);

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
                log::info!("Error reading from UART: {:?}", e);
                ingress.clear();
            }
        }
    });
    // end of atat initialization

    log::info!("Starting ATAT client...");
    let mut mm = match QuectelBG9X::new(modem_pwr_key.clone(), client, &URC_CHANNEL) {
        Ok(mm) => mm,
        Err(e) => {
            log::error!("Error initializing modem: {:?}", e);
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
        // modem_pwr_key.set_low().unwrap();
        loop {
            thread::sleep(time::Duration::from_secs(1));
        }
    });

    mm.set_modem_funcionality(false).unwrap();
    // mm.factory_reset().unwrap();
    mm.set_modem_configuration(mm_config).unwrap();

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

    mm.set_modem_funcionality(true).unwrap();

    mm.network_attach().unwrap();
    let (_, signalq) = mm.get_signal_strength().unwrap();
    log::info!("Signal quality: {}%", signalq);

    let ts_nitz = mm.get_nitz_time();
    mm.context_activate().unwrap();
    let ts_ntp = mm.get_ntp_time("0.es.pool.ntp.org");

    log::info!("NITZ: {:?}", ts_nitz);
    log::info!("NTP: {:?}", ts_ntp);

    // Prepare SSL configuration if enabled
    let ssl_config = if CONFIG.mqtt_use_ssl {
        log::info!("Configuring SSL/TLS for MQTT...");

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
        log::info!("Connected to MQTT broker");
        mm.mqtt_publish("some/test", "some-payload", 0)
            .unwrap_or_else(|e| {
                log::error!("Failed to publish MQTT message: {:?}", e);
            });
        mm.mqtt_disconnect().unwrap_or_else(|e| {
            log::error!("Failed to disconnect MQTT: {:?}", e);
        });
    } else {
        log::error!("Failed to connect to MQTT broker");
    }

    mm.context_deactivate().unwrap();
    mm.power_off().unwrap();

    // modem_pwr_key.set_low().unwrap();

    loop {
        thread::sleep(time::Duration::from_secs(1));
    }
}
