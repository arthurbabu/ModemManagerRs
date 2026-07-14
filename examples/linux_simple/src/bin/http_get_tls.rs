//! HTTPS GET over a modem-terminated TLS socket, with **server authentication**.
//!
//! This talks to an HTTPS server (default `www.google.com:443`) using the
//! modem's own TCP+TLS engine via the async driver methods `ssl_socket_open` /
//! `ssl_socket_send` / `ssl_socket_recv` / `ssl_socket_close`. The modem
//! verifies the server certificate against a CA certificate you upload to its
//! flash (set `ca_cert_path` in `cfg.toml`).
//!
//! Usage:
//! ```text
//! cd examples/linux_simple
//! cp cfg.toml.example cfg.toml   # set ca_cert_path (and APN if needed)
//! cargo run --bin http_get_tls /dev/ttyUSB4
//! ```
//!
//! If `ca_cert_path` is left empty the example falls back to *no* server
//! authentication (encrypted but unverified) and logs a warning — handy for a
//! quick smoke test, but it is not the "server authentication" path.

use std::env;
use std::time::Duration;

use modem_manager_rs::cellular::{
    ssl_recv_digest_hook, QuectelBG9X, INGRESS_BUF_SIZE, URC_CAPACITY, URC_SUBSCRIBERS,
};
use modem_manager_rs::quectel_atat::types::{
    AuthenticationMethod, ModemConfiguration, SslAuthenticationMode, SslConfiguration, SslVersion,
};
use modem_manager_rs::quectel_atat::urc::Urc;

use atat::asynch::Client;
use atat::AtatIngress;
use atat::DefaultDigester;
use atat::Ingress;
use atat::{Config as AtatConfig, ResponseSlot, UrcChannel};

use embedded_hal_mock::eh1::digital::{
    Mock as PinMock, State as PinState, Transaction as PinTransaction,
};
use embedded_io_adapters::tokio_1::FromTokio;
use static_cell::StaticCell;
use tokio_serial::SerialPortBuilderExt;

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

    // HTTPS target.
    #[default("www.google.com")]
    http_host: &'static str,
    #[default(443)]
    http_port: u16,
    #[default("/")]
    http_path: &'static str,

    // TLS / server-authentication settings.
    #[default(2)]
    ssl_context_id: u8,
    /// Path to a local CA certificate (PEM) that signs the server's chain.
    /// Uploaded to the modem as `cacert.pem`. Empty => no server verification.
    #[default("")]
    ca_cert_path: &'static str,
    /// Verify that the certificate hostname matches `http_host`.
    #[default(true)]
    ssl_checkhost: bool,
    /// Ignore certificate validity dates (useful when the modem clock is unset).
    #[default(true)]
    ssl_ignore_localtime: bool,

    #[default("")]
    serial_port: &'static str,
}

#[tokio::main]
async fn main() {
    let serial_port = env::args()
        .nth(1)
        .expect("Usage: cargo run --bin http_get_tls <device>");
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    // The PWR_KEY GPIO is faked; the module is assumed to be already powered.
    let expectations = [
        PinTransaction::set(PinState::High),
        PinTransaction::set(PinState::Low),
    ];
    let modem_pwr_key = PinMock::new(&expectations);

    // Open the serial port asynchronously and split it into tx/rx halves.
    let serial = tokio_serial::new(serial_port, 115_200)
        .open_native_async()
        .expect("Could not open serial port");
    let (serial_rx, serial_tx) = tokio::io::split(serial);
    let serial_tx = FromTokio::new(serial_tx);
    let mut serial_rx = FromTokio::new(serial_rx);

    static INGRESS_BUF: StaticCell<[u8; INGRESS_BUF_SIZE]> = StaticCell::new();
    static RES_SLOT: ResponseSlot<INGRESS_BUF_SIZE> = ResponseSlot::new();
    static URC_CHANNEL: UrcChannel<Urc, URC_CAPACITY, URC_SUBSCRIBERS> = UrcChannel::new();
    // Use a custom-success digester hook so the binary `+QSSLRECV` data frame
    // is framed by its length prefix instead of atat's line/prompt heuristics,
    // which otherwise mis-parse binary payloads containing `>` / `\r\nOK\r\n`.
    let digester = DefaultDigester::<Urc>::default().with_custom_success(ssl_recv_digest_hook);
    let mut ingress = Ingress::new(
        digester,
        INGRESS_BUF.init([0; INGRESS_BUF_SIZE]),
        &RES_SLOT,
        &URC_CHANNEL,
    );

    static BUF: StaticCell<[u8; 1024]> = StaticCell::new();
    let buf = BUF.init([0; 1024]);

    let client = Client::new(serial_tx, &RES_SLOT, buf, AtatConfig::default());

    log::info!("Starting ATAT ingress task...");
    tokio::spawn(async move {
        ingress.read_from(&mut serial_rx).await;
    });
    // end of atat initialization

    log::info!("Starting ATAT client...");
    let mut mm = match QuectelBG9X::new(modem_pwr_key.clone(), client, &URC_CHANNEL).await {
        Ok(mm) => mm,
        Err(e) => {
            log::error!("Error initializing modem: {:?}", e);
            loop {
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }
        }
    };

    // The EG916U (Cat 1bis) uses automatic band / RAT selection:
    // `set_modem_configuration` ignores the per-RAT band masks on this chip, so
    // a default configuration is sufficient. (For BG95/BG96 you would narrow the
    // GSM/eMTC/NB-IoT bands and RAT search order here.)
    let mm_config = ModemConfiguration::new();

    mm.is_alive().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    mm.set_modem_funcionality(true).await.unwrap();
    if mm.test_sim().await.is_err() {
        log::error!("SIM test failed, stopping execution.");
        let _ = mm.power_off().await;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    mm.set_modem_configuration(mm_config).await.unwrap();
    mm.set_context_configuration(
        CONFIG.modem_apn,
        CONFIG.modem_user,
        CONFIG.modem_pass,
        AuthenticationMethod::try_from(CONFIG.modem_auth_method)
            .unwrap_or(AuthenticationMethod::None),
    )
    .await
    .unwrap();
    // mm.set_modem_funcionality(true).await.unwrap();

    mm.network_attach().await.unwrap();
    let (_, signalq) = mm.get_signal_strength().await.unwrap();
    log::info!("Signal quality: {}%", signalq);

    mm.context_activate().await.unwrap();

    // Sync the clock so the modem can check the server certificate validity dates.
    match mm.get_ntp_time("0.pool.ntp.org").await {
        Ok(ts) => log::info!("NTP time: {:?}", ts),
        Err(e) => log::warn!("NTP sync failed ({:?}); relying on ignore_localtime", e),
    }

    // ---- TLS server-authentication configuration ----------------------------
    let mut ssl = SslConfiguration::new();
    ssl.set_context_id(CONFIG.ssl_context_id).unwrap();
    ssl.set_ssl_version(SslVersion::Tls1_2);
    ssl.set_cipher_suite_all();
    ssl.set_sni(true); // required for SNI-based virtual hosts like Google
    ssl.set_check_host(CONFIG.ssl_checkhost);
    ssl.set_ignore_localtime(CONFIG.ssl_ignore_localtime);

    if CONFIG.ca_cert_path.is_empty() {
        log::warn!(
            "ca_cert_path is empty: connecting WITHOUT server authentication \
             (encrypted but unverified). Set ca_cert_path in cfg.toml for real \
             server auth."
        );
        ssl.set_auth_mode(SslAuthenticationMode::None);
    } else {
        // Upload the CA certificate to the modem flash, then reference it.
        let ca_pem = std::fs::read(CONFIG.ca_cert_path)
            .unwrap_or_else(|e| panic!("Could not read CA cert {}: {e}", CONFIG.ca_cert_path));
        log::info!(
            "Uploading CA certificate ({} bytes) as cacert.pem...",
            ca_pem.len()
        );
        // Overwrite any stale copy first (ignore "not found").
        let _ = mm.delete_file_from_internal_flash("cacert.pem").await;
        mm.upload_file_to_internal_flash("cacert.pem", &ca_pem)
            .await
            .expect("Failed to upload CA certificate");

        ssl.set_ca_cert("cacert.pem").unwrap();
        ssl.set_auth_mode(SslAuthenticationMode::ServerOnly);
    }

    mm.configure_ssl_context(ssl)
        .await
        .expect("Failed to configure SSL context");

    // ---- Open the socket, send the request, read the response ----------------
    const SOCKET_ID: u8 = 0;
    log::info!(
        "Connecting to https://{}:{}",
        CONFIG.http_host,
        CONFIG.http_port
    );
    mm.ssl_socket_open(
        SOCKET_ID,
        CONFIG.ssl_context_id,
        CONFIG.http_host,
        CONFIG.http_port,
    )
    .await
    .expect("Failed to open TLS socket");

    // Minimal HTTP/1.1 request. `Connection: close` makes the server close the
    // socket when the response is complete.
    let mut request = String::new();
    request.push_str("GET ");
    request.push_str(CONFIG.http_path);
    request.push_str(" HTTP/1.1\r\nHost: ");
    request.push_str(CONFIG.http_host);
    request.push_str("\r\nUser-Agent: modem_manager_rs\r\nAccept: */*\r\nConnection: close\r\n\r\n");

    log::info!("Sending HTTP request:\n{}", request);
    mm.ssl_socket_send(SOCKET_ID, request.as_bytes())
        .await
        .expect("Failed to send HTTP request");

    // Drain the response. `ssl_socket_recv` returns 0 when nothing is buffered
    // yet, so poll with a small back-off until the server has finished (a burst
    // of consecutive empty reads after we've seen data) or we hit a deadline.
    log::info!("Reading response...");
    let mut buf = [0u8; 512];
    let mut total = 0usize;
    let mut idle_reads = 0u32;
    let start = tokio::time::Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        match mm.ssl_socket_recv(SOCKET_ID, &mut buf).await {
            Ok(0) => {
                // Once we've received something, ~2s of silence means we're done.
                if total > 0 {
                    idle_reads += 1;
                    if idle_reads > 20 {
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Ok(n) => {
                idle_reads = 0;
                total += n;
                print!("{}", String::from_utf8_lossy(&buf[..n]));
            }
            Err(e) => {
                log::error!("Receive error: {:?}", e);
                break;
            }
        }
    }
    println!();
    log::info!("Received {} bytes total", total);

    mm.ssl_socket_close(SOCKET_ID).await.unwrap_or_else(|e| {
        log::error!("Failed to close socket: {:?}", e);
    });
    mm.context_deactivate().await.unwrap();
    mm.power_off().await.unwrap();

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
