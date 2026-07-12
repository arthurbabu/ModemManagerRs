# AT Driver for the Quectel BG9X family of modems

This respository contains a driver for the [Quectel](https://www.quectel.com/) BG95/BG96 modems. The driver is built on top of the [atat crate](https://docs.rs/atat/latest/atat/).

The main features are:

* Send a PWR_KEY which turns on/off the module by toggling a GPIO
* Connect to an MQTT broker and publish data (with optional SSL/TLS support)
* Uses LTE-M with fallback to 2G
* SSL/TLS configuration for secure MQTT connections

The crate is available on [crates.io](https://crates.io/crates/quectel-bg9x-eh-driver).

## Crate configuration

The crate has two independent axes of Cargo features.

**Chip selection** (mutually exclusive band lists — the only difference between them):

* `bg95` *(default)*
* `bg96`

**Runtime selection** (mutually exclusive — enable exactly one):

* `std` *(default)* — **blocking** driver for hosts. Uses `std::thread`/`std::time`
  for delays and timeouts and the `atat` blocking client. This is what the Linux
  example and `cargo test` use.
* `embassy` — **async** (`no_std`) driver for embedded systems. Uses
  [`embassy-time`](https://docs.rs/embassy-time) for delays/timeouts and the
  `atat` async client (`embedded-io-async`). Every driver method is `async` and
  must be `.await`ed.

The two runtimes are generated from a single source: the driver logic is written
once as `async` and the [`maybe-async-cfg`](https://docs.rs/maybe-async-cfg)
macro produces the blocking version under `std` and the async version under
`embassy`. The public method names are identical across both runtimes; only the
`async`/`.await` differs.

```toml
# Blocking, on a host (default):
quectel-bg9x-eh-driver = "0.4"

# Async, on an embedded target with Embassy:
quectel-bg9x-eh-driver = { version = "0.4", default-features = false, features = ["bg95", "embassy"] }
```

Building with both runtimes, or with neither, is a compile error.

## TODO
- [x] Remove STD dependencies (via the `embassy` feature; the driver core is now `no_std`)
- [ ] Make modem user configurable (AT+QCFG commands)
- [ ] Add an Embassy (async) example

## Examples

* `linux-simple`: example of interacting with the module connected through a serial converter to a Linux computer. Requires the converter to be `/dev/ttyUSB4` and the module to be always on (the PWR_KEY is faked).
    ```
    cd examples/linux_simple
    cargo run
    ```
* `esp32c3_connect_and_send_mqtt`: run the example in a board such us [Dark Sky Meter Hardware](https://gitlab.com/scrobotics/optical-makerspace/dark-sky-meter-hw)
    ```
    cd examples/esp32c3_connect_and_send_mqtt
    cargo run
    ```

For a more complete example visit Tested with the [Dark Sky Meter Firmware](https://gitlab.com/scrobotics/optical-makerspace/dark-sky-meter-fw).

## License
[![license](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](https://gitlab.com/scrobotics/embedded-rs/adxl234-rs/-/blob/master/LICENSE)

This tool is released under the MIT license, hence allowing commercial use of the library. Please refer to the [LICENSE](https://gitlab.com/scrobotics/embedded-rs/adxl234-rs/-/blob/master/LICENSE) file.

## Contributing

The project is Open Source but it's been heavily design to fit our needs, in particular for the [Dark Sky Meter Firmware](https://gitlab.com/scrobotics/optical-makerspace/dark-sky-meter-fw).
