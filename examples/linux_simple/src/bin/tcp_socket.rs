//! Wrap the modem as an **async TCP socket** and do an HTTP GET through it.
//!
//! This demonstrates [`modem_manager_rs::tcp`], the shared async socket
//! wrapper (used from tokio here, the same module the `embassy` runtime uses).
//! The modem is shared behind an `embassy_sync::Mutex` and handed to a
//! [`QuectelTcpClient`]; each `connect` returns a [`ModemSocket`] that
//! implements [`embedded_io_async::Read`]/[`Write`] — so the HTTP
//! request/response below is written with `.await` calls analogous to
//! `tokio::net::TcpStream`.
//!
//! Pick the transport per connection with
//! [`Transport`](modem_manager_rs::Transport): `Tcp` (plain) or
//! `Tls { ssl_ctx_id }` (modem-terminated TLS).
//!
//! Usage:
//! ```text
//! cd examples/linux_simple
//! cargo run --bin tcp_socket /dev/ttyUSB4
//! ```
//! By default it does a plain-TCP GET to `example.com:80`. Set `tcp_host` /
//! `tcp_port` / `tcp_path` in `cfg.toml` to target something else.

use std::env;
use std::time::Duration;

use modem_manager_rs::cellular::{
    socket_recv_digest_hook, QuectelBG9X, INGRESS_BUF_SIZE, URC_CAPACITY, URC_SUBSCRIBERS,
};
use modem_manager_rs::quectel_atat::types::{AuthenticationMethod, ModemConfiguration};
use modem_manager_rs::quectel_atat::urc::Urc;
use modem_manager_rs::tcp::QuectelTcpClient;

use atat::asynch::Client;
use atat::AtatIngress;
use atat::DefaultDigester;
use atat::Ingress;
use atat::{Config as AtatConfig, ResponseSlot, UrcChannel};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_hal_mock::eh1::digital::{
    Mock as PinMock, State as PinState, Transaction as PinTransaction,
};
use embedded_io_adapters::tokio_1::FromTokio;
use embedded_io_async::{Read, Write};
use embedded_nal_async::TcpConnect;
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

    // Plain-TCP HTTP target.
    #[default("example.com")]
    tcp_host: &'static str,
    #[default(80)]
    tcp_port: u16,
    #[default("/")]
    tcp_path: &'static str,
}

#[tokio::main]
async fn main() {
    let serial_port = env::args()
        .nth(1)
        .expect("Usage: cargo run --bin tcp_socket <device>");
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
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
    // `socket_recv_digest_hook` frames BOTH the `+QIRD` (plain TCP) and
    // `+QSSLRECV` (TLS) binary read responses by their length prefix, which the
    // default line/prompt-based digester cannot do reliably for binary payloads.
    let digester = DefaultDigester::<Urc>::default().with_custom_success(socket_recv_digest_hook);
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

    // Bring the modem online (EG916U uses automatic band/RAT selection).
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
    mm.network_attach().await.unwrap();
    let (_, signalq) = mm.get_signal_strength().await.unwrap();
    log::info!("Signal quality: {}%", signalq);
    mm.context_activate().await.unwrap();

    // ---- Use the modem as an async TCP socket --------------------------------
    // Share the modem behind a `CriticalSectionRawMutex`-backed async Mutex
    // (safe across tokio's multi-threaded runtime), then hand it to the client.
    let modem: Mutex<CriticalSectionRawMutex, _> = Mutex::new(mm);
    let client = QuectelTcpClient::new_tcp(&modem);

    log::info!(
        "Opening plain TCP socket to {}:{}",
        CONFIG.tcp_host,
        CONFIG.tcp_port
    );
    // `TcpConnect::connect` only takes a `SocketAddr` (an IP), so resolve the
    // configured host first, matching what a real client would do.
    let remote = tokio::net::lookup_host((CONFIG.tcp_host, CONFIG.tcp_port))
        .await
        .expect("Failed to resolve tcp_host")
        .next()
        .expect("No addresses for tcp_host");
    let mut sock = client
        .connect(remote)
        .await
        .expect("Failed to open TCP socket");

    // Build and send a minimal HTTP/1.1 request.
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: modem_manager_rs\r\n\
         Accept: */*\r\nConnection: close\r\n\r\n",
        CONFIG.tcp_path, CONFIG.tcp_host
    );
    log::info!("Sending HTTP request:\n{}", request);
    sock.write_all(request.as_bytes())
        .await
        .expect("Failed to send request");

    // Read the whole response. `Connection: close` makes the server close the
    // socket at the end, which the wrapper surfaces as `Ok(0)` (EOF).
    log::info!("Reading response...");
    let mut response = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        match sock.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(e) => {
                log::warn!("read ended: {:?}", e);
                break;
            }
        }
    }
    println!("{}", String::from_utf8_lossy(&response));
    log::info!("Received {} bytes total", response.len());

    sock.close()
        .await
        .unwrap_or_else(|e| log::error!("close failed: {:?}", e));

    let mut mm = modem.into_inner();
    mm.context_deactivate().await.unwrap();
    mm.power_off().await.unwrap();

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
