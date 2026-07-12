use atat::atat_derive::AtatEnum;
use atat::heapless::String;
use atat::heapless_bytes::Bytes;

/// Echo on
#[derive(Debug, Clone, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum EchoOn {
    ///  Unit does not echo the characters in command mode
    Off = 0,
    /// Unit echoes the characters in command mode. (default)
    On = 1,
}

/// Functionality level of UE
#[derive(Debug, Clone, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum FunctionalityLevelOfUE {
    /// Minimum functionality
    Minimum = 0,
    /// Full functionality (default)
    Full = 1,
    /// Disable modem both transmit and receive RF circuits
    DisableRF = 4,
}

/// Configure RATs Searching Sequence effect
#[derive(Debug, Clone, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum ConfigurationEffect {
    /// After reboot
    AfterReboot = 0,
    /// immediately
    Immediately = 1,
}

/// Echo on
#[derive(Debug, Clone, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum PowerDownMode {
    ///  Immediately power down
    Immediate = 0,
    /// Normal power down (default)
    Normal = 1,
}

#[derive(Debug, Clone, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum GnssState {
    Off = 0,
    On = 1,
}

#[derive(Debug, Clone, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum GnssOperatingMode {
    StandAlone = 1,
    MsBased = 2,
    MsAssisted = 3,
    SpeedOptimal = 4,
}

#[derive(Debug, Clone, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum GnssConstellation {
    GLONASS = 1,
    BeiDou = 2,
    Galileo = 3,
    QZSS = 4,
    Variable = 5,
}

/// Authentication method for PDP context
#[derive(Debug, Clone, Copy, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum AuthenticationMethod {
    /// No authentication
    None = 0,
    /// PAP (Password Authentication Protocol)
    PAP = 1,
    /// CHAP (Challenge Handshake Authentication Protocol)
    CHAP = 2,
    /// PAP or CHAP
    PAPorCHAP = 3,
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum RadioAccessTechnology {
    GSM = 1,
    EMTC = 2,
    NbIoT = 3,
    Default = 0,
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum GsmBands {
    Gsm900MHz = 1,
    Gsm1800MHz = 2,
    Gsm850MHz = 3,
    Gsm1900MHz = 4,
    NoChange = 0,
    Any = u8::MAX,
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum EmtcBands {
    Band1 = 1,
    Band2 = 2,
    Band3 = 3,
    Band4 = 4,
    Band5 = 5,
    Band8 = 8,
    Band12 = 12,
    Band13 = 13,
    Band18 = 18,
    Band19 = 19,
    Band20 = 20,
    Band25 = 25,
    Band26 = 26,
    #[cfg(feature = "bg95")]
    Band27 = 27,
    Band28 = 28,
    #[cfg(feature = "bg95")]
    Band31 = 31,
    #[cfg(feature = "bg95")]
    Band66 = 66,
    #[cfg(feature = "bg95")]
    Band72 = 72,
    #[cfg(feature = "bg95")]
    Band73 = 73,
    #[cfg(feature = "bg95")]
    Band85 = 85,
    NoChange = 0,
    Any = u8::MAX,
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum NbIotBands {
    Band1 = 1,
    Band2 = 2,
    Band3 = 3,
    Band4 = 4,
    Band5 = 5,
    Band8 = 8,
    Band12 = 12,
    Band13 = 13,
    Band18 = 18,
    Band19 = 19,
    Band20 = 20,
    Band25 = 25,
    #[cfg(feature = "bg96")]
    Band26 = 26,
    Band28 = 28,
    #[cfg(feature = "bg95")]
    Band31 = 31,
    #[cfg(feature = "bg95")]
    Band66 = 66,
    #[cfg(feature = "bg95")]
    Band71 = 71,
    #[cfg(feature = "bg95")]
    Band72 = 72,
    #[cfg(feature = "bg95")]
    Band73 = 73,
    #[cfg(feature = "bg95")]
    Band85 = 85,

    NoChange = 0,
    Any = u8::MAX,
}

/// A trait for radio access technology (RAT) band identifiers.
///
/// This trait abstracts over different RAT band enums (e.g. `GsmBands`,
/// `EmtcBands`, `NbIotBands`) so they can be handled generically when
/// building or parsing band masks.
///
/// # Required methods
///
/// ## `all_bands_mask`
///
/// ```ignore
/// fn all_bands_mask(self) -> u128;
/// ```
///
/// Returns a bitmask with **all valid bands** for the enum type set.
///
/// This is typically used when the "Any" sentinel is selected to indicate
/// support for all bands. The returned mask should include *only* bits that
/// correspond to valid bands of that enum.
///
/// For example, for an LTE band enum with bands 1, 3, and 28 defined,
/// `all_bands_mask()` would return:
///
/// ```text
/// (1 << (1 - 1)) | (1 << (3 - 1)) | (1 << (28 - 1))
/// ```
///
/// ## `as_u8`
///
/// ```ignore
/// fn as_u8(self) -> u8;
/// ```
///
/// Returns the numeric representation of the band.  
/// This is used to calculate the bit index when constructing a band mask.
///
/// The return value has special meanings for sentinels:
/// - `0` → represents "NoChange" (do not alter the current mask).
/// - `u8::MAX` → represents "Any" (all bands are selected).
///
/// For all other variants, the returned value should be the band number
/// (e.g., `1` for Band1, `20` for Band20).
///
pub trait Band {
    fn all_bands_mask() -> u128;
    fn as_u8(self) -> u8;
    fn no_change() -> Self;
    fn any() -> Self;
}

impl Band for GsmBands {
    fn all_bands_mask() -> u128 {
        return 0xF;
    }
    fn as_u8(self) -> u8 {
        self as u8
    }
    fn no_change() -> Self {
        return GsmBands::NoChange;
    }
    fn any() -> Self {
        return GsmBands::Any;
    }
}
impl Band for EmtcBands {
    fn all_bands_mask() -> u128 {
        #[cfg(feature = "bg96")]
        return 0xB0E189F;
        #[cfg(feature = "bg95")]
        return 0x100182000000004F0E189F;
    }
    fn as_u8(self) -> u8 {
        self as u8
    }
    fn no_change() -> Self {
        return EmtcBands::NoChange;
    }
    fn any() -> Self {
        return EmtcBands::Any;
    }
}
impl Band for NbIotBands {
    fn all_bands_mask() -> u128 {
        #[cfg(feature = "bg96")]
        return 0xB0E189F;
        #[cfg(feature = "bg95")]
        return 0x1001C200000000490E189F;
    }
    fn as_u8(self) -> u8 {
        self as u8
    }
    fn no_change() -> Self {
        return NbIotBands::NoChange;
    }
    fn any() -> Self {
        return NbIotBands::Any;
    }
}

pub struct ModemConfiguration {
    gsm_bands: u8,
    emtc_bands: u128,
    nb_bands: u128,
    rat_order: String<8>,
}

impl ModemConfiguration {
    pub fn new() -> Self {
        Self {
            gsm_bands: 0,
            emtc_bands: 0,
            nb_bands: 0,
            rat_order: String::<8>::try_from("020301").unwrap(),
        }
    }

    pub fn set_bands<T>(&mut self, rat: RadioAccessTechnology, bands: &[T]) -> Result<(), ()>
    where
        T: Band + Copy,
    {
        if bands.is_empty() {
            return Err(());
        }
        let mut mask: u128 = 0;

        if bands[0].as_u8() == u8::MAX {
            mask = T::all_bands_mask();
        } else if bands.len() == 1 && bands[0].as_u8() == 0 {
            mask = 0;
        } else {
            for &band in bands {
                mask |= 1u128 << (band.as_u8() - 1)
            }
            if (!T::all_bands_mask() & mask) != 0 {
                return Err(());
            }
        }

        match rat {
            RadioAccessTechnology::GSM => self.gsm_bands = mask as u8,
            RadioAccessTechnology::EMTC => self.emtc_bands = mask,
            RadioAccessTechnology::NbIoT => self.nb_bands = mask,
            RadioAccessTechnology::Default => return Err(()),
        }

        Ok(())
    }

    pub fn set_rat_order(&mut self, rat: &[RadioAccessTechnology]) -> Result<(), ()> {
        if rat.is_empty() || rat.len() > 3 {
            return Err(());
        }

        let mut rat_string = String::<8>::new();
        let mut seen = [false; 4];
        for &r in rat {
            if seen.get(r as usize).copied().unwrap_or(true) {
                return Err(());
            }
            seen[r as usize] = true;

            let part: &str = match r {
                RadioAccessTechnology::GSM => "01",
                RadioAccessTechnology::EMTC => "02",
                RadioAccessTechnology::NbIoT => "03",
                RadioAccessTechnology::Default => "00",
            };
            rat_string.push_str(part).unwrap();
        }

        self.rat_order = rat_string;
        Ok(())
    }

    pub fn get_band_string(&self, rat: RadioAccessTechnology) -> Result<String<24>, ()> {
        let mut s: String<24> = String::new();
        let value: u128 = match rat {
            RadioAccessTechnology::GSM => self.gsm_bands as u128,
            RadioAccessTechnology::EMTC => self.emtc_bands,
            RadioAccessTechnology::NbIoT => self.nb_bands,
            _ => return Err(()),
        };

        if value == 0 {
            s.push('0').map_err(|_| ())?;
            return Ok(s);
        }

        let mut tmp = value;
        let mut digits: usize = 0;
        while tmp != 0 {
            digits += 1;
            tmp >>= 4;
        }

        if digits > 24 {
            return Err(());
        }

        let mut buf = [0u8; 32];
        let mut i = digits;
        let mut v = value;
        while v != 0 {
            let nibble = (v & 0xf) as u8;
            v >>= 4;
            let byte = if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + (nibble - 10)
            };
            i -= 1;
            buf[i] = byte;
        }

        for &b in &buf[..digits] {
            s.push(b as char).map_err(|_| ())?;
        }

        Ok(s)
    }

    pub fn get_rat_order(&self) -> String<8> {
        self.rat_order.clone()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum MqttVersion {
    /// MQTT Version 3.1 (default)
    V3_1 = 3,
    /// MQTT Version 3.1.1
    V3_1_1 = 4,
}

#[derive(Copy, Clone, Debug, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum MqttSslEnable {
    /// Use normal TCP connection for MQTT (default)
    False = 0,
    /// Use SSL TCP secure connection for MQTT
    True = 1,
}

#[derive(Copy, Clone, Debug, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum SslVersion {
    /// SSL 3.0
    Ssl3_0 = 0,
    /// TLS 1.0
    Tls1_0 = 1,
    /// TLS 1.1
    Tls1_1 = 2,
    /// TLS 1.2
    Tls1_2 = 3,
    All = 4,
}

#[derive(Copy, Clone, Debug, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum SslAuthenticationMode {
    /// No authentication
    None = 0,
    /// Manage server authentication only
    ServerOnly = 1,
    /// Manage mutual authentication (server and client) if requested by server
    Mutual = 2,
}

#[derive(Copy, Clone, Debug, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum SslIgnoreLocalTime {
    /// Care about validity check for certification
    Care = 0,
    /// Ignore validity check for certification
    Ignore = 1,
}

#[derive(Copy, Clone, Debug, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum SslSniEnable {
    /// Disable Server Name Indication
    Disable = 0,
    /// Enable Server Name Indication
    Enable = 1,
}

#[derive(Copy, Clone, Debug, PartialEq, AtatEnum)]
#[repr(u8)]
pub enum SslCheckHostEnable {
    /// Disable hostname validation
    Disable = 0,
    /// Enable hostname validation
    Enable = 1,
}

pub type SslCipherSuites = Bytes<6>;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SslCipherSuiteEnum {
    /// TLS_RSA_WITH_AES_256_CBC_SHA
    TlsRsaWithAes256CbcSha = 0x0035,
    /// TLS_RSA_WITH_AES_128_CBC_SHA
    TlsRsaWithAes128CbcSha = 0x002F,
    /// TLS_RSA_WITH_RC4_128_SHA
    TlsRsaWithRc4_128Sha = 0x0005,
    /// TLS_RSA_WITH_RC4_128_MD5
    TlsRsaWithRc4_128Md5 = 0x0004,
    /// TLS_RSA_WITH_3DES_EDE_CBC_SHA
    TlsRsaWith3desEdeCbcSha = 0x000A,
    /// TLS_RSA_WITH_AES_256_CBC_SHA256
    TlsRsaWithAes256CbcSha256 = 0x003D,
    /// TLS_ECDHE_RSA_WITH_RC4_128_SHA
    TlsEcdheRsaWithRc4_128Sha = 0xC011,
    /// TLS_ECDHE_RSA_WITH_3DES_EDE_CBC_SHA
    TlsEcdheRsaWith3desEdeCbcSha = 0xC012,
    /// TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA
    TlsEcdheRsaWithAes128CbcSha = 0xC013,
    /// TLS_ECDHE_RSA_WITH_AES_256_CBC_SHA
    TlsEcdheRsaWithAes256CbcSha = 0xC014,
    /// TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA256
    TlsEcdheRsaWithAes128CbcSha256 = 0xC027,
    /// TLS_ECDHE_RSA_WITH_AES_256_CBC_SHA384
    TlsEcdheRsaWithAes256CbcSha384 = 0xC028,
    /// TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
    TlsEcdheRsaWithAes128GcmSha256 = 0xC02F,
    /// Support all
    SupportAll = 0xFFFF,
}

impl SslCipherSuiteEnum {
    /// Convert the cipher suite to its hex byte representation
    pub fn to_bytes(&self) -> SslCipherSuites {
        use core::fmt::Write as _;
        let mut hex_string = atat::heapless::String::<8>::new();
        write!(hex_string, "0x{:04X}", *self as u16).ok();
        let mut bytes = SslCipherSuites::new();
        bytes.extend_from_slice(hex_string.as_bytes()).ok();
        bytes
    }
}

/// SSL/TLS Configuration for secure connections.
///
/// This struct provides a builder-like pattern for configuring SSL/TLS parameters
/// used in secure connections. Similar to `ModemConfiguration`, it allows setting
/// various SSL options before applying them to the modem.
///
/// # Example
///
/// ```ignore
/// let mut ssl_config = SslConfiguration::new();
/// ssl_config
///     .set_ca_cert("cacert.pem")
///     .set_ssl_version(SslVersion::Tls1_2)
///     .set_cipher_suite(SslCipherSuiteEnum::TlsRsaWithAes256CbcSha)
///     .set_auth_mode(SslAuthenticationMode::ServerOnly)
///     .set_sni(true)
///     .set_check_host(false)
///     .set_ignore_localtime(true);
/// ```
#[derive(Clone, Debug)]
pub struct SslConfiguration {
    context_id: u8,
    ca_cert_filename: String<80>,
    ssl_version: SslVersion,
    cipher_suite: Option<SslCipherSuites>,
    auth_mode: SslAuthenticationMode,
    sni_enable: SslSniEnable,
    checkhost_enable: SslCheckHostEnable,
    ignore_localtime: SslIgnoreLocalTime,
}

impl SslConfiguration {
    /// Create a new SSL configuration with default values.
    ///
    /// Defaults:
    /// - Context ID: 2
    /// - CA certificate: empty (must be set, can be empty string if not used)
    /// - SSL version: TLS 1.2
    /// - Cipher suite: All supported (0xFFFF)
    /// - Authentication: Server only
    /// - SNI: Enabled
    /// - Check hostname: Disabled
    /// - Ignore local time: Ignore (don't validate certificate dates)
    pub fn new() -> Self {
        Self {
            context_id: 2,
            ca_cert_filename: String::new(),
            ssl_version: SslVersion::Tls1_2,
            cipher_suite: None, // Will default to SupportAll in configure_ssl_context
            auth_mode: SslAuthenticationMode::ServerOnly,
            sni_enable: SslSniEnable::Enable,
            checkhost_enable: SslCheckHostEnable::Disable,
            ignore_localtime: SslIgnoreLocalTime::Ignore,
        }
    }

    /// Set the SSL context ID (0-5).
    ///
    /// # Arguments
    ///
    /// * `id` - SSL context ID (valid range: 0-5)
    ///
    /// # Returns
    ///
    /// * `Ok(&mut Self)` - For method chaining
    /// * `Err(())` - If context ID is out of valid range
    pub fn set_context_id(&mut self, id: u8) -> Result<&mut Self, ()> {
        if id > 5 {
            return Err(());
        }
        self.context_id = id;
        Ok(self)
    }

    /// Set the CA certificate filename.
    ///
    /// # Arguments
    ///
    /// * `filename` - Name of the CA certificate file stored in UFS (can be empty)
    ///
    /// # Returns
    ///
    /// * `Ok(&mut Self)` - For method chaining
    /// * `Err(())` - If filename is too long
    pub fn set_ca_cert(&mut self, filename: &str) -> Result<&mut Self, ()> {
        self.ca_cert_filename = String::try_from(filename).map_err(|_| ())?;
        Ok(self)
    }

    /// Set the SSL/TLS protocol version.
    pub fn set_ssl_version(&mut self, version: SslVersion) -> &mut Self {
        self.ssl_version = version;
        self
    }

    /// Set the cipher suite.
    ///
    /// # Arguments
    ///
    /// * `suite` - Specific cipher suite to use
    ///
    /// # Note
    ///
    /// If not set (None), the modem will use all supported cipher suites.
    pub fn set_cipher_suite(&mut self, suite: SslCipherSuiteEnum) -> &mut Self {
        self.cipher_suite = Some(suite.to_bytes());
        self
    }

    /// Set the cipher suite to support all available ciphers.
    pub fn set_cipher_suite_all(&mut self) -> &mut Self {
        self.cipher_suite = None; // None means use SupportAll
        self
    }

    /// Set the authentication mode.
    pub fn set_auth_mode(&mut self, mode: SslAuthenticationMode) -> &mut Self {
        self.auth_mode = mode;
        self
    }

    /// Enable or disable Server Name Indication (SNI).
    pub fn set_sni(&mut self, enable: bool) -> &mut Self {
        self.sni_enable = if enable {
            SslSniEnable::Enable
        } else {
            SslSniEnable::Disable
        };
        self
    }

    /// Enable or disable hostname validation.
    pub fn set_check_host(&mut self, enable: bool) -> &mut Self {
        self.checkhost_enable = if enable {
            SslCheckHostEnable::Enable
        } else {
            SslCheckHostEnable::Disable
        };
        self
    }

    /// Set whether to ignore certificate validity dates.
    ///
    /// # Arguments
    ///
    /// * `ignore` - If true, certificate expiration dates won't be validated
    pub fn set_ignore_localtime(&mut self, ignore: bool) -> &mut Self {
        self.ignore_localtime = if ignore {
            SslIgnoreLocalTime::Ignore
        } else {
            SslIgnoreLocalTime::Care
        };
        self
    }

    /// Get the context ID.
    pub fn get_context_id(&self) -> u8 {
        self.context_id
    }

    /// Get the CA certificate filename.
    pub fn get_ca_cert_filename(&self) -> &str {
        self.ca_cert_filename.as_str()
    }

    /// Get the SSL version.
    pub fn get_ssl_version(&self) -> SslVersion {
        self.ssl_version
    }

    /// Get the cipher suite.
    pub fn get_cipher_suite(&self) -> Option<SslCipherSuites> {
        self.cipher_suite.clone()
    }

    /// Get the authentication mode.
    pub fn get_auth_mode(&self) -> SslAuthenticationMode {
        self.auth_mode
    }

    /// Get the SNI enable setting.
    pub fn get_sni_enable(&self) -> SslSniEnable {
        self.sni_enable
    }

    /// Get the check host enable setting.
    pub fn get_checkhost_enable(&self) -> SslCheckHostEnable {
        self.checkhost_enable
    }

    /// Get the ignore local time setting.
    pub fn get_ignore_localtime(&self) -> SslIgnoreLocalTime {
        self.ignore_localtime
    }
}

impl Default for SslConfiguration {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_defaults() {
        let cfg = ModemConfiguration::new();
        assert_eq!(cfg.get_rat_order().as_str(), "020301");

        assert_eq!(
            cfg.get_band_string(RadioAccessTechnology::GSM)
                .unwrap()
                .as_str(),
            "0"
        );
        assert_eq!(
            cfg.get_band_string(RadioAccessTechnology::EMTC)
                .unwrap()
                .as_str(),
            "0"
        );
        assert_eq!(
            cfg.get_band_string(RadioAccessTechnology::NbIoT)
                .unwrap()
                .as_str(),
            "0"
        );
    }

    #[test]
    fn test_set_rat_order_valid() {
        let mut cfg = ModemConfiguration::new();
        let order = [
            RadioAccessTechnology::EMTC,
            RadioAccessTechnology::NbIoT,
            RadioAccessTechnology::GSM,
        ];
        assert!(cfg.set_rat_order(&order).is_ok());
        assert_eq!(cfg.get_rat_order().as_str(), "020301");
    }

    #[test]
    fn test_set_rat_order_errors() {
        let mut cfg = ModemConfiguration::new();

        assert!(cfg.set_rat_order(&[]).is_err());

        let too_many = [
            RadioAccessTechnology::GSM,
            RadioAccessTechnology::EMTC,
            RadioAccessTechnology::NbIoT,
            RadioAccessTechnology::GSM,
        ];
        assert!(cfg.set_rat_order(&too_many).is_err());

        let dup = [RadioAccessTechnology::EMTC, RadioAccessTechnology::EMTC];
        assert!(cfg.set_rat_order(&dup).is_err());
    }

    #[test]
    fn test_set_bands_gsm_and_emtc() {
        let mut cfg = ModemConfiguration::new();

        cfg.set_bands(
            RadioAccessTechnology::GSM,
            &[GsmBands::Gsm900MHz, GsmBands::Gsm1800MHz],
        )
        .unwrap();
        assert_eq!(
            cfg.get_band_string(RadioAccessTechnology::GSM)
                .unwrap()
                .as_str(),
            "3"
        );

        cfg.set_bands(
            RadioAccessTechnology::EMTC,
            &[EmtcBands::Band1, EmtcBands::Band3],
        )
        .unwrap();
        assert_eq!(
            cfg.get_band_string(RadioAccessTechnology::EMTC)
                .unwrap()
                .as_str(),
            "5"
        );
    }

    #[test]
    fn test_set_bands_nbiot_any() {
        let mut cfg = ModemConfiguration::new();

        cfg.set_bands(RadioAccessTechnology::NbIoT, &[NbIotBands::Any])
            .unwrap();

        let expected_val = NbIotBands::all_bands_mask();
        let expected_hex = format!("{:x}", expected_val);
        let actual = cfg.get_band_string(RadioAccessTechnology::NbIoT).unwrap();
        assert_eq!(actual.as_str(), expected_hex);
    }

    #[test]
    fn test_set_bands_empty_rejected() {
        let mut cfg = ModemConfiguration::new();
        assert!(cfg
            .set_bands::<EmtcBands>(RadioAccessTechnology::EMTC, &[])
            .is_err());
    }

    #[test]
    fn test_set_bands_any_with_extra_ignored() {
        let mut cfg = ModemConfiguration::new();

        cfg.set_bands(
            RadioAccessTechnology::NbIoT,
            &[NbIotBands::Any, NbIotBands::Band1, NbIotBands::Band3],
        )
        .unwrap();

        let expected_val = NbIotBands::all_bands_mask();
        let expected_hex = format!("{:x}", expected_val);
        let actual = cfg.get_band_string(RadioAccessTechnology::NbIoT).unwrap();
        assert_eq!(actual.as_str(), expected_hex);
    }

    #[test]
    fn test_set_bands_nochange_is_zero() {
        let mut cfg = ModemConfiguration::new();

        cfg.set_bands(RadioAccessTechnology::EMTC, &[EmtcBands::NoChange])
            .unwrap();

        assert_eq!(
            cfg.get_band_string(RadioAccessTechnology::EMTC)
                .unwrap()
                .as_str(),
            "0"
        );
    }

    #[test]
    fn test_get_band_string_overflow() {
        let mut cfg = ModemConfiguration::new();

        cfg.emtc_bands = 1u128 << 100;
        assert!(cfg.get_band_string(RadioAccessTechnology::EMTC).is_err());
    }

    #[test]
    fn test_ssl_config_defaults() {
        let ssl_config = SslConfiguration::new();
        assert_eq!(ssl_config.get_context_id(), 2);
        assert_eq!(ssl_config.get_ca_cert_filename(), "");
        assert_eq!(ssl_config.get_ssl_version(), SslVersion::Tls1_2);
        assert_eq!(ssl_config.get_cipher_suite(), None);
        assert_eq!(
            ssl_config.get_auth_mode(),
            SslAuthenticationMode::ServerOnly
        );
        assert_eq!(ssl_config.get_sni_enable(), SslSniEnable::Enable);
        assert_eq!(
            ssl_config.get_checkhost_enable(),
            SslCheckHostEnable::Disable
        );
        assert_eq!(
            ssl_config.get_ignore_localtime(),
            SslIgnoreLocalTime::Ignore
        );
    }

    #[test]
    fn test_ssl_config_builder_pattern() {
        let mut ssl_config = SslConfiguration::new();
        ssl_config
            .set_context_id(3)
            .unwrap()
            .set_ca_cert("cacert.pem")
            .unwrap()
            .set_ssl_version(SslVersion::Tls1_0)
            .set_cipher_suite(SslCipherSuiteEnum::TlsRsaWithAes256CbcSha)
            .set_auth_mode(SslAuthenticationMode::Mutual)
            .set_sni(false)
            .set_check_host(true)
            .set_ignore_localtime(false);

        assert_eq!(ssl_config.get_context_id(), 3);
        assert_eq!(ssl_config.get_ca_cert_filename(), "cacert.pem");
        assert_eq!(ssl_config.get_ssl_version(), SslVersion::Tls1_0);
        assert!(ssl_config.get_cipher_suite().is_some());
        assert_eq!(ssl_config.get_auth_mode(), SslAuthenticationMode::Mutual);
        assert_eq!(ssl_config.get_sni_enable(), SslSniEnable::Disable);
        assert_eq!(
            ssl_config.get_checkhost_enable(),
            SslCheckHostEnable::Enable
        );
        assert_eq!(ssl_config.get_ignore_localtime(), SslIgnoreLocalTime::Care);
    }

    #[test]
    fn test_ssl_config_invalid_context_id() {
        let mut ssl_config = SslConfiguration::new();
        assert!(ssl_config.set_context_id(6).is_err());
        assert_eq!(ssl_config.get_context_id(), 2); // Should remain unchanged
    }

    #[test]
    fn test_ssl_config_empty_cert() {
        let mut ssl_config = SslConfiguration::new();
        assert!(ssl_config.set_ca_cert("").is_ok());
        assert_eq!(ssl_config.get_ca_cert_filename(), "");
    }

    #[test]
    fn test_ssl_config_cipher_suite_all() {
        let mut ssl_config = SslConfiguration::new();
        ssl_config.set_cipher_suite(SslCipherSuiteEnum::TlsRsaWithAes256CbcSha);
        assert!(ssl_config.get_cipher_suite().is_some());

        ssl_config.set_cipher_suite_all();
        assert_eq!(ssl_config.get_cipher_suite(), None);
    }
}
