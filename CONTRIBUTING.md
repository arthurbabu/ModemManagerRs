# Contributing

## Adding support for a new modem chip

This crate targets one Quectel chip per build, selected at compile time by a Cargo
feature (`bg95`, `bg96`, `eg916u`, ...). Everything that differs between chips —
which AT commands to send for band/RAT configuration, how to parse the
network-attach response, firmware-revision quirks — is isolated behind a single
trait, `ChipProfile` (`src/chip/mod.rs`), implemented once per chip in its own file
under `src/chip/`. The rest of the driver (`src/cellular/`) never branches on chip
feature flags directly; it calls through `crate::chip::ActiveChip`, a type alias
that resolves to the active chip's `ChipProfile` impl.

This is deliberately the `no_std`/static-dispatch analogue of how [Linux
ModemManager](https://gitlab.freedesktop.org/mobile-broadband/ModemManager)
structures vendor support: MM has a generic `MMBroadbandModem` core plus a
per-vendor plugin that supplies the vendor-specific AT-command behavior, resolved
at runtime via D-Bus/GObject interface probing across a dynamically discovered set
of devices. This crate can't do that — no allocator, no dynamic dispatch, and a
given firmware image only ever talks to one chip — so the same idea is expressed
as a compile-time trait instead: "add a chip" means "add one file implementing
`ChipProfile`", not "touch the driver's control flow".

If you're adding support for a new chip, the EG916U profile
(`src/chip/eg916u.rs`) is a good reference: unlike BG95/BG96 (which share almost
all logic and differ only in band-mask constants, factored into
`src/chip/classic.rs`), EG916U genuinely uses different AT commands, so its
`ChipProfile` impl is self-contained and shows the full shape a from-scratch chip
addition takes.

### 1. Add the Cargo feature

In `Cargo.toml`, add an empty feature next to the existing chips:

```toml
[features]
bg95 = []
bg96 = []
eg916u = []
newchip = []
```

Then add it to the mutual-exclusivity `compile_error!` checks in `src/lib.rs` (two
places: the "no chip selected" check and the "more than one chip selected"
check) — copy the pattern for the existing three chips and extend it to include
`newchip`.

### 2. Implement `ChipProfile`

Create `src/chip/newchip.rs` with a zero-sized marker struct and an impl of the
trait from `src/chip/mod.rs`:

```rust
pub(crate) struct NewChip;

impl ChipProfile for NewChip {
    const NAME: &'static str = "NEWCHIP";
    const EMTC_ALL_BANDS_MASK: u128 = /* ... */;
    const NB_ALL_BANDS_MASK: u128 = /* ... */;

    async fn configure_modem<W: Write>(
        client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
        config: &ModemConfiguration,
    ) -> Result<(), ModemError> { /* ... */ }

    async fn poll_attach_status<W: Write>(
        client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
    ) -> Result<Option<ModemMode>, ModemError> { /* ... */ }

    fn classify_revision(version: &str) -> ModemRevision { /* ... */ }

    // Optional: only override if your chip needs it (default is `false`).
    fn needs_explicit_mqtt_close(rev: ModemRevision) -> bool { false }
}
```

Work through each member:

- **`NAME`** — a human-readable label, logged once in `QuectelBG9X::new()`. Purely
  cosmetic.

- **`EMTC_ALL_BANDS_MASK` / `NB_ALL_BANDS_MASK`** — the "all bands" bitmask
  `quectel_atat::types::Band::all_bands_mask()` returns for `EmtcBands::Any` /
  `NbIotBands::Any` on this chip (see `src/quectel_atat/types.rs`). If your chip
  doesn't support a given RAT at all (EG916U has no NB-IoT radio), set that mask
  to `0` — see `Eg916u::NB_ALL_BANDS_MASK` for the precedent, with a comment
  explaining why. **Do not guess these values** — pull them from the chip's AT
  command manual. If you can't yet verify them, follow EG916U's convention:
  ship a best-effort value and mark it clearly with a `// TODO(newchip): ...`
  comment plus a matching note in `README.md`'s chip section, so it's visible
  that the value is provisional rather than looking authoritative.

- **`configure_modem`** — send whatever AT commands configure bands / RAT search
  order / service domain. If your chip uses the same `AT+QCFG="band"/
  "nwscanseq"/"nwscanmode"/"servicedomain"/"iotopmode"` command family as
  BG95/BG96, you likely don't need a new implementation at all — just add your
  chip to `#[cfg(any(feature = "bg95", feature = "bg96"))]` on `mod classic;` in
  `src/chip/mod.rs` and delegate like `Bg95`/`Bg96` do:
  ```rust
  async fn configure_modem<W: Write>(
      client: &mut Client<'static, W, INGRESS_BUF_SIZE>,
      config: &ModemConfiguration,
  ) -> Result<(), ModemError> {
      super::classic::configure_bg9x_modem(client, config).await
  }
  ```
  If your chip's command set is genuinely different (as EG916U's is — it has no
  per-RAT band-mask command at all today), write it from scratch; see
  `Eg916u::configure_modem` for the shape. This is where you'd add real
  band/RAT AT commands for EG916U too, once the datasheet confirms them (see
  the `TODO(eg916u)` markers in `src/chip/eg916u.rs` and
  `src/quectel_atat/types.rs`).

- **`poll_attach_status`** — one polling attempt at determining whether the modem
  has attached to the network and which radio technology it's using. Return
  `Ok(None)` to mean "not yet, keep polling" and `Ok(Some(mode))` once attached.
  `Eg916u::poll_attach_status` (using `AT+COPS?`/`GetCopsInfo`, integer AcT codes)
  and `classic::poll_bg9x_attach` (using `AT+QNWINFO`/`GetNetworkInfo`, string AcT)
  are two different real answers to the same shape — pick whichever AT command
  your chip actually supports and normalize its response into `ModemMode`.

- **`classify_revision`** — match the `AT+QGMR` firmware-version string into a
  `ModemRevision` variant. If your chip needs its own revision-tracking (e.g. to
  gate a firmware-specific workaround later), add a variant to the
  `ModemRevision` enum in `src/chip/mod.rs`; otherwise matching everything to
  `ModemRevision::Unknown` is fine until you have a reason to distinguish
  revisions.

- **`needs_explicit_mqtt_close`** — only override this if your chip has an
  `AT+QMTDISC`-reliability quirk like BG95's R200 firmware (see
  `Bg95::needs_explicit_mqtt_close`). Most chips should leave the default
  (`false`).

### 3. Register the module

In `src/chip/mod.rs`, add:

```rust
#[cfg(feature = "newchip")]
mod newchip;

#[cfg(feature = "newchip")]
pub(crate) type ActiveChip = newchip::NewChip;
```

That's the entire wiring — nothing in `src/cellular/` needs to change. Every
capability module (`power.rs`, `config.rs`, `registration.rs`, `mqtt.rs`, ...)
calls `ActiveChip::whatever(...)` and picks up your new chip automatically.

### 4. New AT commands, if needed

If your chip needs an AT command that doesn't exist yet, add it to
`src/quectel_atat/mod.rs` as a `#[derive(AtatCmd)]` struct (see any existing
command for the pattern — `#[at_cmd("...", ResponseType, timeout_ms = N)]`, one
`#[at_arg(position = N)]` field per argument) and its response shape to
`src/quectel_atat/responses.rs` as `#[derive(AtatResp)]`. These structs are
chip-agnostic by convention even when only one chip uses them today — don't
`#[cfg]`-gate the command definition itself, only the call site in your chip's
`ChipProfile` impl. If the modem can also send it unprompted as a notification,
add a variant to the `Urc` enum in `src/quectel_atat/urc.rs` instead.

### 5. New band values, if needed

If your chip supports bands that don't exist yet in `EmtcBands`/`NbIotBands`
(`src/quectel_atat/types.rs`), add cfg-gated variants following the existing
`#[cfg(feature = "bg95")]`/`#[cfg(feature = "bg96")]` pattern:

```rust
#[cfg(feature = "newchip")]
Band99 = 99,
```

This stays a per-variant `#[cfg]` rather than going through `ChipProfile`
deliberately: which numeric band values are even constructible is a compile-time
enum-exhaustiveness question (so passing an invalid band for your chip is a
compile error, not a runtime one), and a trait can't add or remove enum variants.
`ChipProfile::EMTC_ALL_BANDS_MASK`/`NB_ALL_BANDS_MASK` (step 2) is the trait-level
piece — it's the *bitmask*, not the *variant set*.

### 6. Test it

```bash
cargo build --no-default-features --features "newchip std"
cargo build --no-default-features --features "newchip embassy log" --target riscv32imac-unknown-none-elf
cargo clippy --no-default-features --features "newchip std"
cargo test
```

The `embassy` build against a bare-metal target (no host `std` available) is the
important one — it's how an accidental `std::`/`alloc` dependency leaking into
`src/chip/newchip.rs` gets caught. (`log` or `defmt` must be enabled explicitly
alongside `embassy`; neither is pulled in by `embassy` alone.)

### 7. Update the docs

- Add your chip to `README.md`'s chip-selection list, with the same
  "provisional, not datasheet-verified" caveat as EG916U's if you shipped
  best-effort band masks.
- If you tested against specific firmware builds, add them to the "Tested
  with:" / "To be tested with:" comment block at the top of
  `src/cellular/mod.rs`.
