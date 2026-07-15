//! Dial the modem into **PPP data mode** and run a full `embassy_net` stack
//! over it, instead of the modem's own AT-command-driven socket engine (see
//! `tcp_socket.rs` / `http_get_tls.rs` for that path). Demonstrates three
//! things a real IP stack gives you that the AT socket engine doesn't: DNS
//! resolution, a UDP-based NTP time sync, and a TLS-secured TCP socket opened
//! by IP.
//!
//! This is the trickiest example in this repo because of one hard
//! constraint: once `ATD*99***<cid>#` succeeds, every byte on the UART is
//! raw PPP framing, not AT traffic. The atat `Client`/`Ingress` machinery
//! must stop touching the port and hand the raw duplex to `embassy-net-ppp`.
//! See `modem_manager_rs::ppp`'s module docs for the full explanation; the
//! short version:
//! - The write half is wrapped in `ppp::Reclaimable` *before* constructing
//!   the `atat::asynch::Client`, so it can be `.take()`n back out later.
//! - The read half is pumped by our own small cancellable task (NOT
//!   `AtatIngress::read_from`, which never returns) so it can be handed back
//!   once dialing succeeds.
//!
//! Usage:
//! ```text
//! cd examples/linux_simple
//! cp cfg.toml.example cfg.toml   # set ca_cert_path (a CA that signs google.com's chain)
//! cargo run --bin ppp_https /dev/ttyUSB4
//! ```

use std::env;
use std::time::{Duration, SystemTime};

use modem_manager_rs::cellular::{QuectelBG9X, INGRESS_BUF_SIZE, URC_CAPACITY, URC_SUBSCRIBERS};
use modem_manager_rs::ppp::{self, Duplex, PppResources, Reclaimable};
use modem_manager_rs::quectel_atat::types::{AuthenticationMethod, ModemConfiguration};
use modem_manager_rs::quectel_atat::urc::Urc;

use atat::asynch::Client;
use atat::{AtatIngress, Config as AtatConfig, DefaultDigester, Ingress, ResponseSlot, UrcChannel};

use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embedded_hal_mock::eh1::digital::{
    Mock as PinMock, State as PinState, Transaction as PinTransaction,
};
use embedded_io_adapters::tokio_1::FromTokio;
use embedded_io_async::Read as _;
use embedded_tls::webpki::CertVerifier;
use embedded_tls::{
    Aes128GcmSha256, Certificate, CryptoProvider, TlsConfig, TlsConnection, TlsContext, TlsError,
    TlsVerifier,
};
use sntpc::{get_time, NtpContext, NtpResult};
use sntpc_net_embassy::UdpSocketWrapper;
use sntpc_time_embassy::EmbassyTimestampGenerator;
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

    #[default("google.com")]
    dns_host: &'static str,
    #[default("/")]
    https_path: &'static str,
    /// NTP server to sync against over UDP once the PPP link is up.
    #[default("pool.ntp.org")]
    ntp_host: &'static str,
    /// Path to a local trust-anchor certificate (PEM) checked against the
    /// target's chain.
    ///
    /// This must be THE CERTIFICATE THAT DIRECTLY SIGNED the target's leaf
    /// cert -- typically its issuing *intermediate*, not the root. embedded-tls
    /// 0.19.0's webpki verifier has a known limitation (see its own
    /// `// TODO: Support intermediates...` in `src/webpki.rs`): it only ever
    /// checks `certificate.entries[0]` (the leaf) against the supplied trust
    /// anchor directly, ignoring any intermediate certificates the server
    /// also sent -- so supplying the real root (which signs the
    /// intermediate, not the leaf) fails with `UnknownIssuer` even though
    /// the chain is perfectly valid. For `google.com` as of this writing,
    /// that's the `WR2` intermediate (fetch fresh from
    /// `http://i.pki.goog/wr2.crt`, DER -- convert with
    /// `openssl x509 -inform DER -in wr2.crt -outform PEM -out wr2.pem`),
    /// not `GTS Root R1`. Find the right one for any target with
    /// `openssl s_client -connect <host>:443 -showcerts` -- it's the
    /// `issuer:` of certificate `0` (the leaf), i.e. entry `1` in the
    /// printed chain. Intermediates rotate more often than roots, so expect
    /// to have to refresh this occasionally.
    #[default("")]
    ca_cert_path: &'static str,

    #[default("")]
    serial_port: &'static str,
}

/// Crypto provider wiring `embedded-tls`'s webpki-based, CA-verified
/// hostname checking to `rand`'s `OsRng`. Mirrors `embedded-tls`'s own
/// `webpki_test.rs`.
struct WebPkiProvider<'a> {
    rng: rand::rngs::OsRng,
    verifier: CertVerifier<'a, Aes128GcmSha256, SystemTime, 4096>,
}

impl CryptoProvider for WebPkiProvider<'_> {
    type CipherSuite = Aes128GcmSha256;
    type Signature = &'static [u8];

    fn rng(&mut self) -> impl embedded_tls::CryptoRngCore {
        &mut self.rng
    }

    fn verifier(&mut self) -> Result<&mut impl TlsVerifier<Aes128GcmSha256>, TlsError> {
        Ok(&mut self.verifier)
    }
}

// `embassy_net::Stack`/`Runner` and `embassy_net_ppp::Runner` are `!Send`
// (they're designed for single-threaded executors, embassy's own or a
// current-thread one here) -- run everything on a `current_thread` runtime
// inside a `LocalSet`, and use `spawn_local` for the background tasks.
fn main() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime")
        .block_on(async {
            tokio::task::LocalSet::new().run_until(run()).await;
        });
}

async fn run() {
    let serial_port = env::args()
        .nth(1)
        .expect("Usage: cargo run --bin ppp_https <device>");
    // Defaults to `info`, but `RUST_LOG` overrides it (e.g.
    // `RUST_LOG=debug cargo run --bin ppp_https ...` to see atat's own
    // raw-byte traces of what the modem actually sends back).
        env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .init();

    if CONFIG.ca_cert_path.is_empty() {
        panic!(
            "ca_cert_path must be set in cfg.toml -- embedded-tls's webpki verifier checks \
             against exactly this one certificate, and (due to a known embedded-tls \
             limitation) it must be the intermediate that directly signed the target's leaf \
             cert, not the root. See the `ca_cert_path` doc comment on this file's Config \
             struct for how to find the right one."
        );
    }

    // The PWR_KEY GPIO is faked; the module is assumed to be already powered.
    let expectations = [
        PinTransaction::set(PinState::High),
        PinTransaction::set(PinState::Low),
    ];
    let modem_pwr_key = PinMock::new(&expectations);

    // Open the serial port and split it into tx/rx halves.
    let serial = tokio_serial::new(serial_port, 115_200)
        .open_native_async()
        .expect("Could not open serial port");
    let (serial_rx, serial_tx) = tokio::io::split(serial);

    // Wrap the writer so it can be reclaimed once AT-command use is done.
    let serial_tx = Reclaimable::new(FromTokio::new(serial_tx));
    let mut serial_rx = FromTokio::new(serial_rx);

    static INGRESS_BUF: StaticCell<[u8; INGRESS_BUF_SIZE]> = StaticCell::new();
    static RES_SLOT: ResponseSlot<INGRESS_BUF_SIZE> = ResponseSlot::new();
    static URC_CHANNEL: UrcChannel<Urc, URC_CAPACITY, URC_SUBSCRIBERS> = UrcChannel::new();
    // The modem's `CONNECT` reply to the PPP dial command is captured as
    // `Urc::FileDataModeStarted` (atat always matches URCs before command
    // responses), not as this digester's success path -- `dial_ppp()`
    // handles that internally via the URC channel. No custom digester needed
    // here, just the default.
    let mut ingress = Ingress::new(
        DefaultDigester::<Urc>::default(),
        INGRESS_BUF.init([0; INGRESS_BUF_SIZE]),
        &RES_SLOT,
        &URC_CHANNEL,
    );

    static BUF: StaticCell<[u8; 1024]> = StaticCell::new();
    let buf = BUF.init([0; 1024]);

    let client = Client::new(serial_tx, &RES_SLOT, buf, AtatConfig::default());

    // AT-phase reader: NOT `AtatIngress::read_from` (it's `-> !` and never
    // returns, so its owned reader could never be reclaimed). Our own pump
    // loop uses the same primitives internally, but can be cancelled and
    // handed the reader back.
    log::info!("Starting ATAT ingress pump...");
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    let pump = tokio::task::spawn_local(async move {
        loop {
            // Mirror `AtatIngress::read_from`'s own guard: if the ingress
            // buffer is full, `write_buf()` returns an empty slice, and
            // `read(&mut [])` returns `Ok(0)` immediately without touching
            // the OS (see `FromTokio::read`) -- without this check the loop
            // would busy-spin on that instant `Ok(0)` forever instead of
            // actually waiting on new bytes, starving this task's executor
            // thread of everything else, including timing out the send this
            // pump is supposed to be servicing.
            let buf = ingress.write_buf();
            if buf.is_empty() {
                log::warn!("Ingress buffer full, clearing");
                ingress.clear();
                continue;
            }
            tokio::select! {
                _ = &mut stop_rx => break,
                result = serial_rx.read(buf) => {
                    match result {
                        Ok(len) => {
                            if let Err(e) = ingress.try_advance(len) {
                                log::info!("Error advancing ingress {:?}", e);
                                ingress.clear();
                            }
                        }
                        Err(e) => {
                            log::info!("Error reading from UART: {:?}", e);
                            ingress.clear();
                        }
                    }
                }
            }
        }
        serial_rx.into_inner()
    });

    log::info!("Starting ATAT client...");
    let mut mm = match QuectelBG9X::new(modem_pwr_key.clone(), client, &URC_CHANNEL).await {
        Ok(mm) => mm,
        Err(e) => panic!("Error initializing modem: {:?}", e),
    };

    // Bring the modem online (EG916U uses automatic band/RAT selection).
    let mm_config = ModemConfiguration::new();
    mm.is_alive().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    mm.set_modem_funcionality(true).await.unwrap();
    mm.test_sim().await.expect("SIM test failed");
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

    // ---- Dial PPP and hand the raw UART to embassy-net-ppp -------------------
    log::info!("Dialing PPP...");
    mm.dial_ppp().await.expect("PPP dial failed");

    // Stop the AT-phase pump and reclaim both serial halves. No more AT
    // commands can be sent on this connection from here on.
    let _ = stop_tx.send(());
    let raw_read_half = pump.await.expect("ingress pump task panicked");
    let raw_write_half = mm.client_mut().inner().take();

    let transport = Duplex::new(
        FromTokio::new(tokio::io::BufReader::new(raw_read_half)),
        raw_write_half,
    );

    static PPP_RESOURCES: StaticCell<PppResources> = StaticCell::new();
    let resources = PPP_RESOURCES.init(PppResources::new());
    let seed = rand::random::<u64>();
    let (stack, mut ppp_runner, mut net_runner) = ppp::new_stack(resources, seed);

    log::info!("Starting PPP + network stack background tasks...");
    let stack_for_ipv4up = stack;
    tokio::task::spawn_local(async move {
        let ppp_config = embassy_net_ppp::Config {
            username: b"",
            password: b"",
        };
        let result = ppp_runner
            .run(transport, ppp_config, |status| {
                log::info!("PPP IPv4 up: {:?}", status);
                if let Some(cfg) = ppp::ppp_ipv4_to_stack_config(status) {
                    stack_for_ipv4up.set_config_v4(cfg);
                }
            })
            .await;
        log::error!("PPP link ended: {:?}", result);
    });
    tokio::task::spawn_local(async move {
        net_runner.run().await;
    });

    log::info!("Waiting for PPP IPCP to bring up an IP configuration...");
    stack.wait_config_up().await;
    log::info!("Stack config: {:?}", stack.config_v4());

    // ---- NTP: sync time over UDP, via DNS + embassy-net's udp::UdpSocket -----
    log::info!("Resolving NTP server {} via DNS...", CONFIG.ntp_host);
    let ntp_addrs = stack
        .dns_query(CONFIG.ntp_host, DnsQueryType::A)
        .await
        .expect("NTP DNS query failed");
    let ntp_ip = ntp_addrs
        .first()
        .copied()
        .expect("DNS returned no addresses for NTP host");
    let ntp_ip_std = match ntp_ip {
        embassy_net::IpAddress::Ipv4(v4) => std::net::IpAddr::V4(v4),
    };

    let mut ntp_rx_meta = [PacketMetadata::EMPTY; 16];
    let mut ntp_rx_buffer = [0u8; 512];
    let mut ntp_tx_meta = [PacketMetadata::EMPTY; 16];
    let mut ntp_tx_buffer = [0u8; 512];
    let mut ntp_socket = UdpSocket::new(
        stack,
        &mut ntp_rx_meta,
        &mut ntp_rx_buffer,
        &mut ntp_tx_meta,
        &mut ntp_tx_buffer,
    );
    ntp_socket.bind(0).expect("Failed to bind UDP socket");
    let ntp_socket = UdpSocketWrapper::new(ntp_socket);

    log::info!("Requesting time from {} ({})...", CONFIG.ntp_host, ntp_ip_std);
    let ntp_context = NtpContext::new(EmbassyTimestampGenerator::default());
    let ntp_result = get_time(
        std::net::SocketAddr::new(ntp_ip_std, 123),
        &ntp_socket,
        ntp_context,
    )
    .await;

    match ntp_result {
        Ok(result) => {
            log::info!(
                "NTP roundtrip {} us, offset {} us, stratum {} \
                 (roundtrip/2 is roughly the anchor's accuracy bound)",
                result.roundtrip,
                result.offset,
                result.stratum
            );
            log::info!(
                "res = {:?}",
                result
            );
        }
        Err(e) => {
            log::warn!("NTP sync failed: {:?}", e);
        }
    }

    // ---- DNS: resolve the target host -----------------------------------------
    log::info!("Resolving {} via DNS...", CONFIG.dns_host);
    let addrs = stack
        .dns_query(CONFIG.dns_host, DnsQueryType::A)
        .await
        .expect("DNS query failed");
    let ip = addrs.first().copied().expect("DNS returned no addresses");
    log::info!("{} resolved to {}", CONFIG.dns_host, ip);

    // ---- Open a TCP socket, then wrap it in a verified TLS connection --------
    let mut rx_buffer = [0u8; 4096];
    let mut tx_buffer = [0u8; 4096];
    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    log::info!("Opening TCP socket to {}:443...", ip);
    socket
        .connect((ip, 443))
        .await
        .expect("TCP connect failed");

    let ca_pem = std::fs::read_to_string(CONFIG.ca_cert_path)
        .unwrap_or_else(|e| panic!("Could not read CA cert {}: {e}", CONFIG.ca_cert_path));
    let ca_der = pem::parse(ca_pem).expect("Could not parse CA cert PEM").into_contents();

    let mut read_record_buffer = [0u8; 16384];
    let mut write_record_buffer = [0u8; 16384];
    let tls_config = TlsConfig::new().with_server_name(CONFIG.dns_host);
    let mut tls = TlsConnection::new(socket, &mut read_record_buffer, &mut write_record_buffer);

    log::info!("Performing TLS handshake (verified against {})...", CONFIG.ca_cert_path);
    tls.open(TlsContext::new(
        &tls_config,
        WebPkiProvider {
            rng: rand::rngs::OsRng,
            verifier: CertVerifier::new(Certificate::X509(&ca_der)),
        },
    ))
    .await
    .expect("TLS handshake failed");
    log::info!("TLS connection established -- secure TCP socket to {} is up", CONFIG.dns_host);

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: modem_manager_rs\r\n\
         Accept: */*\r\nConnection: close\r\n\r\n",
        CONFIG.https_path, CONFIG.dns_host
    );
    tls.write(request.as_bytes())
        .await
        .expect("Failed to send request");
    tls.flush().await.expect("Failed to flush request");

    log::info!("Reading response...");
    let mut response = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        match tls.read(&mut buf).await {
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

    let _ = tls.close().await;

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
