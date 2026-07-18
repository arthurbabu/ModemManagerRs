//! High-level supervisor over the PPP path ([`crate::ppp`]): power on,
//! attach, dial, run the network stack, and automatically reconnect on
//! failure -- so a caller only needs to call [`CellularNetwork::init`] once
//! and spawn the returned [`CellularNetworkTask`], instead of hand-driving
//! the whole power-on/SIM/context/attach/dial/reclaim/wire-up sequence
//! itself (see `examples/linux_simple/src/bin/ppp_https.rs` before this
//! module existed, for what that looked like).
//!
//! # What this does and doesn't solve
//!
//! **Status is a cached, last-known value, not a live poll.** Once dialed,
//! the UART carries raw PPP framing -- there is no AT command access while a
//! session is up (see [`crate::cellular::QuectelBG9X::dial_ppp`]'s docs).
//! [`CellularNetwork::status`] returns whatever [`NetworkStatus`] was
//! captured the last time a session was (re)established, not a live RSSI
//! reading. Real hardware supports escaping back to AT mode mid-session via
//! `+++`/`ATO` without tearing down PPP, but that path is untested here and
//! deliberately not used.
//!
//! **Pause and reconnect-on-error are the same mechanism: full teardown,
//! then a full re-dial.** There is no live suspend/resume of an in-flight
//! PPP session -- [`CellularNetwork::pause`] stops driving PPP, and
//! [`CellularNetwork::resume`] (or an automatic reconnect after a link
//! failure) re-runs the entire power-on/SIM/context/attach/dial sequence
//! from scratch. This loses IP/session continuity across a pause (fresh
//! IPCP negotiation, likely a different address), but reuses only
//! already-proven code paths -- no new untested modem interaction.
//!
//! **`Drop` cannot gracefully power off the modem.** A clean shutdown
//! (`AT+QPOWD`) is an `async` operation, and `Drop::drop` cannot `.await`.
//! Use [`CellularNetwork::shutdown`] for a graceful, awaited power-off;
//! dropping the handle without calling it only logs a warning that cleanup
//! did not run (the modem stays powered until it's separately reset).
//!
//! **A failure of the modem's own power-on/detection (`QuectelBG9X::new`)
//! is unrecoverable.** Every other failure (SIM not ready, attach timeout,
//! dial failure, a dropped PPP link) happens with the modem already
//! constructed, so the power pin *and* the AT client can always be
//! reclaimed via [`crate::cellular::QuectelBG9X::release`] and reused for
//! the next attempt. But if `QuectelBG9X::new` itself never returns
//! successfully, both were moved into that failed call and cannot be
//! recovered (this matches `QuectelBG9X::new`'s existing behavior everywhere
//! else in the crate -- its error type carries neither back). The task logs
//! this as fatal and stops rather than silently spinning with nothing to
//! retry with.
//!
//! **Single instance.** This module's `static` storage (control signals,
//! cached status, the AT command/ingress buffers) is allocated once per
//! process; calling [`CellularNetwork::init`] a second time will panic (the
//! same [`static_cell::StaticCell`] double-init guard pattern used
//! throughout this crate).

use core::cell::Cell;

#[cfg(feature = "defmt")]
use defmt::{error, info, warn};
#[cfg(not(feature = "defmt"))]
use log::{error, info, warn};

// Wraps a `Debug`-only value (an error from a generic parameter like
// `S::Error`, or a foreign crate's error type) for the `{:?}` formatter --
// defmt's `{:?}` requires `defmt::Format`, not `core::fmt::Debug`, and we
// can't require every `OpenSerial` impl (or every dependency's error type) to
// derive it. `defmt::Debug2Format` bridges via the existing `Debug` impl at a
// small runtime cost; under `log`, `{:?}` already works on `Debug` directly.
#[cfg(feature = "defmt")]
macro_rules! fmt_dbg {
    ($e:expr) => {
        defmt::Debug2Format(&$e)
    };
}
#[cfg(not(feature = "defmt"))]
macro_rules! fmt_dbg {
    ($e:expr) => {
        $e
    };
}

use atat::asynch::Client;
use atat::{AtatIngress, Config as AtatConfig, DefaultDigester, Ingress, ResponseSlot, UrcChannel};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex as BlockingMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use embedded_hal::digital::OutputPin;
use embedded_io_async::{BufRead, Write};
use static_cell::StaticCell;

use crate::cellular::{QuectelBG9X, INGRESS_BUF_SIZE, URC_CAPACITY, URC_SUBSCRIBERS};
use crate::ppp::{self, Duplex, PppResources, Reclaimable};
use crate::quectel_atat::types::AuthenticationMethod;
use crate::quectel_atat::urc::Urc;
use crate::ModemError;

/// Caller-provided factory for (re)opening the serial transport.
///
/// [`CellularNetwork::init`] and its reconnect loop call this once per
/// session cycle instead of the caller managing reopen logic by hand --
/// opening a serial port is the one thing this module cannot do portably
/// itself (completely different mechanics between e.g. `tokio-serial` under
/// `std` and a HAL UART under `embassy`).
///
/// `Reader` must be `BufRead` (not just `Read`): both the AT-command ingress
/// pump and `embassy-net-ppp`'s `Runner::run` need it. Under `std`/tokio,
/// wrap in `tokio::io::BufReader` before adapting with
/// `embedded_io_adapters::tokio_1::FromTokio` (see the `ppp_https` example).
// `async fn` in a public trait is discouraged upstream because it can't
// express a `Send` bound on the returned future -- irrelevant here, since
// this whole module already requires a single-threaded/`LocalSet`-style
// executor (see `CellularNetworkTask`'s docs), same tradeoff
// `embedded_io_async::{Read, Write}` themselves make.
#[allow(async_fn_in_trait)]
pub trait OpenSerial {
    /// Read half of the transport. Must support `BufRead` (buffering, not
    /// just raw `Read`) -- see the trait docs above.
    type Reader: BufRead;
    /// Write half of the transport. Must report the *same* `Error` type as
    /// [`Self::Reader`] -- required by [`crate::ppp::Duplex`], which
    /// combines the two into one transport. True of the halves produced by
    /// splitting one duplex serial port (e.g. `tokio::io::split` +
    /// `embedded_io_adapters::tokio_1::FromTokio`, which always reports
    /// `std::io::Error` for both halves).
    type Writer: Write<Error = <Self::Reader as embedded_io_async::ErrorType>::Error>;
    /// Error type for [`Self::open`].
    type Error: core::fmt::Debug;

    /// Open (or reopen) the serial transport, split into read/write halves.
    async fn open(&mut self) -> Result<(Self::Reader, Self::Writer), Self::Error>;
}

/// APN/credentials for the PDP context. The context id is always 1, matching
/// the rest of the driver's existing convention
/// (`set_context_configuration`/`context_activate`/`context_deactivate` all
/// hardcode it).
#[derive(Clone, Copy, Debug)]
pub struct CellularNetworkConfig<'a> {
    pub apn: &'a str,
    pub user: &'a str,
    pub pass: &'a str,
    pub auth: AuthenticationMethod,
}

/// Last-known modem signal status, refreshed each time a session is
/// (re)established. **Not live** while a session is up -- see the module
/// docs.
#[derive(Clone, Copy, Debug, Default)]
pub struct NetworkStatus {
    pub rssi_dbm: i16,
    pub signal_percent: u8,
    /// Number of sessions successfully established so far (the initial
    /// connect counts as 1). Useful for telling a fresh connection apart
    /// from a long-lived one that's been reconnecting.
    pub session_count: u32,
}

/// Error from [`CellularNetwork::init`]: either the modem/AT layer (see
/// [`ModemError`]) or the caller's [`OpenSerial::Error`].
#[derive(Debug)]
pub enum CellularNetworkError<E> {
    Modem(ModemError),
    Serial(E),
}

impl<E> From<ModemError> for CellularNetworkError<E> {
    fn from(e: ModemError) -> Self {
        Self::Modem(e)
    }
}

/// Pause/resume/shutdown command sent from [`CellularNetwork`] to its
/// [`CellularNetworkTask`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Pause,
    Resume,
    Shutdown,
}

type ControlSignal = Signal<CriticalSectionRawMutex, Control>;
type DoneSignal = Signal<CriticalSectionRawMutex, ()>;
type StatusCell = BlockingMutex<CriticalSectionRawMutex, Cell<NetworkStatus>>;
type CellularIngress =
    Ingress<'static, DefaultDigester<Urc>, Urc, INGRESS_BUF_SIZE, URC_CAPACITY, URC_SUBSCRIBERS>;

// Module-scoped (not per-`CellularNetwork`, matching the single-instance
// design): the response slot and URC channel are just shared, reusable
// state across every session cycle. `INGRESS_BUF`/`CMD_BUF` back the one
// `Ingress`/`Client` built on the *first* session (see `run_session`) and
// reused (not rebuilt) on every reconnect after that -- `QuectelBG9X` bakes
// in `Client<'static, ...>`, so these must be genuine `'static` storage, and
// `StaticCell::init` can only ever be called once anyway, which lines up
// exactly with "build once, reuse forever".
static URC_CHANNEL: UrcChannel<Urc, URC_CAPACITY, URC_SUBSCRIBERS> = UrcChannel::new();
static RES_SLOT: ResponseSlot<INGRESS_BUF_SIZE> = ResponseSlot::new();
static INGRESS_BUF: StaticCell<[u8; INGRESS_BUF_SIZE]> = StaticCell::new();
static CMD_BUF: StaticCell<[u8; 1024]> = StaticCell::new();

/// Handle for querying status and controlling a running
/// [`CellularNetworkTask`]. See the module docs for what "pause"/"status"
/// actually mean here (not a live escape/resume, not a live RSSI poll).
pub struct CellularNetwork {
    control: &'static ControlSignal,
    shutdown_done: &'static DoneSignal,
    status: &'static StatusCell,
    shut_down: bool,
}

impl CellularNetwork {
    /// Last-known signal status (see the module docs: cached, not live).
    pub fn status(&self) -> NetworkStatus {
        self.status.lock(Cell::get)
    }

    /// Request a pause: the task stops driving PPP and powers the modem
    /// down, then waits for [`Self::resume`] (or [`Self::shutdown`]).
    /// Returns immediately; the pause takes effect once the task notices.
    pub fn pause(&self) {
        self.control.signal(Control::Pause);
    }

    /// Resume after a [`Self::pause`]: the task re-runs the full
    /// power-on/attach/dial sequence from scratch (see the module docs --
    /// this is not a live resume, the session/IP will be new).
    pub fn resume(&self) {
        self.control.signal(Control::Resume);
    }

    /// Gracefully power off the modem and stop the task. Awaits
    /// confirmation that shutdown actually completed.
    pub async fn shutdown(mut self) {
        self.control.signal(Control::Shutdown);
        self.shutdown_done.wait().await;
        self.shut_down = true;
    }
}

impl Drop for CellularNetwork {
    fn drop(&mut self) {
        if !self.shut_down {
            warn!(
                "CellularNetwork dropped without calling shutdown().await -- the modem was not \
                 gracefully powered off and the reconnect task may still be running."
            );
        }
    }
}

/// The background task driving a [`CellularNetwork`]. Spawn this once (e.g.
/// `spawn_local`, same `!Send` constraint as `embassy_net`/`embassy_net_ppp`'s
/// own runners) and forget it -- control happens through the
/// [`CellularNetwork`] handle, not by holding onto this.
pub struct CellularNetworkTask<'d, S: OpenSerial, P: OutputPin> {
    serial_factory: S,
    power_pin: P,
    client: Client<'static, Reclaimable<S::Writer>, INGRESS_BUF_SIZE>,
    ingress: CellularIngress,
    config: CellularNetworkConfig<'static>,
    ppp_runner: embassy_net_ppp::Runner<'d>,
    /// `embassy-net`'s own poller (distinct from the PPP side above) --
    /// must keep running for the `Stack`'s whole lifetime independent of PPP
    /// session cycling, so `run()` drives it concurrently with the
    /// reconnect loop via `select` rather than the caller spawning it
    /// separately.
    net_runner: embassy_net::Runner<'d, embassy_net_ppp::Device<'d>>,
    stack: embassy_net::Stack<'d>,
    control: &'static ControlSignal,
    shutdown_done: &'static DoneSignal,
    status: &'static StatusCell,
    /// The already-dialed transport from [`CellularNetwork::init`]'s own
    /// first session, consumed on the task's first loop iteration instead
    /// of dialing a second time.
    initial_transport: Option<Duplex<S::Reader, S::Writer>>,
}

/// Push the `Ipv4Status` delivered on IPCP completion into `stack`.
fn apply_ipv4_up(stack: embassy_net::Stack<'_>, status: embassy_net_ppp::Ipv4Status) {
    if let Some(cfg) = ppp::ppp_ipv4_to_stack_config(status) {
        stack.set_config_v4(cfg);
    }
}

/// Outcome of one session-setup attempt (see [`run_session`]).
enum SessionOutcome<P, T, E> {
    /// Connected: the reclaimed power pin and the ready-to-drive transport.
    Connected(P, T),
    /// Setup failed after `QuectelBG9X::new` succeeded -- the pin (and the
    /// client, stashed back into `client_slot`) were reclaimed via
    /// `release()` and can be reused for another attempt.
    Failed(P, E),
    /// Setup failed at (or before) `QuectelBG9X::new` itself -- the power
    /// pin (and, on the very first call, the client) are unrecoverable
    /// (moved into the failed call). Fatal; see the module docs.
    Unrecoverable(E),
}

/// Run one session: (re)open the serial transport, power on and bring up
/// the modem, dial PPP, and return the outcome. Updates `status` on success.
///
/// `client_slot` holds the reusable `Client` between calls: `None` only on
/// the very first ever call (constructed fresh, claiming `CMD_BUF`/
/// `RES_SLOT`), `Some` on every call after (its writer refilled via
/// [`Reclaimable::put`] instead of rebuilding the whole `Client`). `ingress`
/// is likewise reused across calls, just re-pointed at the new reader each
/// time via `AtatIngress::read_from`.
async fn run_session<S: OpenSerial, P: OutputPin>(
    serial_factory: &mut S,
    power_pin: P,
    client_slot: &mut Option<Client<'static, Reclaimable<S::Writer>, INGRESS_BUF_SIZE>>,
    ingress: &mut CellularIngress,
    config: &CellularNetworkConfig<'_>,
    status: &'static StatusCell,
) -> SessionOutcome<P, Duplex<S::Reader, S::Writer>, CellularNetworkError<S::Error>> {
    let (mut reader, writer) = match serial_factory.open().await {
        Ok(v) => v,
        Err(e) => return SessionOutcome::Failed(power_pin, CellularNetworkError::Serial(e)),
    };

    let client = match client_slot.take() {
        Some(mut c) => {
            c.inner().put(writer);
            c
        }
        None => {
            let cmd_buf = CMD_BUF.init([0; 1024]);
            Client::new(Reclaimable::new(writer), &RES_SLOT, cmd_buf, AtatConfig::default())
        }
    };

    enum SetupError<P, C> {
        NoPin(ModemError),
        WithPin(P, C, ModemError),
    }

    // Races the AT-command ingress pump (which never returns on its own)
    // against the whole setup+dial sequence; once the sequence finishes,
    // `select` drops the pump future and we get `reader` back, still fully
    // owned (only ever lent to the pump as `&mut`).
    let setup = async {
        let mut modem = match QuectelBG9X::new(power_pin, client, &URC_CHANNEL).await {
            Ok(m) => m,
            Err(e) => return Err(SetupError::NoPin(e)),
        };

        let result: Result<(i16, u8), ModemError> = async {
            modem.is_alive().await?;
            modem.set_modem_funcionality(true).await?;
            modem.test_sim().await?;
            modem
                .set_context_configuration(config.apn, config.user, config.pass, config.auth)
                .await?;
            modem.network_attach().await?;
            let signal = modem.get_signal_strength().await?;
            modem.dial_ppp().await?;
            Ok(signal)
        }
        .await;

        match result {
            Ok((rssi_dbm, signal_percent)) => {
                let session_count = status.lock(|c| {
                    let mut s = c.get();
                    s.rssi_dbm = rssi_dbm;
                    s.signal_percent = signal_percent;
                    s.session_count += 1;
                    c.set(s);
                    s.session_count
                });
                info!(
                    "Cellular session #{} up (RSSI {} dBm, {}%)",
                    session_count, rssi_dbm, signal_percent
                );
                let writer = modem.client_mut().inner().take();
                let (pin, client) = modem.release();
                Ok((pin, client, writer))
            }
            Err(e) => {
                let (pin, client) = modem.release();
                Err(SetupError::WithPin(pin, client, e))
            }
        }
    };

    match select(ingress.read_from(&mut reader), setup).await {
        Either::First(_) => unreachable!("Ingress::read_from never returns"),
        Either::Second(Ok((pin, client, writer))) => {
            *client_slot = Some(client);
            SessionOutcome::Connected(pin, Duplex::new(reader, writer))
        }
        Either::Second(Err(SetupError::WithPin(pin, client, e))) => {
            *client_slot = Some(client);
            SessionOutcome::Failed(pin, e.into())
        }
        Either::Second(Err(SetupError::NoPin(e))) => SessionOutcome::Unrecoverable(e.into()),
    }
}

impl<'d, S: OpenSerial, P: OutputPin> CellularNetworkTask<'d, S, P> {
    /// Run the reconnect supervisor loop until [`CellularNetwork::shutdown`]
    /// is called (or an unrecoverable failure occurs -- see the module
    /// docs). See the module docs for the pause/reconnect model.
    pub async fn run(self) {
        let CellularNetworkTask {
            mut serial_factory,
            mut power_pin,
            client,
            mut ingress,
            config,
            mut ppp_runner,
            mut net_runner,
            stack,
            control,
            shutdown_done,
            status,
            initial_transport,
        } = self;
        let mut client_slot = Some(client);

        // `net_runner.run()` never returns (`-> !`) and must keep polling
        // the stack for the caller's `Stack` handle to work at all,
        // independent of PPP session cycling -- race it against the
        // reconnect supervisor loop below so both run concurrently under
        // this one task, instead of the caller having to spawn a second one
        // (as the pre-`net.rs` `ppp_https.rs` did).
        let supervisor = async {
            let mut backoff = Duration::from_secs(2);
            const BACKOFF_MAX: Duration = Duration::from_secs(60);
            let mut pending_transport = initial_transport;

            loop {
                if control.try_take() == Some(Control::Shutdown) {
                    shutdown_done.signal(());
                    return;
                }

                let transport = if let Some(t) = pending_transport.take() {
                    t
                } else {
                    match run_session(
                        &mut serial_factory,
                        power_pin,
                        &mut client_slot,
                        &mut ingress,
                        &config,
                        status,
                    )
                    .await
                    {
                        SessionOutcome::Connected(pin, t) => {
                            power_pin = pin;
                            t
                        }
                        SessionOutcome::Failed(pin, e) => {
                            power_pin = pin;
                            warn!(
                                "Cellular session setup failed: {:?}; retrying...",
                                fmt_dbg!(e)
                            );
                            Timer::after(backoff).await;
                            backoff = core::cmp::min(backoff * 2, BACKOFF_MAX);
                            continue;
                        }
                        SessionOutcome::Unrecoverable(e) => {
                            error!(
                                "Modem initialization failed unrecoverably (power-control pin \
                                 lost): {:?}; stopping.",
                                fmt_dbg!(e)
                            );
                            return;
                        }
                    }
                };
                backoff = Duration::from_secs(2);

                let ppp_config = embassy_net_ppp::Config {
                    username: b"",
                    password: b"",
                };
                let run_fut =
                    ppp_runner.run(transport, ppp_config, |ipv4| apply_ipv4_up(stack, ipv4));

                match select(run_fut, control.wait()).await {
                    Either::First(result) => {
                        warn!("PPP link ended: {:?}; reconnecting...", fmt_dbg!(result));
                        Timer::after(backoff).await;
                        backoff = core::cmp::min(backoff * 2, BACKOFF_MAX);
                    }
                    Either::Second(Control::Shutdown) => {
                        shutdown_done.signal(());
                        return;
                    }
                    Either::Second(Control::Pause) => loop {
                        match control.wait().await {
                            Control::Resume => break,
                            Control::Shutdown => {
                                shutdown_done.signal(());
                                return;
                            }
                            Control::Pause => continue,
                        }
                    },
                    Either::Second(Control::Resume) => {
                        // Not paused; nothing to do.
                    }
                }
            }
        };

        select(net_runner.run(), supervisor).await;
    }
}

impl CellularNetwork {
    /// Power on the modem, attach to the network, and dial PPP -- the same
    /// sequence [`crate::cellular::QuectelBG9X`]/[`crate::ppp`] callers have
    /// always had to hand-drive, wrapped up here. Returns the network
    /// `Stack` (usable immediately, though it has no route until the
    /// spawned task's IPCP negotiation completes -- await
    /// `stack.wait_config_up()` after spawning), a [`CellularNetwork`]
    /// handle, and a [`CellularNetworkTask`] to spawn.
    ///
    /// `ppp_resources` must be a `'static` (e.g. `StaticCell`-backed, see
    /// [`crate::ppp::PppResources`]'s own docs) set of buffers -- the
    /// `Stack`/PPP `Runner` returned here borrow it for their whole
    /// lifetime, across every reconnect.
    ///
    /// Only one `CellularNetwork` may exist per process (see the module
    /// docs) -- calling this twice panics.
    pub async fn init<
        S: OpenSerial,
        P: OutputPin,
        const N_RX: usize,
        const N_TX: usize,
        const SOCK: usize,
    >(
        power_pin: P,
        mut serial_factory: S,
        ppp_resources: &'static mut PppResources<N_RX, N_TX, SOCK>,
        config: CellularNetworkConfig<'static>,
        seed: u64,
    ) -> Result<
        (
            embassy_net::Stack<'static>,
            CellularNetwork,
            CellularNetworkTask<'static, S, P>,
        ),
        CellularNetworkError<S::Error>,
    > {
        static CONTROL: ControlSignal = Signal::new();
        static SHUTDOWN_DONE: DoneSignal = Signal::new();
        static STATUS: StatusCell = BlockingMutex::new(Cell::new(NetworkStatus {
            rssi_dbm: 0,
            signal_percent: 0,
            session_count: 0,
        }));

        let ingress_buf = INGRESS_BUF.init([0; INGRESS_BUF_SIZE]);
        let mut ingress = Ingress::new(
            DefaultDigester::<Urc>::default(),
            ingress_buf,
            &RES_SLOT,
            &URC_CHANNEL,
        );
        let mut client_slot = None;

        let (power_pin, transport) = match run_session(
            &mut serial_factory,
            power_pin,
            &mut client_slot,
            &mut ingress,
            &config,
            &STATUS,
        )
        .await
        {
            SessionOutcome::Connected(pin, t) => (pin, t),
            SessionOutcome::Failed(_, e) => return Err(e),
            SessionOutcome::Unrecoverable(e) => return Err(e),
        };
        let client = client_slot.expect("run_session always fills client_slot on success");

        let (stack, ppp_runner, net_runner) = ppp::new_stack(ppp_resources, seed);

        let task = CellularNetworkTask {
            serial_factory,
            power_pin,
            client,
            ingress,
            config,
            ppp_runner,
            net_runner,
            stack,
            control: &CONTROL,
            shutdown_done: &SHUTDOWN_DONE,
            status: &STATUS,
            initial_transport: Some(transport),
        };

        let handle = CellularNetwork {
            control: &CONTROL,
            shutdown_done: &SHUTDOWN_DONE,
            status: &STATUS,
            shut_down: false,
        };

        Ok((stack, handle, task))
    }
}
