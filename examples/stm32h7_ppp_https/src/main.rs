//! PPP-over-cellular HTTPS demo for STM32H7 (written against a Nucleo-H743ZI;
//! adjust the chip feature in `Cargo.toml`, `memory.x`, and the pins/GPIO
//! below if yours differs).
//!
//! Shows the embedded (`embassy`, no_std) counterpart to
//! `examples/linux_simple/src/bin/ppp_https.rs`: wraps the modem behind
//! [`modem_manager_rs::net::CellularNetwork`] (power-on, attach, dial,
//! auto-reconnect) instead of hand-driving the AT/PPP sequence, then fetches
//! a URL over the resulting `embassy_net::Stack`.
//!
//! # The one genuinely new piece versus the `std` example: [`OpenSerial`]
//! `OpenSerial::open()` is called fresh on every reconnect cycle (the
//! previous session's `Reader`/`Writer` are fully consumed once a PPP
//! session ends -- see `modem_manager_rs::net`'s module docs). On `std`,
//! "reopen" means closing and reopening an OS file descriptor, which
//! `tokio-serial` does cheaply and correctly. A real UART has no such
//! operation: `embassy_stm32::usart::BufferedUart::new()` consumes owned,
//! genuinely `'static`-lifetime `Peri` tokens for the peripheral/pins, so it
//! can only be constructed once, ever -- and the physical link doesn't need
//! tearing down when only the modem's *logical* PPP session drops.
//!
//! [`Stm32Serial`]/[`UartHandle`] below split the UART exactly once at
//! startup and hand out a cheap pointer-backed proxy on every `open()` call
//! instead, reusing the same underlying `BufferedUartRx`/`BufferedUartTx`
//! forever. This is sound only because `net.rs`'s session loop is strictly
//! sequential (never two live transports at once) -- see the `# Safety`
//! comment on [`UartHandle`].

#![no_std]
#![no_main]

use core::convert::Infallible;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicBool, Ordering};

use defmt::info;
use embassy_executor::Spawner;
use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::peripherals::RNG as RngPeripheral;
use embassy_stm32::rng::Rng;
use embassy_stm32::usart::{BufferedUart, BufferedUartRx, BufferedUartTx, Config as UartConfig};
use embassy_stm32::{bind_interrupts, peripherals, rng, usart};
use embassy_time::Timer;
use embedded_io_async::{BufRead, ErrorType, Read, Write};
use embedded_tls::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, UnsecureProvider};
use modem_manager_rs::net::{
    CellularNetwork, CellularNetworkConfig, CellularNetworkTask, OpenSerial,
};
use modem_manager_rs::ppp::PppResources;
use modem_manager_rs::quectel_atat::types::AuthenticationMethod;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    // `BufferedInterruptHandler`, not the plain (DMA-based) `InterruptHandler`
    // -- `BufferedUart` drives its ring buffer off this one.
    USART2 => usart::BufferedInterruptHandler<peripherals::USART2>;
    RNG => rng::InterruptHandler<RngPeripheral>;
});

// APN/credentials for the PDP context -- replace with your SIM's values (or
// port the `toml_cfg`-based `cfg.toml` approach from `examples/linux_simple`
// if you want these configurable at build time instead of hardcoded).
const APN: &str = "internet";
const APN_USER: &str = "";
const APN_PASS: &str = "";

// `embedded-tls`'s CA-chain verifier is `std`-only upstream (see
// `modem_manager_rs::ppp`'s module docs), so this fetches over an *encrypted
// but unauthenticated* TLS connection (`UnsecureProvider`) -- fine for a
// demo against a host you trust the network path to, not for anything
// security-sensitive without supplying your own verifier.
const HTTPS_HOST: &str = "example.com";
const HTTPS_PATH: &str = "/";

/// Thin proxy around a raw pointer to a `'static` UART half, minted fresh by
/// [`Stm32Serial::open`] on every reconnect cycle. See the module docs for
/// why this exists instead of literally reopening the UART.
///
/// # Safety
/// At most one `UartHandle<T>` per half may be alive at a time. This holds
/// because `net.rs`'s session loop is strictly sequential: it fully awaits
/// (and therefore fully drops) one session's `Duplex` -- which owns the
/// previous `UartHandle`s -- before ever calling `open()` again. `in_use`
/// turns a violation of that invariant into a panic instead of undefined
/// behavior (two live `&mut` views of the same UART half).
struct UartHandle<T: 'static> {
    ptr: *mut T,
    in_use: &'static AtomicBool,
}

impl<T> Drop for UartHandle<T> {
    fn drop(&mut self) {
        self.in_use.store(false, Ordering::Release);
    }
}

impl<T: ErrorType> ErrorType for UartHandle<T> {
    type Error = T::Error;
}

impl<T: Read> Read for UartHandle<T> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        unsafe { &mut *self.ptr }.read(buf).await
    }
}

impl<T: BufRead> BufRead for UartHandle<T> {
    async fn fill_buf(&mut self) -> Result<&[u8], Self::Error> {
        unsafe { &mut *self.ptr }.fill_buf().await
    }

    fn consume(&mut self, amt: usize) {
        unsafe { &mut *self.ptr }.consume(amt)
    }
}

impl<T: Write> Write for UartHandle<T> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        unsafe { &mut *self.ptr }.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        unsafe { &mut *self.ptr }.flush().await
    }
}

/// [`OpenSerial`] for a USART split into its buffered halves exactly once at
/// startup (see `main`) -- `open()` just re-proxies the same halves instead
/// of re-initializing hardware.
struct Stm32Serial {
    rx: *mut BufferedUartRx<'static>,
    tx: *mut BufferedUartTx<'static>,
    rx_in_use: &'static AtomicBool,
    tx_in_use: &'static AtomicBool,
}

impl OpenSerial for Stm32Serial {
    type Reader = UartHandle<BufferedUartRx<'static>>;
    type Writer = UartHandle<BufferedUartTx<'static>>;
    // The UART itself is already up by the time `Stm32Serial` exists (see
    // `main`) -- there's no fallible reopen step to report an error from.
    type Error = Infallible;

    async fn open(&mut self) -> Result<(Self::Reader, Self::Writer), Self::Error> {
        assert!(
            !self.rx_in_use.swap(true, Ordering::AcqRel),
            "Stm32Serial::open called while the previous session's Reader was still alive"
        );
        assert!(
            !self.tx_in_use.swap(true, Ordering::AcqRel),
            "Stm32Serial::open called while the previous session's Writer was still alive"
        );
        Ok((
            UartHandle {
                ptr: self.rx,
                in_use: self.rx_in_use,
            },
            UartHandle {
                ptr: self.tx,
                in_use: self.tx_in_use,
            },
        ))
    }
}

#[embassy_executor::task]
async fn cellular_task(task: CellularNetworkTask<'static, Stm32Serial, Output<'static>>) {
    task.run().await;
}

/// DNS-resolve `host`, open a TCP socket to it on 443, and perform an
/// unverified TLS handshake (see the module docs) followed by a minimal
/// GET. Returns the number of response bytes read. Errors are collapsed to
/// `()` -- this is a demo, not production error handling; see
/// `examples/linux_simple/src/bin/ppp_https.rs`'s `FetchError` for a
/// `std`-side example of doing this properly.
async fn fetch_https(
    stack: Stack<'static>,
    rng: &mut Rng<'static, RngPeripheral>,
    host: &str,
    path: &str,
) -> Result<usize, ()> {
    let addrs = stack.dns_query(host, DnsQueryType::A).await.map_err(|_| ())?;
    let ip = addrs.first().copied().ok_or(())?;

    let mut rx_buffer = [0u8; 4096];
    let mut tx_buffer = [0u8; 4096];
    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    socket.connect((ip, 443)).await.map_err(|_| ())?;

    let mut read_record_buffer = [0u8; 16384];
    let mut write_record_buffer = [0u8; 16384];
    let tls_config = TlsConfig::new().with_server_name(host);
    let mut tls = TlsConnection::new(socket, &mut read_record_buffer, &mut write_record_buffer);
    tls.open(TlsContext::new(
        &tls_config,
        UnsecureProvider::new::<Aes128GcmSha256>(rng),
    ))
    .await
    .map_err(|_| ())?;

    let mut request = heapless::String::<256>::new();
    let _ = write!(
        request,
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );
    tls.write_all(request.as_bytes()).await.map_err(|_| ())?;

    let mut total = 0usize;
    let mut buf = [0u8; 512];
    loop {
        match tls.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(_) => break,
        }
    }
    let _ = tls.close().await;
    Ok(total)
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    // USART2, interrupt-driven ring buffer (no DMA channels needed). PA2/PA3
    // are the common USART2 AF7 pins on most H7 packages -- if this doesn't
    // compile, it's most likely an unsatisfied `RxPin<T>`/`TxPin<T>` bound
    // telling you those aren't valid USART2 pins on your exact part.
    static TX_BUF: StaticCell<[u8; 256]> = StaticCell::new();
    static RX_BUF: StaticCell<[u8; 512]> = StaticCell::new();
    let uart = BufferedUart::new(
        p.USART2,
        p.PA3,
        p.PA2,
        TX_BUF.init([0; 256]),
        RX_BUF.init([0; 512]),
        Irqs,
        UartConfig::default(), // 115200 8N1 -- matches the modem's default AT baud
    )
    .expect("USART2 init failed");
    let (tx, rx) = uart.split();

    static TX_HALF: StaticCell<BufferedUartTx<'static>> = StaticCell::new();
    static RX_HALF: StaticCell<BufferedUartRx<'static>> = StaticCell::new();
    let tx_ptr: *mut BufferedUartTx<'static> = TX_HALF.init(tx);
    let rx_ptr: *mut BufferedUartRx<'static> = RX_HALF.init(rx);

    static RX_IN_USE: AtomicBool = AtomicBool::new(false);
    static TX_IN_USE: AtomicBool = AtomicBool::new(false);
    let serial = Stm32Serial {
        rx: rx_ptr,
        tx: tx_ptr,
        rx_in_use: &RX_IN_USE,
        tx_in_use: &TX_IN_USE,
    };

    // PWR_KEY: adjust to whichever GPIO actually drives the modem's power
    // key pin on your wiring.
    let pwr_key = Output::new(p.PB0, Level::Low, Speed::Low);

    // Hardware RNG: seeds the PPP/TCP stack once, then is reborrowed
    // (`&mut Rng`) per TLS connection in the fetch loop below.
    let mut rng = Rng::new(p.RNG, Irqs);
    let seed = rng.next_u64();

    let config = CellularNetworkConfig {
        apn: APN,
        user: APN_USER,
        pass: APN_PASS,
        auth: AuthenticationMethod::None,
    };

    static PPP_RESOURCES: StaticCell<PppResources> = StaticCell::new();
    let ppp_resources = PPP_RESOURCES.init(PppResources::new());

    info!("Bringing up the cellular link (power-on, attach, dial)...");
    let (stack, network, task) =
        CellularNetwork::init(pwr_key, serial, ppp_resources, config, seed)
            .await
            .expect("failed to bring up the cellular link");

    spawner.spawn(cellular_task(task).unwrap());

    info!("Waiting for PPP IPCP to bring up an IP configuration...");
    stack.wait_config_up().await;
    info!("Stack config: {:?}", stack.config_v4());

    // `network` also exposes `.pause()`/`.resume()` (drop the link to save
    // power, redial later) and an async `.shutdown()` -- not wired to
    // anything here since this demo just runs forever, but see
    // `modem_manager_rs::net`'s module docs for the semantics of each.
    let mut iteration = 0u32;
    loop {
        iteration += 1;
        let status = network.status();
        info!(
            "[{}] modem status: RSSI {} dBm, {}%, session #{} | link up: {} | config up: {}",
            iteration,
            status.rssi_dbm,
            status.signal_percent,
            status.session_count,
            stack.is_link_up(),
            stack.is_config_up(),
        );

        if !stack.is_config_up() {
            info!("Link down -- waiting for the automatic reconnect...");
            stack.wait_config_up().await;
            info!("Link back up: {:?}", stack.config_v4());
        }

        match fetch_https(stack, &mut rng, HTTPS_HOST, HTTPS_PATH).await {
            Ok(n) => info!("fetch succeeded, {} bytes", n),
            Err(()) => info!("fetch failed; will check link status and retry next iteration"),
        }

        Timer::after_secs(30).await;
    }
}
