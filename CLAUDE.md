# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`modem-manager-rs` — a Rust `embedded-hal` driver for the Quectel BG95/BG96/EG916U cellular modems, built on top of the [`atat`](https://docs.rs/atat) AT-command crate. It supports two mutually exclusive runtime backends selected by feature — hosted `std` (default) and bare-metal `no_std` `embassy` — **both async**; they differ only in `no_std`-ness and which `embassy-time` backend is registered, not in blocking-vs-async (see the `compat` module below).

## Commands

```bash
cargo build                     # default = ["bg95", "std", "log"] -> hosted async build
cargo test                      # unit tests (std backend; in src/cellular/ and src/quectel_atat/types.rs)
cargo test <name>               # run a single test by name substring
cargo clippy

# no_std (embassy) build. Verify it against a bare-metal target so any
# accidental `std` leak fails the build. Needs `log` or `defmt` explicitly --
# neither is pulled in by `embassy` alone:
cargo build --no-default-features --features "bg95 embassy log" --target riscv32imac-unknown-none-elf
cargo build --no-default-features --features "bg96 embassy log" --target riscv32imac-unknown-none-elf
cargo build --no-default-features --features "eg916u embassy log" --target riscv32imac-unknown-none-elf
```

Two orthogonal, each-mutually-exclusive feature axes (both enforced by `compile_error!` in [src/lib.rs](src/lib.rs) — exactly one of each):
- **Chip**: `bg95` (default), `bg96`, or `eg916u`. bg95/bg96 are Cat-M/NB-IoT/GSM and differ only in band lists; `eg916u` is LTE Cat 1bis + GSM. Each chip's AT-command/quirk differences live in one file under [src/chip/](src/chip/) (see Architecture below). EG916U band masks / RAT config in [src/chip/eg916u.rs](src/chip/eg916u.rs) are **provisional** (`TODO(eg916u)`) — not datasheet-verified.
- **Runtime**: `std` (default) or `embassy` (`no_std`) — both async.

See [CONTRIBUTING.md](CONTRIBUTING.md) for a full walkthrough of adding a new chip.

### Running the examples

Examples are **separate crates** under `examples/*/` (each with its own `Cargo.toml` depending on the driver via `path = "../../"`), not `cargo run --example` targets. You must `cd` into them:

- `examples/linux_simple` — runs on a host with the modem on a USB serial adapter; the PWR_KEY GPIO is faked with `embedded-hal-mock`. Run with `cargo run <serial-device>` (e.g. `/dev/ttyUSB4`). Multiple binaries under `src/bin/` (`connect_and_send_mqtt`, `file_handling`, `tcp_socket`, `http_get_tls`, `ppp_https`) — select with `cargo run --bin <name> <device>`.
- `examples/esp32c3_connect_and_send_mqtt` — real firmware for an ESP32-C3, using the `std` backend (ESP-IDF provides a real hosted `std` environment even though it's a microcontroller). Uses a pinned nightly toolchain (`rust-toolchain.toml`), target `riscv32imc-esp-espidf`, and flashes via `espflash` (`cargo run` runs the espflash runner). Requires the ESP-IDF toolchain.
- `examples/stm32h7_ppp_https` — bare-metal firmware for an STM32H7, using the `embassy`/`no_std`/`defmt` backend and the `ppp` feature (dials the modem into PPP data mode and runs a full `embassy_net` stack over it, instead of the AT-command-driven socket engine in `src/tcp.rs`).

The `linux_simple` and `esp32c3_connect_and_send_mqtt` examples read runtime config (APN, MQTT broker, SSL) from a `cfg.toml` (copy from `cfg.toml.example`) via the `toml_cfg` crate at build time.

## Architecture

Three layers:

### 1. AT-command protocol layer — `src/quectel_atat/`
Pure declarative mapping of Quectel AT commands to Rust types via `atat`'s derive macros. No I/O logic here.
- `mod.rs` — command structs, each `#[derive(AtatCmd)]` + `#[at_cmd("...", ResponseType, timeout_ms = N)]`. Each field is an `#[at_arg(position = N)]`. This is where you add or edit an AT command. Command *shapes* here are chip-agnostic (identical struct for all chips) even where a command's semantics only apply to some chips — chip selection of *which* commands to send lives one layer up, in `src/chip/`.
- `responses.rs` — `#[derive(AtatResp)]` structs the modem replies with.
- `types.rs` — enums/newtypes used as command args (bands, RATs, SSL config, etc.) plus `ModemConfiguration`, a builder holding band masks and RAT search order (`.set_bands()`, `.set_rat_order()`). The `Band` trait's `all_bands_mask()` impls delegate to `crate::chip::ActiveChip`'s associated consts (see below) rather than branching inline. Individual `EmtcBands`/`NbIotBands` enum variants are still `#[cfg(feature = "bg95"/"bg96")]`-gated per-variant (which numeric bands even exist is a compile-time enum concern, not something a trait can express). Unit tests here verify band-mask / RAT encoding.
- `urc.rs` — `Urc` enum of Unsolicited Result Codes (async modem notifications like power-down, MQTT state changes) dispatched through `atat`'s `UrcChannel`. Chip-agnostic — no `#[cfg(feature = "bg95"/...)]` here.

### 2. Chip "plugin" layer — `src/chip/`
One `ChipProfile` trait impl per supported chip (`bg95.rs`, `bg96.rs`, `eg916u.rs`), selected at compile time via the `pub(crate) type ActiveChip = ...` alias in `mod.rs` (cfg'd on the chip feature). This is the `no_std`/static-dispatch analogue of Linux ModemManager's per-vendor plugin: MM resolves "which modem is this" at runtime via D-Bus/GObject interface probing across a dynamically discovered device set; here a binary only ever targets one chip, so it's resolved at compile time instead — no dynamic dispatch, no allocator.
- The trait: `configure_modem` (band/RAT/service-domain AT commands), `poll_attach_status` (one network-attach polling attempt, chip-specific AT command + response parsing, normalized to `Result<Option<ModemMode>, ModemError>`), `classify_revision` (firmware-version string → `ModemRevision`), and `needs_explicit_mqtt_close` (defaulted `false`; BG95 overrides it for the R200 firmware quirk).
- `classic.rs` holds the AT command sequence shared by BG95/BG96 (they're identical except for band-mask constants); `bg95.rs`/`bg96.rs` are thin wrappers supplying those constants plus their own revision table. `eg916u.rs` implements the trait directly (different commands entirely — `AT+COPS` instead of `AT+QNWINFO`, no per-RAT band config yet). EG916U's band masks carry `TODO(eg916u)` markers — **provisional**, not datasheet-verified.
- Adding a new chip = adding one file here + a Cargo feature; see [CONTRIBUTING.md](CONTRIBUTING.md).

### 3. Driver / state-machine layer — `src/cellular/`
`QuectelBG9X<W, OutputPinGeneric>` is the public driver, generic over a `Write` (`embedded_io_async::Write`, serial TX) and an `embedded_hal` `OutputPin` (the PWR_KEY GPIO) -- unparameterized by chip; chip selection happens inside method bodies via `crate::chip::ActiveChip`, not in the driver's type signature. It owns:
- the AT `Client` used to `send()` commands and get typed responses,
- a `&'static UrcChannel` it `.subscribe()`s to when it needs to wait for an async event (power-down, network registration, MQTT connect),
- detected `ModemRevision` (from `crate::chip`) and `ModemMode` state.

The struct, constructor and core lifecycle live in `src/cellular/mod.rs`; everything else is grouped into **capability modules**, each contributing an additional `impl QuectelBG9X` block in its own file — mirroring how ModemManager splits modem functionality into separate D-Bus interfaces (`Modem`, `Modem.3gpp`, `Modem.Location`, ...), but as plain inherent-impl modules rather than traits (only one type ever implements them, so a trait per capability would be pure ceremony):

| Module | Capability |
|---|---|
| `power.rs` | power on/off, `is_alive`, factory reset |
| `sim.rs` | SIM status (`AT+CPIN?`) |
| `config.rs` | modem functionality/band/RAT config (delegates to `ActiveChip::configure_modem`), network time |
| `registration.rs` | network attach/registration (delegates to `ActiveChip::poll_attach_status`) |
| `bearer.rs` | PDP context (bearer) config/activation, PPP dial |
| `mqtt.rs` | MQTT connect/publish/disconnect (delegates the R200 quirk to `ActiveChip::needs_explicit_mqtt_close`) |
| `socket.rs` | plain-TCP + TLS socket primitives, SSL context config |
| `gnss.rs` | GNSS control |
| `file.rs` | internal (UFS) flash file operations (SSL CA cert upload, etc.) |
| `digest.rs` | free-function atat digester hooks (see "Binary read framing" below) -- chip-agnostic |

Each capability file does `use super::*;` to inherit the imports declared once in `mod.rs` (glob-imported private items are visible to descendant modules per Rust's normal privacy rules, so this isn't re-exporting anything publicly).

Key behavioral points to preserve when editing:
- **Write the driver as `async fn` directly** -- there is no sync/async code-generation macro in this crate (despite older docs/READMEs describing one). `std` and `embassy` run the *same* async code; `std` is not blocking. `.await` on client sends, internal method calls, and delays exactly as `embassy` would.
- **All timing goes through the private `compat` module** (`src/cellular/mod.rs`). `compat::delay_ms`/`delay_secs` and `compat::Instant`/`compat::elapsed_ms` (a `u64`-millis monotonic clock) are backed by `embassy-time` in *both* runtimes (its `std` timer-queue backend under `std`, its embedded time driver under `embassy` -- selected by which Cargo feature is forwarded into the `embassy-time` dependency, not by any `#[cfg]` inside `compat` itself). Do NOT reintroduce `std::thread`, `std::time`, or `SystemTime` anywhere under `src/cellular/` or `src/chip/` -- they break the `no_std`/embassy build. Timeout loops are `while compat::elapsed_ms(now) < N { compat::delay_ms(..).await; ... }`; public durations use `core::time::Duration`.
- **`no_std` discipline.** No `std::`, no `format!`/`Vec`/`String` from `alloc` — use `core::` paths, `atat::heapless::{String, Vec}`, and `core::fmt::Write` (`write!`) into a heapless buffer. Timestamp parsing (`config.rs`) uses the compile-time `time::macros::format_description!` (no allocator).
- **URC waits are still poll-based in both runtimes.** `subscriber.try_next_message_pure()` is non-async in `atat`; the loops poll it with a `compat::delay_ms(..).await` between tries. This is intentional (identical behavior in both backends), not an oversight.
- **Firmware-revision branching goes through `ChipProfile`, not inline matches.** `update_module_revision()` (in `mod.rs`) calls `ActiveChip::classify_revision(version_str)`; behavior that varies per revision (e.g. `mqtt_disconnect`'s R200 quirk) calls `ActiveChip::needs_explicit_mqtt_close(self.rev)` rather than matching `self.rev` directly. When adding a modem-behavior workaround, add it to the relevant chip's `ChipProfile` impl in `src/chip/`, not as a new inline `#[cfg(feature = "...")]`/`if self.rev == ...` in `src/cellular/`.
- **Construction runs I/O.** `QuectelBG9X::new()` immediately powers on the modem and queries revision + IMEI, returning `Err(ModemError::NotResponding)` if the modem is silent. It is `async fn new(...).await` on both runtimes.

### Socket layer — `src/tcp.rs`
A single module, shared unchanged by both `std` and `embassy` (no per-runtime split -- there is no `src/tcp_std.rs`). Wraps the driver's socket methods and supports **both transports**, chosen per connection with the crate-level `Transport` enum (`Tcp` — plain `AT+QIOPEN`/`QISEND`/`QIRD`/`QICLOSE`; `Tls { ssl_ctx_id }` — modem-terminated `AT+QSSLOPEN`/`QSSLSEND`/`QSSLRECV`/`QSSLCLOSE`). The driver exposes `tcp_socket_open`/`send`/`recv`/`close` alongside the `ssl_socket_*` family (both in `src/cellular/socket.rs`); both read paths need the length-prefix digester hook (see below).

`QuectelTcpClient::new(modem, transport)` (or `new_tcp`/`new_tls`) implements `embedded-nal-async`'s `TcpConnect`; the returned `ModemSocket` (aliased `TlsSocket` for back-compat) implements `embedded_io_async::Read`/`Write`. The modem is shared behind an `embassy_sync::mutex::Mutex` (locked per op, works under both `std` and `embassy`) because `TcpConnect::connect` takes `&self`.

mTLS credentials live in the modem's UFS flash (uploaded via `upload_file_to_internal_flash`, wired through `SslConfiguration::set_client_cert`/`set_client_key` → `configure_ssl_context`). Known limitation: `TcpConnect::connect` only carries an IP (no hostname for SNI — use `ssl_socket_open`/the ergonomic `QuectelTcpClient::connect(host,…)` for that). See `examples/linux_simple/src/bin/tcp_socket.rs` for the wrapper end-to-end.

- **Binary read framing.** `AT+QIRD` and `AT+QSSLRECV` return `+<HDR>: <len>\r\n<len raw bytes>\r\n\r\nOK\r\n`, whose binary payload defeats atat's line/prompt-oriented `DefaultDigester` (it treats stray `>` / `\r\nOK\r\n`-looking bytes as prompts/terminators). `cellular::{ssl_recv_digest_hook, tcp_recv_digest_hook, socket_recv_digest_hook}` (defined in `src/cellular/digest.rs`, re-exported from `src/cellular/mod.rs`) are `with_custom_success` digester hooks that frame the payload by its length prefix; wire the combined `socket_recv_digest_hook` into the `Ingress` when using either socket type. Echo **must** be off (`is_powered_on` sends `ATE0`) or echoed send payloads corrupt these reads.

### atat runtime wiring (see examples for the canonical setup)
The caller, not the driver, owns the `atat` plumbing: an `Ingress` + `ResponseSlot` + `UrcChannel` (all `static`/`StaticCell` because they must be `'static`), a reader that feeds the serial RX into `ingress.write_buf()` and calls `ingress.try_advance()` (a background thread under `std`; an Embassy task calling `ingress.read_from(...).await` under `embassy`), and a `Client` built from the serial TX. The `UrcChannel` is passed by reference into the driver. Buffer sizes are the public consts `INGRESS_BUF_SIZE`, `URC_CAPACITY`, `URC_SUBSCRIBERS` in `src/cellular/mod.rs`. `examples/stm32h7_ppp_https` is the canonical `embassy` example; `examples/linux_simple` and `examples/esp32c3_connect_and_send_mqtt` use `std`.
