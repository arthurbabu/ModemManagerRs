//! Wrap the modem as a **standard TCP socket** and do an HTTP GET through it.
//!
//! This demonstrates [`quectel_bg9x_eh_driver::tcp_std`], the blocking socket
//! wrapper (the `std`/tokio-style counterpart of the embassy `embedded-nal-async`
//! wrapper in `quectel_bg9x_eh_driver::tcp`). The modem is shared behind a
//! `RefCell` and handed to a [`QuectelTcpClient`]; each `connect` returns a
//! [`QuectelTcpStream`] that implements [`std::io::Read`]/[`Write`] — so the HTTP
//! request/response below is written with the exact same calls you'd use on a
//! `std::net::TcpStream`.
//!
//! The same [`QuectelTcpStream`] also implements the `embedded_io` blocking
//! traits, and [`quectel_bg9x_eh_driver::tcp_std::QuectelTcpStack`] implements
//! `embedded_nal::TcpClientStack` (see [`run_via_embedded_nal`] at the bottom for
//! that surface). Pick the transport per connection with
//! [`Transport`](quectel_bg9x_eh_driver::Transport): `Tcp` (plain) or
//! `Tls { ssl_ctx_id }` (modem-terminated TLS).
//!
//! Usage:
//! ```text
//! cd examples/linux_simple
//! cargo run --bin tcp_socket /dev/ttyUSB4
//! ```
//! By default it does a plain-TCP GET to `example.com:80`. Set `tcp_host` /
//! `tcp_port` / `tcp_path` in `cfg.toml` to target something else.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::{env, thread, time};

use quectel_bg9x_eh_driver::cellular::{
    socket_recv_digest_hook, QuectelBG9X, INGRESS_BUF_SIZE, URC_CAPACITY, URC_SUBSCRIBERS,
};
use quectel_bg9x_eh_driver::quectel_atat::types::{AuthenticationMethod, ModemConfiguration};
use quectel_bg9x_eh_driver::quectel_atat::urc::Urc;
use quectel_bg9x_eh_driver::tcp_std::QuectelTcpClient;
use quectel_bg9x_eh_driver::Transport;

use atat::blocking::Client;
use atat::AtatIngress;
use atat::DefaultDigester;
use atat::Ingress;
use atat::{Config as AtatConfig, ResponseSlot, UrcChannel};

use embedded_hal_mock::eh1::digital::{
    Mock as PinMock, State as PinState, Transaction as PinTransaction,
};
use embedded_io::Read as _;
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

    // Plain-TCP HTTP target.
    #[default("example.com")]
    tcp_host: &'static str,
    #[default(80)]
    tcp_port: u16,
    #[default("/")]
    tcp_path: &'static str,
}

fn main() {
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

    // Open the serial port and split it into tx/rx halves.
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

    log::info!("Starting ATAT loop...");
    let _ = std::thread::spawn(move || loop {
        let buf = ingress.write_buf();
        match serial_rx.read(buf) {
            Ok(len) => match ingress.try_advance(len) {
                Ok(_) => {}
                Err(e) => {
                    log::info!("Error advancing ingress {:?}", e);
                    ingress.clear();
                }
            },
            Err(e) => {
                if e.kind() != std::io::ErrorKind::TimedOut {
                    log::info!("Error reading from UART: {:?}", e);
                }
                ingress.clear();
            }
        }
    });

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

    // Bring the modem online (EG916U uses automatic band/RAT selection).
    let mm_config = ModemConfiguration::new();
    mm.is_alive().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    mm.set_modem_funcionality(true).unwrap();
    mm.test_sim().unwrap_or_else(|_| {
        log::error!("SIM test failed, stopping execution.");
        let _ = mm.power_off();
        loop {
            thread::sleep(time::Duration::from_secs(1));
        }
    });
    mm.set_modem_configuration(mm_config).unwrap();
    mm.set_context_configuration(
        CONFIG.modem_apn,
        CONFIG.modem_user,
        CONFIG.modem_pass,
        AuthenticationMethod::try_from(CONFIG.modem_auth_method)
            .unwrap_or(AuthenticationMethod::None),
    )
    .unwrap();
    mm.network_attach().unwrap();
    let (_, signalq) = mm.get_signal_strength().unwrap();
    log::info!("Signal quality: {}%", signalq);
    mm.context_activate().unwrap();

    // ---- Use the modem as a std TCP socket ----------------------------------
    // Share the modem behind a RefCell so the socket client can borrow it per
    // operation, then hand it to the client.
    let modem = RefCell::new(mm);
    let client = QuectelTcpClient::new(&modem);

    log::info!(
        "Opening plain TCP socket to {}:{}",
        CONFIG.tcp_host,
        CONFIG.tcp_port
    );
    let mut sock = client
        .connect(CONFIG.tcp_host, CONFIG.tcp_port, Transport::Tcp)
        .expect("Failed to open TCP socket");

    // Build and send a minimal HTTP/1.1 request using std::io::Write.
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: quectel-bg9x-eh-driver\r\n\
         Accept: */*\r\nConnection: close\r\n\r\n",
        CONFIG.tcp_path, CONFIG.tcp_host
    );
    log::info!("Sending HTTP request:\n{}", request);
    sock.write_all(request.as_bytes())
        .expect("Failed to send request");

    // Read the whole response with std::io::Read. `Connection: close` makes the
    // server close the socket at the end, which the wrapper surfaces as EOF.
    log::info!("Reading response...");
    let mut response = Vec::new();
    match sock.read_to_end(&mut response) {
        Ok(_) => {}
        Err(e) => log::warn!("read ended: {e}"),
    }
    println!("{}", String::from_utf8_lossy(&response));
    log::info!("Received {} bytes total", response.len());

    sock.close().unwrap_or_else(|e| log::error!("close failed: {e}"));

    let mut mm = modem.into_inner();
    mm.context_deactivate().unwrap();
    mm.power_off().unwrap();

    loop {
        thread::sleep(time::Duration::from_secs(1));
    }
}

/// The same connection expressed through the portable
/// [`embedded_nal::TcpClientStack`] surface instead of `std::io`.
///
/// Not called by `main` (it needs an IP, not a hostname, per the trait), but
/// shown to document the third supported interface. `nb::block!` turns the
/// non-blocking `receive` into a blocking read.
#[allow(dead_code)]
fn run_via_embedded_nal<W: embedded_io::Write, P: embedded_hal::digital::OutputPin>(
    modem: &RefCell<QuectelBG9X<W, P>>,
) {
    use core::net::SocketAddr;
    use embedded_nal::TcpClientStack;
    use quectel_bg9x_eh_driver::tcp_std::QuectelTcpStack;

    let mut stack = QuectelTcpStack::new(modem, Transport::Tcp);
    let mut socket = stack.socket().unwrap();
    let remote: SocketAddr = "93.184.216.34:80".parse().unwrap();

    nb::block!(stack.connect(&mut socket, remote)).unwrap();
    nb::block!(stack.send(&mut socket, b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n"))
        .unwrap();

    let mut buf = [0u8; 512];
    loop {
        match nb::block!(stack.receive(&mut socket, &mut buf)) {
            Ok(n) => print!("{}", String::from_utf8_lossy(&buf[..n])),
            Err(_) => break, // PipeClosed => done
        }
    }
    stack.close(socket).unwrap();
}
