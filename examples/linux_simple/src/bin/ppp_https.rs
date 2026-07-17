//! Dial the modem into **PPP data mode** and run a full `embassy_net` stack
//! over it, instead of the modem's own AT-command-driven socket engine (see
//! `tcp_socket.rs` / `http_get_tls.rs` for that path). Demonstrates three
//! things a real IP stack gives you that the AT socket engine doesn't: DNS
//! resolution, a UDP-based NTP time sync, and a TLS-secured TCP socket opened
//! by IP.
//!
//! All the power-on/SIM/context/attach/dial/reconnect choreography is
//! wrapped by [`modem_manager_rs::net::CellularNetwork`] -- this example
//! only needs to implement [`modem_manager_rs::net::OpenSerial`] (how to
//! (re)open the serial port; the one thing that's fundamentally different
//! between `std`/`tokio` and `embassy`) and call `CellularNetwork::init`.
//! See that module's docs for what "status"/"pause"/"reconnect" actually
//! mean (cached status, full teardown+re-dial, not a live escape/resume).
//!
//! Usage:
//! ```text
//! cd examples/linux_simple
//! cp cfg.toml.example cfg.toml   # set ca_cert_path (a CA that signs google.com's chain)
//! cargo run --bin ppp_https /dev/ttyUSB4
//! ```

use std::env;
use std::time::{Duration, SystemTime};

use modem_manager_rs::net::{CellularNetwork, CellularNetworkConfig, OpenSerial};
use modem_manager_rs::ppp::PppResources;
use modem_manager_rs::quectel_atat::types::AuthenticationMethod;

use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embedded_io_adapters::tokio_1::FromTokio;
use embedded_tls::webpki::CertVerifier;
use embedded_tls::{
    Aes128GcmSha256, Certificate, CryptoProvider, TlsConfig, TlsConnection, TlsContext, TlsError,
    TlsVerifier,
};
use sntpc::{get_time, NtpContext};
use sntpc_net_embassy::UdpSocketWrapper;
use sntpc_time_embassy::EmbassyTimestampGenerator;
use static_cell::StaticCell;
use tokio_serial::SerialPortBuilderExt;

/// [`OpenSerial`] impl for `std`/`tokio`: (re)opens the device path fresh
/// each call, matching what [`CellularNetwork`]'s reconnect loop needs (a
/// serial port already consumed by a dropped PPP session can't be reused).
struct TokioSerial {
    path: String,
}

impl OpenSerial for TokioSerial {
    type Reader = FromTokio<tokio::io::BufReader<tokio::io::ReadHalf<tokio_serial::SerialStream>>>;
    type Writer = FromTokio<tokio::io::WriteHalf<tokio_serial::SerialStream>>;
    type Error = tokio_serial::Error;

    async fn open(&mut self) -> Result<(Self::Reader, Self::Writer), Self::Error> {
        let serial = tokio_serial::new(&self.path, 115_200).open_native_async()?;
        let (rx, tx) = tokio::io::split(serial);
        Ok((FromTokio::new(tokio::io::BufReader::new(rx)), FromTokio::new(tx)))
    }
}

/// Stand-in PWR_KEY GPIO: always succeeds. `CellularNetwork`'s reconnect
/// loop may power-cycle the modem an unbounded number of times, unlike the
/// other examples' `embedded_hal_mock` pin (which only ever models a single
/// power-on and would panic once its fixed expectation queue ran out) --  on
/// real hardware, use your actual GPIO type here instead.
struct NoopPin;

impl embedded_hal::digital::ErrorType for NoopPin {
    type Error = core::convert::Infallible;
}

impl embedded_hal::digital::OutputPin for NoopPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

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

/// Sync time over UDP against `ntp_host`, via DNS + `embassy-net`'s
/// `udp::UdpSocket`. Best-effort: logs and returns on any failure instead of
/// panicking, since a transient DNS/UDP failure here (e.g. right after a
/// reconnect, before the link has fully settled) shouldn't take the whole
/// example down.
async fn sync_ntp(stack: embassy_net::Stack<'static>, ntp_host: &str) {
    log::info!("Resolving NTP server {} via DNS...", ntp_host);
    let ntp_addrs = match stack.dns_query(ntp_host, DnsQueryType::A).await {
        Ok(addrs) => addrs,
        Err(e) => {
            log::warn!("NTP DNS query failed: {:?}", e);
            return;
        }
    };
    let Some(ntp_ip) = ntp_addrs.first().copied() else {
        log::warn!("DNS returned no addresses for NTP host");
        return;
    };
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
    if let Err(e) = ntp_socket.bind(0) {
        log::warn!("Failed to bind UDP socket for NTP: {:?}", e);
        return;
    }
    let ntp_socket = UdpSocketWrapper::new(ntp_socket);

    log::info!("Requesting time from {} ({})...", ntp_host, ntp_ip_std);
    let ntp_context = NtpContext::new(EmbassyTimestampGenerator::default());
    match get_time(std::net::SocketAddr::new(ntp_ip_std, 123), &ntp_socket, ntp_context).await {
        Ok(result) => log::info!(
            "NTP roundtrip {} us, offset {} us, stratum {} \
             (roundtrip/2 is roughly the accuracy bound)",
            result.roundtrip,
            result.offset,
            result.stratum
        ),
        Err(e) => log::warn!("NTP sync failed: {:?}", e),
    }
}

/// Everything that can go wrong in [`fetch_https`]. `Debug`-only (this is
/// example code, not a library) -- logged, not matched on, by the caller.
/// `dead_code` doesn't count `{:?}` logging as reading the fields, hence the
/// `allow`; they're genuinely printed at the `run()` call site.
#[derive(Debug)]
#[allow(dead_code)]
enum FetchError {
    Dns(embassy_net::dns::Error),
    NoAddress,
    Connect(embassy_net::tcp::ConnectError),
    ReadCert(std::io::Error),
    ParseCert(String),
    Tls(TlsError),
}

/// Resolve `host`, open a TCP socket to it on port 443, wrap it in a
/// webpki-verified TLS connection (see the `ca_cert_path` doc comment on
/// `Config` for why that CA must be an intermediate, not a root), and GET
/// `path`. Returns the response body.
///
/// Deliberately returns `Result` instead of panicking/`.expect()`-ing on
/// failure: any step here can fail transiently if the PPP link drops mid-call
/// (`CellularNetwork`'s reconnect loop will bring it back on its own, but
/// this call won't retry internally -- see the retry loop in `run` that
/// calls this).
async fn fetch_https(
    stack: embassy_net::Stack<'static>,
    host: &str,
    path: &str,
    ca_cert_path: &str,
) -> Result<Vec<u8>, FetchError> {
    log::info!("Resolving {} via DNS...", host);
    let addrs = stack
        .dns_query(host, DnsQueryType::A)
        .await
        .map_err(FetchError::Dns)?;
    let ip = addrs.first().copied().ok_or(FetchError::NoAddress)?;
    log::info!("{} resolved to {}", host, ip);

    let mut rx_buffer = [0u8; 4096];
    let mut tx_buffer = [0u8; 4096];
    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    log::info!("Opening TCP socket to {}:443...", ip);
    socket.connect((ip, 443)).await.map_err(FetchError::Connect)?;

    let ca_pem = std::fs::read_to_string(ca_cert_path).map_err(FetchError::ReadCert)?;
    let ca_der = pem::parse(ca_pem)
        .map_err(|e| FetchError::ParseCert(e.to_string()))?
        .into_contents();

    let mut read_record_buffer = [0u8; 16384];
    let mut write_record_buffer = [0u8; 16384];
    let tls_config = TlsConfig::new().with_server_name(host);
    let mut tls = TlsConnection::new(socket, &mut read_record_buffer, &mut write_record_buffer);

    log::info!("Performing TLS handshake (verified against {})...", ca_cert_path);
    tls.open(TlsContext::new(
        &tls_config,
        WebPkiProvider {
            rng: rand::rngs::OsRng,
            verifier: CertVerifier::new(Certificate::X509(&ca_der)),
        },
    ))
    .await
    .map_err(FetchError::Tls)?;
    log::info!("TLS connection established -- secure TCP socket to {} is up", host);

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: modem_manager_rs\r\n\
         Accept: */*\r\nConnection: close\r\n\r\n",
        path, host
    );
    tls.write(request.as_bytes()).await.map_err(FetchError::Tls)?;
    tls.flush().await.map_err(FetchError::Tls)?;

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
    let _ = tls.close().await;
    Ok(response)
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

    let net_config = CellularNetworkConfig {
        apn: CONFIG.modem_apn,
        user: CONFIG.modem_user,
        pass: CONFIG.modem_pass,
        auth: AuthenticationMethod::try_from(CONFIG.modem_auth_method)
            .unwrap_or(AuthenticationMethod::None),
    };
    // NOTE: band/RAT selection (`ModemConfiguration`/`set_modem_configuration`)
    // isn't yet exposed through `CellularNetworkConfig` -- fine for the
    // EG916U this example targets (it uses automatic band/RAT selection
    // regardless, per the other examples' own comments), but bg95/bg96
    // callers who need to narrow bands will have to wait for that to be
    // added, or call `set_modem_configuration` themselves before `dial_ppp`
    // if using `QuectelBG9X`/`crate::ppp` directly instead of this facade.

    static PPP_RESOURCES: StaticCell<PppResources> = StaticCell::new();
    let resources = PPP_RESOURCES.init(PppResources::new());
    let seed = rand::random::<u64>();

    log::info!("Bringing up the cellular network...");
    let (stack, network, task) = CellularNetwork::init(
        NoopPin,
        TokioSerial { path: serial_port },
        resources,
        net_config,
        seed,
    )
    .await
    .expect("Failed to bring up cellular network");
    tokio::task::spawn_local(task.run());

    log::info!("Waiting for PPP IPCP to bring up an IP configuration...");
    stack.wait_config_up().await;
    log::info!("Stack config: {:?}", stack.config_v4());

    sync_ntp(stack, CONFIG.ntp_host).await;

    // ---- Repeatedly fetch over HTTPS, demonstrating status monitoring and
    // automatic-reconnect resilience: if the PPP link drops between
    // iterations, `CellularNetwork`'s background task redials on its own
    // (see `net.rs`'s module docs -- full teardown+re-dial, not a live
    // resume, so a fresh `Stack` config/IP is expected after a drop). This
    // loop doesn't crash on a failed fetch; it just logs, waits for the
    // link to come back if it's down, and tries again next iteration.
    const ITERATIONS: u32 = 5;
    const FETCH_INTERVAL: Duration = Duration::from_secs(30);

    for i in 1..=ITERATIONS {
        // `network.status()` is the modem's own last-known signal status
        // (cached -- see `net.rs`'s module docs, not live while PPP is up).
        // `stack.is_link_up()`/`is_config_up()` are the IP stack's live view
        // of the PPP link itself. Together they answer "is the modem still
        // registered and how good is its signal" vs. "is my IP connectivity
        // currently usable" -- two different questions.
        log::info!(
            "[{}/{}] modem status: {:?} | PPP link up: {} | IP config up: {}",
            i,
            ITERATIONS,
            network.status(),
            stack.is_link_up(),
            stack.is_config_up(),
        );

        if !stack.is_config_up() {
            log::warn!("IP link is down -- waiting for the automatic reconnect to bring it back...");
            stack.wait_config_up().await;
            log::info!("Link back up: {:?}", stack.config_v4());
        }

        match fetch_https(stack, CONFIG.dns_host, CONFIG.https_path, CONFIG.ca_cert_path).await {
            Ok(body) => {
                println!("{}", String::from_utf8_lossy(&body));
                log::info!("[{}/{}] fetch succeeded ({} bytes)", i, ITERATIONS, body.len());
            }
            Err(e) => {
                log::warn!(
                    "[{}/{}] fetch failed: {:?} -- will check link status and retry next \
                     iteration",
                    i,
                    ITERATIONS,
                    e
                );
            }
        }

        if i < ITERATIONS {
            tokio::time::sleep(FETCH_INTERVAL).await;
        }
    }

    log::info!("Shutting down cellular network...");
    network.shutdown().await;
}
