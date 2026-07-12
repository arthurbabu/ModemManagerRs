# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`quectel-bg9x-eh-driver` — a Rust `embedded-hal` driver for the Quectel BG95/BG96 cellular modems, built on top of the [`atat`](https://docs.rs/atat) AT-command crate. It supports two mutually exclusive runtime backends selected by feature: a **blocking `std`** backend (default) and an **async `no_std` embassy** backend.

## Commands

```bash
cargo build                     # default = ["bg95", "std"] -> blocking host build
cargo test                      # unit tests (std backend; in src/cellular.rs and src/quectel_atat/types.rs)
cargo test <name>               # run a single test by name substring
cargo clippy

# Async no_std (embassy) build. Verify it against a bare-metal target so any
# accidental `std` leak fails the build:
cargo build --no-default-features --features "bg95 embassy" --target riscv32imac-unknown-none-elf
cargo build --no-default-features --features "bg96 embassy" --target riscv32imac-unknown-none-elf
```

Two orthogonal, each-mutually-exclusive feature axes:
- **Chip**: `bg95` (default) or `bg96` — only changes the compiled-in NB-IoT / eMTC band lists. Never enable both.
- **Runtime**: `std` (default, blocking) or `embassy` (async, `no_std`). Enabling both or neither is a `compile_error!` (see [src/lib.rs](src/lib.rs)).

### Running the examples

Examples are **separate crates** under `examples/*/` (each with its own `Cargo.toml` depending on the driver via `path = "../../"`), not `cargo run --example` targets. You must `cd` into them:

- `examples/linux_simple` — runs on a host with the modem on a USB serial adapter; the PWR_KEY GPIO is faked with `embedded-hal-mock`. Run with `cargo run <serial-device>` (e.g. `/dev/ttyUSB4`). Multiple binaries under `src/bin/` (`connect_and_send_mqtt`, `file_handling`) — select with `cargo run --bin <name> <device>`.
- `examples/esp32c3_connect_and_send_mqtt` — real firmware for an ESP32-C3. Uses a pinned nightly toolchain (`rust-toolchain.toml`), target `riscv32imc-esp-espidf`, and flashes via `espflash` (`cargo run` runs the espflash runner). Requires the ESP-IDF toolchain.

Both examples read runtime config (APN, MQTT broker, SSL) from a `cfg.toml` (copy from `cfg.toml.example`) via the `toml_cfg` crate at build time.

## Architecture

Two layers:

### 1. AT-command protocol layer — `src/quectel_atat/`
Pure declarative mapping of Quectel AT commands to Rust types via `atat`'s derive macros. No I/O logic here.
- `mod.rs` — command structs, each `#[derive(AtatCmd)]` + `#[at_cmd("...", ResponseType, timeout_ms = N)]`. Each field is an `#[at_arg(position = N)]`. This is where you add or edit an AT command.
- `responses.rs` — `#[derive(AtatResp)]` structs the modem replies with.
- `types.rs` — enums/newtypes used as command args (bands, RATs, SSL config, etc.) plus `ModemConfiguration`, a builder holding band masks and RAT search order (`.set_bands()`, `.set_rat_order()`). Unit tests here verify band-mask / RAT encoding.
- `urc.rs` — `Urc` enum of Unsolicited Result Codes (async modem notifications like power-down, MQTT state changes) dispatched through `atat`'s `UrcChannel`.

### 2. Driver / state-machine layer — `src/cellular.rs` (the bulk of the code)
`QuectelBG9X<W, OutputPinGeneric>` is the public driver, generic over a `Write` (serial TX) and an `embedded_hal` `OutputPin` (the PWR_KEY GPIO). `Write`, `Client` and `AtatClient` are `#[cfg]`-aliased at the top of the file: `embedded_io::Write` + `atat::blocking::*` under `std`, `embedded_io_async::Write` + `atat::asynch::*` under `embassy` (GPIO stays sync `embedded_hal::digital::OutputPin` in both). It owns:
- the AT `Client` used to `send()` commands and get typed responses,
- a `&'static UrcChannel` it `.subscribe()`s to when it needs to wait for an async event (power-down, network registration, MQTT connect),
- detected `ModemRevision` and `ModemMode` state.

High-level methods orchestrate multi-command flows and translate `atat` errors into the crate's `ModemError` (defined in `src/lib.rs`): `power_on`/`power_off`, `test_sim`, `set_modem_configuration`, `network_attach`, `context_activate`, `mqtt_connect`/`mqtt_publish`/`mqtt_disconnect`, GNSS control, and internal-flash file operations (upload/read/write/delete — used for SSL CA certs).

Key behavioral points to preserve when editing:
- **Write the driver ONCE, as `async`.** The whole `impl` block carries `#[maybe_async_cfg::maybe(keep_self, sync(feature = "std"), async(feature = "embassy"))]`. Write every method `async fn` with `.await` on client sends, internal method calls, and delays; the macro strips `async`/`.await` to generate the blocking version under `std`. `keep_self` preserves the public type name `QuectelBG9X` (without it the macro renames it to `QuectelBG9XSync`/`QuectelBG9XAsync`). Never hand-edit a "blocking copy" — there isn't one.
- **All timing goes through the private `compat` module.** `compat::delay_ms`/`delay_secs` (blocking `std::thread::sleep` vs `embassy_time::Timer`) and `compat::Instant`/`compat::elapsed_ms` (a `u64`-millis monotonic clock). Do NOT reintroduce `std::thread`, `std::time`, or `SystemTime` in this file — they break the `no_std`/embassy build. Timeout loops are `while compat::elapsed_ms(now) < N { compat::delay_ms(..).await; ... }`; public durations use `core::time::Duration`.
- **`no_std` discipline.** No `std::`, no `format!`/`Vec`/`String` from `alloc` — use `core::` paths, `atat::heapless::{String, Vec}`, and `core::fmt::Write` (`write!`) into a heapless buffer. Timestamp parsing uses the compile-time `time::macros::format_description!` (no allocator).
- **URC waits are still poll-based in both runtimes.** `subscriber.try_next_message_pure()` is non-async in `atat`; the loops poll it with a `compat::delay_ms(..).await` between tries. This is intentional (identical behavior in both backends), not an oversight.
- **Firmware-revision branching.** `update_module_revision()` string-matches the `AT+QGMR` firmware version into `ModemRevision` (R200/R018/R014/R012/Unknown). Some commands behave differently per revision (e.g. `mqtt_disconnect` special-cases `R200`). When adding modem-behavior workarounds, gate them on `self.rev`.
- **Construction runs I/O.** `QuectelBG9X::new()` immediately powers on the modem and queries revision + IMEI, returning `Err(ModemError::NotResponding)` if the modem is silent. Under `embassy` it is `async fn new(...).await`.

### atat runtime wiring (see examples for the canonical setup)
The caller, not the driver, owns the `atat` plumbing: an `Ingress` + `ResponseSlot` + `UrcChannel` (all `static`/`StaticCell` because they must be `'static`), a reader that feeds the serial RX into `ingress.write_buf()` and calls `ingress.try_advance()` (a background thread under `std`; an Embassy task calling `ingress.read_from(...).await` under `embassy`), and a `Client` built from the serial TX. The `UrcChannel` is passed by reference into the driver. Buffer sizes are the public consts `INGRESS_BUF_SIZE`, `URC_CAPACITY`, `URC_SUBSCRIBERS` in `src/cellular.rs`. The existing examples all use the `std` (blocking) backend; there is not yet an Embassy example.
