// Serial passthrough for AT commands
// This example forwards data between USB serial (UART0) and the modem serial (UART1)
// allowing you to send AT commands directly to the modem and see responses

use std::{thread, time};

use esp_idf_sys as _;

use esp_idf_hal::gpio;
use esp_idf_hal::prelude::*;
use esp_idf_hal::uart;
use esp_idf_svc::log::EspLogger;

use log::*;

fn main() {
    esp_idf_sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();

    info!("Starting Serial Passthrough");

    // Modem power control
    let mut modem_pwr_en = gpio::PinDriver::output(peripherals.pins.gpio3).unwrap();
    let modem_pwr_key = gpio::PinDriver::output(peripherals.pins.gpio2).unwrap();

    // Modem UART (UART1)
    let modem_tx = peripherals.pins.gpio0;
    let modem_rx = peripherals.pins.gpio1;
    let modem_uart_conf = uart::UartConfig::new()
        .baudrate(115_200.Hz())
        .source_clock(uart::config::SourceClock::RTC);
    let modem_uart = uart::UartDriver::new(
        peripherals.uart1,
        modem_tx,
        modem_rx,
        Option::<gpio::AnyIOPin>::None,
        Option::<gpio::AnyIOPin>::None,
        &modem_uart_conf,
    )
    .unwrap();

    // USB Serial (UART0) - this is the default serial console
    let usb_tx = peripherals.pins.gpio21;
    let usb_rx = peripherals.pins.gpio20;
    let usb_uart_conf = uart::UartConfig::new()
        .baudrate(115_200.Hz())
        .source_clock(uart::config::SourceClock::RTC);
    let usb_uart = uart::UartDriver::new(
        peripherals.uart0,
        usb_tx,
        usb_rx,
        Option::<gpio::AnyIOPin>::None,
        Option::<gpio::AnyIOPin>::None,
        &usb_uart_conf,
    )
    .unwrap();

    // Power up the modem
    info!("Powering up modem...");
    modem_pwr_en.set_low().unwrap();
    thread::sleep(time::Duration::from_millis(500));
    modem_pwr_en.set_high().unwrap();
    thread::sleep(time::Duration::from_millis(500));

    // Toggle power key to turn on modem
    info!("Toggling power key...");
    let mut modem_pwr_key_driver = modem_pwr_key;
    modem_pwr_key_driver.set_high().unwrap();
    thread::sleep(time::Duration::from_millis(500));
    modem_pwr_key_driver.set_low().unwrap();

    info!("Serial passthrough active - you can now send AT commands");
    info!("Commands from USB will be forwarded to modem");
    info!("Responses from modem will be forwarded to USB");
    info!("Press Esc for a clean exit");

    let mut usb_buf = [0u8; 256];
    let mut modem_buf = [0u8; 256];

    loop {
        // Forward USB -> Modem
        match usb_uart.read(&mut usb_buf, 0) {
            Ok(len) if len > 0 => {
                // info!("USB->Modem: {} bytes", len);

                // Check for Esc key (ASCII 27) to exit
                if usb_buf[..len].contains(&27) {
                    info!("Esc key detected, exiting serial passthrough...");
                    break;
                }

                // Write byte by byte with throttle
                for byte in &usb_buf[..len] {
                    if let Err(e) = modem_uart.write(&[*byte]) {
                        error!("Error writing to modem: {:?}", e);
                    }

                    // If we just sent CR (13), also send LF (10)
                    if *byte == 13 {
                        if let Err(e) = modem_uart.write(&[10]) {
                            error!("Error writing LF to modem: {:?}", e);
                        }
                        // Echo LF to USB as well
                        if let Err(e) = usb_uart.write(&[10]) {
                            error!("Error echoing LF to USB: {:?}", e);
                        }
                    }

                    thread::sleep(time::Duration::from_millis(1));
                }
            }
            _ => {}
        }

        // Forward Modem -> USB
        match modem_uart.read(&mut modem_buf, 0) {
            Ok(len) if len > 0 => {
                // info!("Modem->USB: {} bytes", len);
                if let Err(e) = usb_uart.write(&modem_buf[..len]) {
                    error!("Error writing to USB: {:?}", e);
                }
            }
            _ => {}
        }

        thread::sleep(time::Duration::from_millis(10));
    }
    info!("Serial passthrough terminated. Powering down modem...");
    modem_pwr_en.set_low().unwrap();

    loop {
        thread::sleep(time::Duration::from_millis(1000));
    }
}
