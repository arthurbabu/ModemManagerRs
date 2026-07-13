# AT Driver for the Quectel BG9X family of modems

This respository contains a driver for the [Quectel](https://www.quectel.com/) BG95/BG96 modems. The driver is built on top of the [atat crate](https://docs.rs/atat/latest/atat/).

The main features are:

* Send a PWR_KEY which turns on/off the module by toggling a GPIO
* Connect to an MQTT broker and publish data (with optional SSL/TLS support)
* Uses LTE-M with fallback to 2G
* SSL/TLS configuration for secure MQTT connections
* TCP+TLS sockets (including **mutual TLS**) exposed via the
  [`embedded-nal-async`](https://docs.rs/embedded-nal-async) traits (async /
  `embassy` only)

The crate is available on [crates.io](https://crates.io/crates/modem-manager-rs).

## Crate configuration

The crate has two independent axes of Cargo features.

**Chip selection** (mutually exclusive — enable exactly one):

* `bg95` *(default)* — Cat-M / NB-IoT / GSM
* `bg96` — Cat-M / NB-IoT / GSM
* `eg916u` — LTE Cat 1bis + GSM fallback

> **Note:** the EG916U band lists and RAT configuration are provisional and
> marked with `TODO(eg916u)` in `src/quectel_atat/types.rs` — confirm them
> against the EG916U datasheet before relying on `set_modem_configuration` for
> that chip. The TCP/TLS socket support below is chip-independent.

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
modem-manager-rs = "0.4"

# Async, on an embedded target with Embassy:
modem-manager-rs = { version = "0.4", default-features = false, features = ["bg95", "embassy"] }
```

Building with both runtimes, or with neither, is a compile error.

## TCP + mutual TLS sockets

With the `embassy` feature the driver can open TCP+TLS sockets using the
**modem's own** TLS engine (`AT+QSSLOPEN`/`QSSLSEND`/`QSSLRECV`/`QSSLCLOSE`) and
expose them through the [`embedded-nal-async`](https://docs.rs/embedded-nal-async)
`TcpConnect` trait. Because TLS is terminated on the modem, the CA certificate
and — for mutual TLS — the client certificate and private key live in the
modem's UFS flash (upload them with `upload_file_to_internal_flash`), not on the
MCU.

```rust,ignore
// 1. Upload credentials to the modem flash (once).
modem.upload_file_to_internal_flash("ca.pem", ca_bytes).await?;
modem.upload_file_to_internal_flash("client.pem", client_cert_bytes).await?;
modem.upload_file_to_internal_flash("client.key", client_key_bytes).await?;

// 2. Configure an SSL context for mutual TLS.
let mut ssl = SslConfiguration::new();
ssl.set_context_id(2).unwrap();
ssl.set_ca_cert("ca.pem").unwrap();
ssl.set_client_cert("client.pem").unwrap();
ssl.set_client_key("client.key").unwrap();
ssl.set_auth_mode(SslAuthenticationMode::Mutual);
modem.configure_ssl_context(ssl).await?;

// 3a. Open a socket directly (hostname preserved for SNI):
modem.ssl_socket_open(0, 2, "example.com", 8883).await?;
modem.ssl_socket_send(0, b"hello").await?;
let mut buf = [0u8; 256];
let n = modem.ssl_socket_recv(0, &mut buf).await?;
modem.ssl_socket_close(0).await?;

// 3b. …or via embedded-nal-async (share the modem behind a Mutex):
//     see `src/tcp.rs` (QuectelTcpClient / TlsSocket).
```

`QuectelTcpClient` (in `src/tcp.rs`) implements `embedded_nal_async::TcpConnect`;
the returned `TlsSocket` implements `embedded_io_async::Read`/`Write`. Note that
`TcpConnect::connect` only carries an IP address, so use `ssl_socket_open`
directly when you need a hostname for SNI/hostname verification.

## TODO
- [x] Remove STD dependencies (via the `embassy` feature; the driver core is now `no_std`)
- [x] TCP+TLS sockets with mutual TLS via `embedded-nal-async` (`embassy` feature)
- [ ] Confirm EG916U band lists / RAT config against the datasheet
- [ ] Wire `+QSSLURC: "closed"` into `TlsSocket::read` for EOF detection
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
