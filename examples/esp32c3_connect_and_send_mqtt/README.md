# Example that sends a MQTT message for the Dark Sky board

## Running
```
cp cfg.toml.example cfg.toml
# Edit cfg.toml
cargo run --release --bin connect_and_send_mqtt # or file_handling or other example at src/bin
```