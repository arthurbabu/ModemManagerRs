fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Check if the `cfg.toml` file exists and has been filled out.
    if !std::path::Path::new("cfg.toml").exists() {
        return Err("Please, create a `cfg.toml` file with your default configuration.".into());
    }

    embuild::espidf::sysenv::output();

    Ok(())
}
