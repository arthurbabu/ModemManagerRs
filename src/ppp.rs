//! Second, lower-level networking path: dial the modem into PPP data mode
//! and run a full [`embassy_net::Stack`] over it, instead of the modem's own
//! AT-command-driven socket engine ([`crate::tcp`]). Gives real DNS, UDP, and
//! concurrent TCP sockets usable by any `embedded-nal-async`/
//! `embedded_io_async` consumer. Written once, generic over both runtimes
//! (`std` and `embassy`), same as the rest of the driver.
//!
//! # Flow
//! 1. Bring the modem up and dial as usual: [`crate::cellular::QuectelBG9X::new`],
//!    `set_context_configuration`, `network_attach`, then
//!    [`crate::cellular::QuectelBG9X::dial_ppp`].
//! 2. Reclaim the raw serial halves. Wrap the writer in [`Reclaimable`]
//!    *before* constructing the `atat::asynch::Client` you pass to
//!    `QuectelBG9X::new`, so you can `.take()` it back out via
//!    [`crate::cellular::QuectelBG9X::client_mut`] once dialing succeeds. The
//!    reader must be pumped by your own cancellable loop while in AT mode --
//!    **not** `atat::AtatIngress::read_from`, which is `-> !` and never
//!    returns, so its owned reader can never be reclaimed. See
//!    `examples/linux_simple/src/bin/ppp_https.rs` for the full pattern.
//! 3. Combine the reclaimed halves with [`Duplex`] and hand them, plus a
//!    [`PppResources`], to [`new_stack`].
//! 4. Spawn the two returned background futures (the PPP runner's `run()`
//!    and the network stack's own `run()`; both are non-terminating), then
//!    use the returned [`embassy_net::Stack`] like any other --
//!    `Stack::dns_query`, [`embassy_net::tcp::TcpSocket`], and
//!    `embedded-tls` for a TLS-secured socket.
//!
//! # TLS certificate verification
//! `embedded-tls`'s CA-chain verifier (`embedded_tls::webpki::CertVerifier`)
//! is std-only upstream. Both runtimes get the same `embedded-tls` transport
//! here; only `std` gets automatic, verified server-certificate checking out
//! of the box (wired in by this crate's `std` feature). An `embassy`/no_std
//! caller must supply their own `embedded_tls::CryptoProvider` (e.g.
//! `embedded_tls::UnsecureProvider` for an encrypted-but-unverified
//! connection, or a custom verifier) -- this module doesn't attempt to work
//! around that upstream limitation.
//!
//! # Known limitation: bytes lost in the AT-to-PPP handoff
//! If the modem's first read after `\r\nCONNECT\r\n` contains both that
//! terminator *and* the start of the peer's first LCP frame in the same
//! chunk, those leading PPP bytes are consumed into atat's internal ingress
//! buffer and are not recoverable before the handoff to [`Duplex`]. PPP's LCP
//! layer retransmits its Configure-Request on timeout, so this is self
//! healing in practice and not worth adding complexity to eliminate.
//!
//! # One-way trip
//! There is no API here (or on [`crate::cellular::QuectelBG9X::dial_ppp`]) to
//! escape back to AT command mode once dialed. Real hardware supports that
//! via a `+++` guard sequence, but wiring it back up is out of scope.

use embedded_io_async::{BufRead, ErrorType, Read, Write};

/// Wraps a writer so it can be reclaimed (taken back out) once AT-command use
/// of it is done.
///
/// Wrap the real serial writer in this *before* constructing
/// `atat::asynch::Client::new(...)`, so that after
/// [`crate::cellular::QuectelBG9X::dial_ppp`] succeeds you can call
/// `modem.client_mut().inner().take()` to get the raw writer back for
/// [`Duplex`]. Using the writer through this wrapper (`write`/`flush`) after
/// `.take()` panics -- by that point the driver must not send any more AT
/// commands anyway (see the module docs).
pub struct Reclaimable<W> {
    inner: Option<W>,
}

impl<W> Reclaimable<W> {
    /// Wrap `inner` for later reclamation.
    pub fn new(inner: W) -> Self {
        Self { inner: Some(inner) }
    }

    /// Take the wrapped writer back out.
    ///
    /// Panics if already taken.
    pub fn take(&mut self) -> W {
        self.inner.take().expect("Reclaimable already taken")
    }

    fn inner_mut(&mut self) -> &mut W {
        self.inner
            .as_mut()
            .expect("Reclaimable already taken; no more AT commands can be sent on this writer")
    }
}

impl<W: ErrorType> ErrorType for Reclaimable<W> {
    type Error = W::Error;
}

impl<W: Write> Write for Reclaimable<W> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.inner_mut().write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner_mut().flush().await
    }
}

/// Combines a separately-owned, reclaimed read half and write half into one
/// [`embedded_io_async::BufRead`] + [`embedded_io_async::Write`] transport --
/// the shape `embassy_net_ppp::Runner::run` expects.
///
/// `R` and `W` must share the same `Error` type (true of the halves produced
/// by splitting one duplex serial port, e.g. `tokio::io::split` +
/// `embedded_io_adapters::tokio_1::FromTokio`, which always uses
/// `std::io::Error`). If your read/write halves have different error types,
/// map one to the other before combining.
pub struct Duplex<R, W> {
    r: R,
    w: W,
}

impl<R, W> Duplex<R, W> {
    pub fn new(r: R, w: W) -> Self {
        Self { r, w }
    }
}

impl<R: ErrorType, W: ErrorType<Error = R::Error>> ErrorType for Duplex<R, W> {
    type Error = R::Error;
}

impl<R: Read, W: ErrorType<Error = R::Error>> Read for Duplex<R, W> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.r.read(buf).await
    }
}

impl<R: BufRead, W: ErrorType<Error = R::Error>> BufRead for Duplex<R, W> {
    async fn fill_buf(&mut self) -> Result<&[u8], Self::Error> {
        self.r.fill_buf().await
    }

    fn consume(&mut self, amt: usize) {
        self.r.consume(amt)
    }
}

impl<R: ErrorType, W: Write<Error = R::Error>> Write for Duplex<R, W> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.w.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.w.flush().await
    }
}

/// Buffer/socket resources for one PPP link, borrowed by [`new_stack`] for
/// the lifetime of the returned [`embassy_net::Stack`]. Hold this in a
/// `static` (embassy-style) or on the stack of the task that owns the link.
///
/// `N_RX`/`N_TX` size `embassy-net-ppp`'s internal packet channel; `SOCK`
/// sizes how many concurrent `embassy-net` sockets (TCP/UDP/DNS) the stack
/// supports. The defaults are a reasonable starting point for a handful of
/// concurrent connections.
pub struct PppResources<const N_RX: usize = 4, const N_TX: usize = 4, const SOCK: usize = 4> {
    ppp_state: embassy_net_ppp::State<N_RX, N_TX>,
    stack_resources: embassy_net::StackResources<SOCK>,
}

impl<const N_RX: usize, const N_TX: usize, const SOCK: usize> PppResources<N_RX, N_TX, SOCK> {
    pub const fn new() -> Self {
        Self {
            ppp_state: embassy_net_ppp::State::new(),
            stack_resources: embassy_net::StackResources::new(),
        }
    }
}

impl<const N_RX: usize, const N_TX: usize, const SOCK: usize> Default
    for PppResources<N_RX, N_TX, SOCK>
{
    fn default() -> Self {
        Self::new()
    }
}

/// Build an `embassy_net::Stack` driven by a PPP link over `resources`.
///
/// Returns the `Stack` handle -- usable immediately (`dns_query`,
/// `embassy_net::tcp::TcpSocket::new`, ...), though it won't have a route
/// until the PPP link actually comes up -- plus the two background runners
/// the caller must spawn as tasks:
/// - `ppp_runner.run(transport, ppp_config, on_ipv4_up)`, where `on_ipv4_up`
///   is typically `|status| stack.set_config_v4(ppp_ipv4_to_stack_config(status))`;
/// - `net_runner.run()`.
///
/// Both are non-terminating (`-> !` / `Result<Infallible, _>` that only
/// resolves on link failure).
///
/// `seed` seeds TCP initial sequence numbers / ephemeral port selection --
/// any reasonably-random `u64` (the RNG used for TLS is a fine source).
pub fn new_stack<'d, const N_RX: usize, const N_TX: usize, const SOCK: usize>(
    resources: &'d mut PppResources<N_RX, N_TX, SOCK>,
    seed: u64,
) -> (
    embassy_net::Stack<'d>,
    embassy_net_ppp::Runner<'d>,
    embassy_net::Runner<'d, embassy_net_ppp::Device<'d>>,
) {
    let (device, ppp_runner) = embassy_net_ppp::new(&mut resources.ppp_state);
    let (stack, net_runner) = embassy_net::new(
        device,
        embassy_net::Config::default(),
        &mut resources.stack_resources,
        seed,
    );
    (stack, ppp_runner, net_runner)
}

/// Convert the `Ipv4Status` delivered by `embassy_net_ppp::Runner::run`'s
/// `on_ipv4_up` callback (once IPCP negotiation completes) into the
/// `embassy_net::ConfigV4` needed for `Stack::set_config_v4`.
///
/// PPP is point-to-point, so the local address is given a `/32` prefix and
/// the peer's negotiated address is used as the (only) route/gateway --
/// there's no on-link subnet to speak of, unlike Ethernet/WiFi.
///
/// Returns `None` if the peer never negotiated a local IPv4 address (IPv4CP
/// failed or was rejected).
pub fn ppp_ipv4_to_stack_config(status: embassy_net_ppp::Ipv4Status) -> Option<embassy_net::ConfigV4> {
    let address = status.address?;
    let mut dns_servers = heapless::Vec::new();
    for dns in status.dns_servers.into_iter().flatten() {
        let _ = dns_servers.push(embassy_net::Ipv4Address::from(dns.octets()));
    }
    Some(embassy_net::ConfigV4::Static(embassy_net::StaticConfigV4 {
        address: embassy_net::Ipv4Cidr::new(embassy_net::Ipv4Address::from(address.octets()), 32),
        gateway: status
            .peer_address
            .map(|a| embassy_net::Ipv4Address::from(a.octets())),
        dns_servers,
    }))
}
