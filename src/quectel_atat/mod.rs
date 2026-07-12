pub mod responses;
pub mod types;
pub mod urc;

use atat::atat_derive::AtatCmd;
use atat::heapless::String;
use atat::heapless_bytes::Bytes;

use responses::*;
use types::*;

#[derive(Clone, AtatCmd)]
#[at_cmd("", NoResponse, timeout_ms = 1000)]
pub struct AT;

/// Echo On/Off E
///
/// This command configures whether or not the unit echoes the characters received
/// from the DTE in Command Mode. If <echo_on> is omitted, it turns off the echoing.
#[derive(Debug, PartialEq, Clone, AtatCmd)]
#[at_cmd("E", NoResponse, timeout_ms = 1000, value_sep = false)]
pub struct SetEcho {
    #[at_arg(position = 0)]
    pub on: EchoOn,
}

/// Reset to Factory Default
/// AT&F
/// This command resets all parameters to their factory default values.
///
/// The command responds with OK.
#[derive(Clone, AtatCmd)]
#[at_cmd("&F", NoResponse, timeout_ms = 300)]
pub struct ResetToFactoryDefault;

/// AT+CFUN Set UE Functionality
///
/// This command sets the UE functionality.
/// Note: This command can take up to 15 seconds to complete according to Quectel documentation.
#[derive(Debug, PartialEq, Clone, AtatCmd)]
#[at_cmd("+CFUN", NoResponse, timeout_ms = 15000)]
pub struct SetUeFunctionality {
    #[at_arg(position = 0)]
    pub fun: FunctionalityLevelOfUE,
}

/// AT+CPIN? Query SIM Card Status
///
/// This command is used to query the status of the SIM card.
///
#[derive(Clone, AtatCmd)]
#[at_cmd("+CPIN?", SimStatus, timeout_ms = 1000)]
pub struct GetSimStatus;

/// AT+QGMR Query Firmware Version
///
/// This command is used to query the firmware version of the module.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QGMR", VersionInfo, timeout_ms = 1000)]
pub struct GetVersionInfo;

/// AT+CGMR Query Firmware Version
///
/// This command is used to query the firmware version of the module.
/// It is a new version of QGMR command but only returns the first part
/// of the firmware version ("BG95M3LAR02A03" from "BG95M3LAR02A03_01.012.01.012").
#[derive(Clone, AtatCmd)]
#[at_cmd("+CGMR", VersionInfo, timeout_ms = 1000)]
pub struct GetVersionInfoCGMR;

/// AT+QCFG="band" Generic band Configuration
///
/// The command is used to configure the modem to narrow the search for the bands given in a bit mask
#[derive(Clone, AtatCmd)]
#[at_cmd("+QCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureBands {
    /// "band" fixed string
    #[at_arg(position = 1)]
    pub param: String<16>,
    /// <gsmbandval>
    /// GSM band mask (to see usable bands, check GsmBands enum)
    #[at_arg(position = 2)]
    pub gsm_band_mask: Bytes<24>,
    /// <emtcbandval>
    /// eMTC band mask string (to see usable bands, check EmtcBands)
    #[at_arg(position = 3)]
    pub emtc_band_mask: Bytes<24>,
    /// <nbiotbandval>
    /// NB-IoT band mask string (to see usable values, check NbIotBands)
    #[at_arg(position = 4)]
    pub nbiot_band_mask: Bytes<24>,
    /// <effect>
    /// determines when the command will take effect.
    /// The configurations will be saved automatically (1) or after a reboot (0).
    #[at_arg(position = 5)]
    pub effect: ConfigurationEffect,
}

/// AT+QCFG="nwscanseq" Configure RATs Searching Sequence
///
/// This Write Command configures the searching sequence of RATs or queries the current setting.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureRatSearchingSequence {
    /// "nwscanseq" fixed string
    #[at_arg(position = 1)]
    pub param: String<16>,
    /// <scanseq>
    /// Numeric String without quotes representing RATs searching sequence, e.g.: 020301 stands for eMTC → NB-IoT → GSM.
    /// 00 Automatic (eMTC → NB-IoT → GSM)
    /// 01 GSM
    /// 02 eMTC
    /// 03 NB-IoT
    #[at_arg(position = 2)]
    pub rat_searching_sequence: Bytes<8>,
    /// <effect>
    /// determines when the command will take effect.
    /// The configurations will be saved automatically (1) or after a reboot (0).
    #[at_arg(position = 3)]
    pub effect: ConfigurationEffect,
}

/// AT+QCFG="nvrestore",0 Restore Factory Configuration
/// This command restores the factory configuration.
/// The command responds with OK.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QCFG=\"nvrestore\",0", NoResponse, timeout_ms = 300)]
pub struct RestoreFactoryConfiguration;

/// AT+QCFG="nwscanmode" Configure RATs Searching Mode
///
/// This Write Command configures the searching mode of RATs or queries the current setting.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureRatSearchingMode {
    /// "nwscanmode" fixed string
    #[at_arg(position = 1)]
    pub param: String<16>,
    /// <scanmode>
    /// Numeric String without quotes representing RATs searching mode, e.g.: 0 stands for Automatic.
    /// 0 Automatic (GSM and LTE)
    /// 1 GSM only
    /// 3 LTE only
    #[at_arg(position = 2)]
    pub rat_searching_mode: u8,
    /// <effect>
    /// determines when the command will take effect.
    /// The configurations will be saved automatically (1) or after a reboot (0).
    #[at_arg(position = 3)]
    pub effect: ConfigurationEffect,
}

/// AT+QCFG="servicedomain" Configure Service Domain
///
/// This Write Command configures the service domain to be registered or queries the current setting.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureServiceDomain {
    /// "servicedomain" fixed string
    #[at_arg(position = 1)]
    pub param: String<16>,
    /// <service>
    /// Integer type. Service domain to be registered.
    /// 1 PS only
    /// 2 CS & PS
    #[at_arg(position = 2)]
    pub service_domain: u8,
    /// <effect>
    /// determines when the command will take effect.
    /// The configurations will be saved automatically (1) or after a reboot (0).
    #[at_arg(position = 3)]
    pub effect: ConfigurationEffect,
}

/// AT+QCFG="iotopmode" Configure Network Category to be Searched for under LTE RAT
///
/// This Write Command configures the network category to be searched for under LTE RAT or queries the
/// current setting.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureIotOpMode {
    /// "iotopmode" fixed string
    #[at_arg(position = 1)]
    pub param: String<16>,
    /// <iotopmode>
    /// Integer type. Network category to be searched for under LTE RAT.
    /// 0 eMTC
    /// 1 NB-IoT
    /// 2 eMTC and NB-IoT
    #[at_arg(position = 2)]
    pub mode: u8,
    /// <effect>
    /// determines when the command will take effect.
    /// The configurations will be saved automatically (1) or after a reboot (0).
    #[at_arg(position = 3)]
    pub effect: ConfigurationEffect,
}

/// AT+QICSGP Configure Parameters of a TCP/IP Context
///
/// This command configures the <APN>, <username>, <password> and other parameters of a TCP/IP
/// context.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QICSGP", NoResponse, timeout_ms = 300)]
pub struct ConfigureContext {
    /// <contextid>
    /// Integer type. The context ID. The range is from 1 to 16.
    #[at_arg(position = 0)]
    pub context_id: u8,
    /// <contexttype>
    /// Integer type. The context type.
    /// 1: IPV4
    /// 2: IPV6
    /// 3: IPV4V6
    #[at_arg(position = 1)]
    pub context_type: u8,
    /// <apn>
    /// String type. The APN.
    #[at_arg(position = 2)]
    pub apn: String<64>,
    /// <username>
    /// String type. The username.
    #[at_arg(position = 3)]
    pub username: String<64>,
    /// <password>
    /// String type. The password.
    #[at_arg(position = 4)]
    pub password: String<64>,
    /// <authentication>
    /// AuthenticationMethod type.
    #[at_arg(position = 5)]
    pub authentication: u8,
}

/// AT+QNWINFO Query Network Information
///
/// This command indicates network information such as the access technology selected, the operator, and
/// the band selected.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QNWINFO", NetworkInfo, timeout_ms = 300)]
pub struct GetNetworkInfo;

#[derive(Clone, AtatCmd)]
#[at_cmd(r#"+QENG="servingcell""#, ServingCellInfo, timeout_ms = 300)]
pub struct GetServingCellInfo;

#[derive(Clone, AtatCmd)]
#[at_cmd("+COPS?", CopsResponse, timeout_ms = 300)]
pub struct GetCopsInfo;

/// AT+CEREG EPS Network Registration Status
///
/// This command queries the LTE network registration status and controls the presentation of an unsolicited
/// result code +CEREG: <stat> when <n>=1 and there is a change in the MT’s EPS network registration
/// status in E-UTRAN, or unsolicited result code +CEREG: <stat>[,[<tac>],[<ci>],[<AcT>]] when <n>=2
/// and there is a change of the network cell in E-UTRAN.
#[derive(Clone, AtatCmd)]
#[at_cmd("+CEREG?", EPSNetworkRegistrationStatusResponse, timeout_ms = 300)]
pub struct GetEPSNetworkRegistrationStatus;

/// AT+CGREG EGPRS Network Registration Status
///
/// This command queries the EGPRS network registration status and controls the presentation of an
/// unsolicited result code +CGREG: <stat> when <n>=1 and there is a change in the MT’s EGPRS network
/// registration status in GERAN, or unsolicited result code +CGREG: <stat>[,[<lac>],[<ci>],[<AcT>],[<rac>]]
/// when <n>=2 and there is a change of the network cell in GERAN.
#[derive(Clone, AtatCmd)]
#[at_cmd("+CGREG?", EGPRSNetworkRegistrationStatusResponse, timeout_ms = 300)]
pub struct GetEGPRSNetworkRegistrationStatus;

/// AT+QCSQ Query and Report Signal Strength
///
/// The command is used to query and report the signal strength of the current service network. If the MT is
/// registered on multiple networks in different service modes, customers can query the signal strength of
/// networks in each mode. No matter whether the MT is registered on a network or not, the command can be
/// run to query the signal strength or allow the MT to unsolicitedly report the detected signal strength if the
/// MT camps on the network. If the MT is not using any service network or the service mode is uncertain,
/// "NOSERVICE" will be returned as the query result.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QCSQ", GetSignalStrengthResponse, timeout_ms = 300)]
pub struct GetSignalStrength;

/// AT+QIACT Activate a PDP Context and query
///
/// Before activating a PDP context with AT+QIACT, the context should be configured by AT+QICSGP. After
/// activation, the IP address can be queried with AT+QIACT?. Although the range of <contextID> is 1–16,
/// the module supports maximum three PDP contexts activated simultaneously under LTE Cat M/EGPRS and
/// maximum two under LTE Cat NB2. Depending on the network, it may take at most 150 seconds to return
/// OK or ERROR after executing AT+QIACT. Before the response is returned, other AT commands cannot be executed.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QIACT", NoResponse, timeout_ms = 150000)]
pub struct ActivatePDPContext {
    /// <contextid>
    /// Integer type. The context ID. The range is from 1 to 16.
    #[at_arg(position = 1)]
    pub context_id: u8,
}

#[derive(Clone, AtatCmd)]
#[at_cmd("+QIACT?", PDPContextInfo, timeout_ms = 300)]
pub struct GetPDPContextInfo;

/// AT+QIACT Deactivate a PDP Context
///
/// This command deactivates a specific context and close all TCP/IP connections set up in this context.
/// Depending on the network, it may take at most 40 seconds to return OK or ERROR after executing
/// AT+QIDEACT. Before the response is returned, other AT commands cannot be executed.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QIDEACT", NoResponse, timeout_ms = 40000)]
pub struct DeactivatePDPContext {
    /// <contextid>
    /// Integer type. The context ID. The range is from 1 to 16.
    #[at_arg(position = 1)]
    pub context_id: u8,
}

/// AT+QLTS Obtain the Latest Time Synchronized Through Network
///
/// The Execution Command returns the latest time synchronized through network.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QLTS", NitzTimeResponse, timeout_ms = 300)]
pub struct GetNetworkNitzTime {
    /// <mode>
    /// Integer type. Query network time mode
    /// 0: Query the latest time that has been synchronized through network
    /// 1: Query the current GMT time calculated from the latest time that has been synchronized through network
    /// 2: Query the current LOCAL time calculated from the latest time that has been synchronized through network
    #[at_arg(position = 1)]
    pub mode: u8,
}

/// AT+QNTP Synchronize Local Time with NTP Server
///
/// The Write Command synchronizes UTC with the NTP server. Before using NTP, the host should activate the context.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QNTP", NoResponse, timeout_ms = 300)]
pub struct GetNetworkNtpTime {
    /// <contextid>
    /// Integer type. The context ID. The range is from 1 to 16.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <server>
    /// String type. The NTP server address. The maximum length is 100 bytes.
    #[at_arg(position = 2)]
    pub server: String<100>,
}

/// AT+QMTCFG="version"
///
/// Sets the MQTT version
#[derive(Clone, AtatCmd)]
#[at_cmd("+QMTCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureMqttVersion {
    /// <subcommand>
    /// String literal. The MQTT configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <tcpconnectID>
    /// Integer type. MQTT socket identifier. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub tcp_connect_id: u8,
    /// <version>
    /// Integer type. MQTT version.
    pub version: MqttVersion,
}

/// AT+QMTCFG="ssl"
///
/// Sets the SSL configuration for MQTT
#[derive(Clone, AtatCmd)]
#[at_cmd("+QMTCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureMqttSsl {
    /// <subcommand>
    /// String literal. The MQTT configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <tcpconnectID>
    /// Integer type. MQTT socket identifier. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub tcp_connect_id: u8,
    /// <ssl_enable>
    /// Integer type. SSL enable flag.
    #[at_arg(position = 2)]
    pub ssl_enable: MqttSslEnable,
    /// <sslctxid>
    /// Integer type. SSL context identifier. The range is from 0 to 5.
    #[at_arg(position = 3)]
    pub ssl_ctx_id: u8,
}

/// AT+QMTOPEN Open a Network for MQTT Client and query
///
/// The command is used to open a network for MQTT client.
///
/// The command responds with OK. We need to get the response from the URC +QMTOPEN,
/// that can last up to 75 seconds and returns a MqttOpenResponse.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QMTOPEN", NoResponse, timeout_ms = 300)]
pub struct MqttOpen {
    /// <tcpconnectID>
    /// Integer type. MQTT socket identifier. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub tcp_connect_id: u8,
    /// <server>
    /// String type. The server address. The maximum length is 100 bytes.
    #[at_arg(position = 2)]
    pub server: String<100>,
    /// <port>
    /// Integer type. The server port. The range is 1-65535.
    #[at_arg(position = 3)]
    pub port: u16,
}

/// AT+QMTCONN Establish an MQTT Connection
///
/// The command is used to establish an MQTT connection. To be used after the TCP connection is established.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QMTCONN", NoResponse, timeout_ms = 5000)]
pub struct MqttConnect {
    /// <tcpconnectID>
    /// Integer type. MQTT socket identifier. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub tcp_connect_id: u8,
    /// <clientID>
    /// String type. The client identifier. The maximum length is 23 bytes.
    #[at_arg(position = 2)]
    pub client_id: String<23>,
    /// <username>
    /// String type. The username. The maximum length is 64 bytes.
    #[at_arg(position = 3)]
    pub username: Option<String<64>>,
    /// <password>
    /// String type. The password. The maximum length is 64 bytes.
    #[at_arg(position = 4)]
    pub password: Option<String<64>>,
}

/// AT+QMTPUBEX Publish an MQTT Message with Extended Parameters
///
/// The command responds with OK. We need to get the response from the URC +QMTPUB.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QMTPUBEX", NoResponse, timeout_ms = 300)]
pub struct MqttPublishExtended {
    /// <tcpconnectID>
    /// Integer type. MQTT socket identifier. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub tcp_connect_id: u8,
    /// <msg_id>
    /// Integer type. The message identifier. The range is from 0 to 65535.
    #[at_arg(position = 2)]
    pub msg_id: u16,
    /// <qos>
    /// Integer type. The QoS level. The range is from 0 to 2.
    /// 0: At most once
    /// 1: At least once
    /// 2: Exactly once
    #[at_arg(position = 3)]
    pub qos: u8,
    /// <retain>
    /// Integer type. Retain flag. The range is from 0 to 1.
    /// 0: The server must publish the message as if the message was not retained.
    /// 1: The server must publish the message as if the message was retained.
    #[at_arg(position = 4)]
    pub retain: u8,
    /// <topic>
    /// String type. The topic. The maximum length is 128 bytes.
    /// The topic name must be a UTF-8 encoded string.
    /// The topic name must not include the wildcard characters + and #.
    #[at_arg(position = 5)]
    pub topic: String<128>,
    /// <payload>
    /// String type. The payload. The maximum length is 1024 bytes.
    /// The payload must be a UTF-8 encoded string.
    /// The payload must not include the null character.
    #[at_arg(position = 6)]
    pub payload: String<1024>,
}

/// AT+QMTDISC Disconnect a MQTT Connection
///
/// The command is used when a client requests a disconnection from MQTT server. A DISCONNECT
/// message is sent from the client to the server to indicate that it is about to close its TCP/IP connection.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QMTDISC", NoResponse, timeout_ms = 300)]
pub struct MqttDisconnect {
    /// <tcpconnectID>
    /// Integer type. MQTT socket identifier. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub tcp_connect_id: u8,
}

/// AT+QMTCLOSE Close an MQTT Network
///
/// The command is used to close a network for MQTT client.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QMTCLOSE", NoResponse, timeout_ms = 300)]
pub struct MqttClose {
    /// <tcpconnectID>
    /// Integer type. MQTT socket identifier. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub tcp_connect_id: u8,
}

/// AT+QPOWD Power Down the Module
///
/// This command powers down the module.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QPOWD", NoResponse, timeout_ms = 300)]
pub struct PowerDown {
    /// Integer type.
    #[at_arg(position = 1)]
    pub mode: PowerDownMode,
}

/// AT+GSN Request International Mobile Equipment Identity (IMEI)
///
/// This command returns the International Mobile Equipment Identity (IMEI) number of the product in
/// information text which permits the user to identify the individual ME device. It is identical with AT+CGSN.
#[derive(Clone, AtatCmd)]
#[at_cmd("+GSN", Imei, timeout_ms = 300)]
pub struct GetImei;

/// AT+QCCID Show Integrated Circuit Card Identifier (ICCID)
///
/// The command returns the ICCID (Integrated Circuit Card Identifier) number of the (U)SIM card.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QCCID", Iccid, timeout_ms = 300)]
pub struct GetIccid;

/// AT+QGPS Turn on GNSS
///
/// Turns on the GNSS function.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QGPS", NoResponse, timeout_ms = 300)]
pub struct TurnOnGnss {
    /// <GNSS_mode>
    /// Integer type. GNSS operationg mode. Range from 1 to 4 (BG95 only supports mode = 1)
    #[at_arg(position = 1)]
    pub mode: GnssOperatingMode,
}

/// AT+QGPSEND Turn off GNSS
///
/// Turns off GNSS when <fixcount> is 0 (default).
#[derive(Clone, AtatCmd)]
#[at_cmd("+QGPSEND", NoResponse, timeout_ms = 300)]
pub struct TurnOffGnss;

/// AT+QGPSLOC
///
/// Gets gnss positioning data
#[derive(Clone, AtatCmd)]
#[at_cmd("+QGPSLOC=2", GnssPositionInformationResponse, timeout_ms = 300)]
pub struct GetGnssPositionInformation;

/// AT+QGPSCFG="gnssconfig"
///
/// Sets the GNSS constellation used to Galileo (European constellation)
#[derive(Clone, AtatCmd)]
#[at_cmd("+QGPSCFG", NoResponse, timeout_ms = 300)]
pub struct SetGnssConstellation {
    /// "gnssconfig" fixed string
    #[at_arg(position = 1)]
    pub param: String<16>,
    #[at_arg(position = 2)]
    pub constellation: GnssConstellation,
}

/// AT+QGPSCFG="priority"
///
/// Sets the priority mode (by default the GNSS module has priority)
#[derive(Clone, AtatCmd)]
#[at_cmd("+QGPSCFG=\"priority\",0,0", NoResponse, timeout_ms = 300)]
pub struct ConfigureGnssPriorityMode;

/// AT+QGPSNMEA="GGA"
///
/// Gets the NMEA sentence of the GNSS location in GGA format
#[derive(Clone, AtatCmd)]
#[at_cmd("+QGPSGNMEA=\"GGA\"", GnssGgaNmeaSentenceResponse, timeout_ms = 300)]
pub struct GetGgaNmeaSentence;

/// AT+QSSLCFG="sslversion"
///
/// Sets the SSL version
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureSslVersion {
    /// <subcommand>
    /// String literal. The SSL configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <contextid>
    /// Integer type. The context ID. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <sslversion>
    /// Integer type. SSL version.
    /// 0: SSLv3
    /// 1: TLSv1.0
    /// 2: TLSv1.1
    /// 3: TLSv1.2
    #[at_arg(position = 2)]
    pub ssl_version: SslVersion,
}

/// AT+QSSLCFG="ciphersuite"
///
/// Sets the SSL cipher suites
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureSslCipherSuites {
    /// <subcommand>
    /// String literal. The SSL configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <contextid>
    /// Integer type. The context ID. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <ciphersuite>
    /// Raw bytes type. SSL cipher suites in a hex string format.
    #[at_arg(position = 2)]
    pub cipher_suites: SslCipherSuites,
}

/// AT+QSSLCFG="cacert"
///
/// Sets the CA certificate path
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureSslCaCertificate {
    /// <subcommand>
    /// String literal. The SSL configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <contextid>
    /// Integer type. The context ID. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <cacertpath>
    /// String type. The CA certificate file path in the file system.
    #[at_arg(position = 2)]
    pub ca_cert_path: String<128>,
}

/// AT+QSSLCFG="clientcert"
///
/// Sets the client certificate path
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureSslClientCertificate {
    /// <subcommand>
    /// String literal. The SSL configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <contextid>
    /// Integer type. The context ID. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <clientcertpath>
    /// String type. The client certificate file path in the file system.
    #[at_arg(position = 2)]
    pub client_cert_path: String<128>,
}

/// AT+QSSLCFG="clientkey"
///
/// Sets the client private key path
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureSslClientPrivateKey {
    /// <subcommand>
    /// String literal. The SSL configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <contextid>
    /// Integer type. The context ID. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <clientkeypath>
    /// String type. The client private key file path in the file system.
    #[at_arg(position = 2)]
    pub client_key_path: String<128>,
}

/// AT+QSSLCFG="seclevel"
///
/// Sets the SSL security level
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureSslSecurityLevel {
    /// <subcommand>
    /// String literal. The SSL configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <contextid>
    /// Integer type. The context ID. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <seclevel>
    /// Integer type. SSL security level.
    #[at_arg(position = 2)]
    pub security_level: SslAuthenticationMode,
}

/// AT+QSSLCFG="ignorelocaltime"
///
/// Sets whether to ignore the validity check
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureSslIgnoreLocalTime {
    /// <subcommand>
    /// String literal. The SSL configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <contextid>
    /// Integer type. The context ID. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <ignorelocaltime>
    /// Integer type. Whether to ignore local time when verifying the server certificate.
    #[at_arg(position = 2)]
    pub ignore_local_time: SslIgnoreLocalTime,
}

/// AT+QSSLCFG="sni"
///
/// Sets the Server Name Indication (SNI) feature
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureSslSni {
    /// <subcommand>
    /// String literal. The SSL configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <contextid>
    /// Integer type. The context ID. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <sni_enable>
    /// Integer type. Enable or disable Server Name Indication.
    /// 0: Disable
    /// 1: Enable
    #[at_arg(position = 2)]
    pub sni_enable: SslSniEnable,
}

/// AT+QSSLCFG="checkhost"
///
/// Sets the hostname validation feature
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLCFG", NoResponse, timeout_ms = 300)]
pub struct ConfigureSslCheckHost {
    /// <subcommand>
    /// String literal. The SSL configuration subcommand.
    #[at_arg(position = 0)]
    pub subcommand: String<16>,
    /// <contextid>
    /// Integer type. The context ID. The range is from 0 to 5.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <checkhost_enable>
    /// Integer type. Enable or disable hostname validation.
    /// 0: Disable
    /// 1: Enable
    #[at_arg(position = 2)]
    pub checkhost_enable: SslCheckHostEnable,
}

// ---------------------------------------------------------------------------
// TCP / SSL socket commands (AT+QSSLOPEN / QSSLSEND / QSSLRECV / QSSLCLOSE)
//
// These drive the modem's own TCP+TLS engine in *buffer access mode*
// (<access_mode> = 0): the MCU pushes/pulls payload with QSSLSEND/QSSLRECV and
// is notified of readable data via the `+QSSLURC: "recv",<id>` URC. mTLS is
// configured beforehand through the `AT+QSSLCFG` commands above.
// ---------------------------------------------------------------------------

/// AT+QSSLOPEN Open an SSL socket.
///
/// `AT+QSSLOPEN=<pdpCtxID>,<sslCtxID>,<clientID>,<host>,<port>,<accessMode>`
///
/// Responds with `OK`, then the actual result arrives asynchronously as the
/// `+QSSLOPEN: <clientID>,<err>` URC (handshake can take several seconds).
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLOPEN", NoResponse, timeout_ms = 500)]
pub struct SslOpen {
    /// <pdpCtxID> — PDP context id (the activated data context, usually 1).
    #[at_arg(position = 1)]
    pub pdp_ctx_id: u8,
    /// <sslCtxID> — SSL context id configured via AT+QSSLCFG (0-5).
    #[at_arg(position = 2)]
    pub ssl_ctx_id: u8,
    /// <clientID> — socket identifier to allocate (0-11).
    #[at_arg(position = 3)]
    pub client_id: u8,
    /// <host> — server hostname or IP (quoted). Used for SNI when enabled.
    #[at_arg(position = 4)]
    pub host: String<128>,
    /// <port> — server port.
    #[at_arg(position = 5)]
    pub port: u16,
    /// <accessMode> — 0: buffer access (use QSSLRECV), 1: direct push, 2: transparent.
    #[at_arg(position = 6)]
    pub access_mode: u8,
}

/// AT+QSSLSEND Send data on an SSL socket.
///
/// `AT+QSSLSEND=<clientID>,<length>` — the modem replies with a `>` prompt,
/// after which exactly `<length>` raw bytes are written (see
/// [`SendRawContents`]) and the modem answers `SEND OK` / `SEND FAIL`.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLSEND", NoResponse, timeout_ms = 1000)]
pub struct SslSend {
    /// <clientID> — socket identifier.
    #[at_arg(position = 1)]
    pub client_id: u8,
    /// <length> — number of bytes that will follow the `>` prompt.
    #[at_arg(position = 2)]
    pub length: u16,
}

/// AT+QSSLCLOSE Close an SSL socket.
///
/// `AT+QSSLCLOSE=<clientID>,<timeout>`
#[derive(Clone, AtatCmd)]
#[at_cmd("+QSSLCLOSE", NoResponse, timeout_ms = 10000)]
pub struct SslClose {
    /// <clientID> — socket identifier.
    #[at_arg(position = 1)]
    pub client_id: u8,
    /// <timeout> — seconds to wait for graceful close.
    #[at_arg(position = 2)]
    pub timeout: u16,
}

/// AT+QSSLRECV Read buffered data from an SSL socket.
///
/// `AT+QSSLRECV=<clientID>,<length>` → `+QSSLRECV: <actualLen>\r\n<binary>\r\nOK`.
/// A custom parser extracts `<actualLen>` and the following raw bytes.
#[derive(Clone)]
pub struct SslRecv {
    /// <clientID> — socket identifier.
    pub client_id: u8,
    /// <length> — maximum number of bytes to read (capped at 512 by the buffer).
    pub length: u16,
}

impl atat::AtatCmd for SslRecv {
    type Response = SslRecvResponse;
    const MAX_LEN: usize = 32;

    fn write(&self, buf: &mut [u8]) -> usize {
        use core::fmt::Write as _;
        use embedded_io::Write;

        let original_len = buf.len();
        let mut writer = buf;

        let mut cmd = atat::heapless::String::<32>::new();
        write!(cmd, "AT+QSSLRECV={},{}\r", self.client_id, self.length).unwrap();
        writer.write(cmd.as_bytes()).unwrap();

        original_len - writer.len()
    }

    fn parse(
        &self,
        resp: Result<&[u8], atat::InternalError>,
    ) -> Result<Self::Response, atat::Error> {
        let data = resp.map_err(atat::Error::from)?;

        // Locate the "+QSSLRECV: " header.
        let header = b"+QSSLRECV: ";
        let start = data
            .windows(header.len())
            .position(|w| w == header)
            .ok_or(atat::Error::InvalidResponse)?
            + header.len();
        let after_header = &data[start..];

        // The decimal length runs up to the first CRLF.
        let crlf = after_header
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or(atat::Error::InvalidResponse)?;
        let actual_len = core::str::from_utf8(&after_header[..crlf])
            .map_err(|_| atat::Error::InvalidResponse)?
            .trim()
            .parse::<u16>()
            .map_err(|_| atat::Error::InvalidResponse)?;

        let payload = &after_header[crlf + 2..];
        let to_copy = core::cmp::min(actual_len as usize, 512);
        let to_copy = core::cmp::min(to_copy, payload.len());

        let mut buffer = Bytes::<512>::new();
        buffer
            .extend_from_slice(&payload[..to_copy])
            .map_err(|_| atat::Error::InvalidResponse)?;

        Ok(SslRecvResponse {
            length: to_copy as u16,
            data: buffer,
        })
    }
}

/// AT+QFUPL
///
/// Upload a file to internal flash. Uses UFS (User File Storage) by default.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QFUPL", FileDataModeStarted, timeout_ms = 1000)]
pub struct FileUploadToInternalFlash {
    /// <filename>
    /// String type. The file path in the file system.
    #[at_arg(position = 1)]
    pub file_path: String<80>,
    /// <file_size>
    /// Integer type. The size of the file to be uploaded in bytes.
    #[at_arg(position = 2)]
    pub file_size: u32,
    /// <timeout>
    /// Integer type. The time waiting for data to be inputted in seconds.
    #[at_arg(position = 3)]
    pub timeout: Option<u16>,
    /// <ackmode>
    /// Integer type. Wheter to use acknowledgment mode.
    #[at_arg(position = 4)]
    pub ack_mode: Option<bool>,
}

// /// Raw file contents.
// #[derive(Clone, AtatCmd)]
// #[at_cmd("", NoResponse, timeout_ms = 300)]
// pub struct FileRawContents {
//     /// <file_contents>
//     /// Raw bytes. The file contents.
//     #[at_arg(position = 1)]
//     pub file_contents: Bytes<1024>,
// }

/// Raw data to be sent after receiving CONNECT
pub struct SendRawContents {
    /// <bytes>
    /// Raw bytes. For example, part of a file contents.
    pub bytes: Bytes<256>,
}

impl atat::AtatCmd for SendRawContents {
    type Response = NoResponse;
    const MAX_LEN: usize = 2560;
    const EXPECTS_RESPONSE_CODE: bool = false;

    fn write(&self, mut buf: &mut [u8]) -> usize {
        let buf_len = buf.len();
        use embedded_io::Write;
        // Write raw bytes directly without formatting
        buf.write(&self.bytes).unwrap();
        buf_len - buf.len()
    }

    // Parse response after sending raw bytes: no response expected here
    fn parse(
        &self,
        _resp: Result<&[u8], atat::InternalError>,
    ) -> Result<Self::Response, atat::Error> {
        Ok(NoResponse)
    }
}

/// AT+QFDEL
///
/// Delete a given file or all the files in the storage. Uses UFS (User File Storage) by default.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QFDEL", NoResponse, timeout_ms = 300)]
pub struct DeleteFileFromInternalFlash {
    /// <filename>
    /// String type. The file path in the file system.
    #[at_arg(position = 1)]
    pub file_path: String<80>,
}

/// AT+QFLST
///
/// List the file information from the internal flash storage.
/// This command lists the information of a single file or all files in the specified storage.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QFLST", FileListResponse, timeout_ms = 1000)]
pub struct ListFilesFromInternalFlash {
    /// <name_pattern>
    /// String type. The file pattern to be listed.
    /// Examples:
    /// "*" - All files in UFS
    /// "UFS:*" - All files in UFS
    /// "<filename>" - A specified file in UFS
    /// "UFS:<filename>" - A specified file in UFS
    #[at_arg(position = 1)]
    pub name_pattern: String<80>,
}

/// AT+QFDWL
///
/// Download a file from internal flash. Uses UFS (User File Storage) by default.
/// The modem will respond with CONNECT, then binary data, then +QFDWL response.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QFDWL", FileDownloadDoneResponse, timeout_ms = 1000)]
pub struct DownloadFileFromInternalFlash {
    /// <filename>
    /// String type. Name of the file to be downloaded. The maximum length is 80 bytes.
    /// Examples:
    /// "<filename>" - Download from UFS
    /// "UFS:<filename>" - Download from UFS
    /// "EUFS:<filename>" - Download from ufs directory of EUFS
    #[at_arg(position = 1)]
    pub file_path: String<80>,
}

/// AT+QFOPEN
///
/// Open a file and get the file handle to be used in subsequent commands.
/// Mode 0: Create if not exist, open if exists (read/write)
/// Mode 1: Create/overwrite (read/write)
/// Mode 2: Open existing for read only
/// Mode 3: Create if not exist, append if exists (read/write)
#[derive(Clone, AtatCmd)]
#[at_cmd("+QFOPEN", FileOpenResponse, timeout_ms = 1000)]
pub struct OpenFile {
    /// <filename>
    /// String type. Name of the file to be opened. The maximum length is 80 bytes.
    #[at_arg(position = 1)]
    pub filename: String<80>,
    /// <mode>
    /// Integer type. The open mode of the file.
    /// 0: Create if not exist, open if exists (read/write)
    /// 1: Create/overwrite (read/write)
    /// 2: Open existing for read only
    /// 3: Create if not exist, append if exists (read/write)
    #[at_arg(position = 2)]
    pub mode: Option<u8>,
}

/// AT+QFSEEK
///
/// Set file pointer to a specified position.
/// Position modes:
/// 0: From the beginning of the file
/// 1: From the current position
/// 2: From the end of the file
#[derive(Clone, AtatCmd)]
#[at_cmd("+QFSEEK", NoResponse, timeout_ms = 1000)]
pub struct SeekFile {
    /// <filehandle>
    /// Integer type. The handle of the file to be operated.
    #[at_arg(position = 1)]
    pub filehandle: u32,
    /// <offset>
    /// Integer type. The number of bytes of the file pointer movement.
    #[at_arg(position = 2)]
    pub offset: u32,
    /// <position>
    /// Integer type. Pointer movement mode.
    /// 0: From the beginning of the file
    /// 1: From the current position
    /// 2: From the end of the file
    #[at_arg(position = 3)]
    pub position: Option<u8>,
}

/// AT+QFREAD
///
/// Read data from a file. The modem responds with CONNECT <read_length>, binary data, then OK.
#[derive(Clone)]
pub struct ReadFile {
    /// <filehandle>
    /// Integer type. The handle of the file to be operated.
    pub filehandle: u32,
    /// <length>
    /// Integer type. The length of the file to be read out. If omitted, reads entire file.
    pub length: Option<u32>,
}

impl atat::AtatCmd for ReadFile {
    type Response = FileReadStarted;
    const MAX_LEN: usize = 64;

    fn write(&self, buf: &mut [u8]) -> usize {
        use core::fmt::Write as _;
        use embedded_io::Write;

        let original_len = buf.len();
        let mut writer = buf;

        let mut cmd = atat::heapless::String::<64>::new();
        if let Some(length) = self.length {
            write!(cmd, "AT+QFREAD={},{}\r", self.filehandle, length).unwrap();
        } else {
            write!(cmd, "AT+QFREAD={}\r", self.filehandle).unwrap();
        }
        writer.write(cmd.as_bytes()).unwrap();

        original_len - writer.len()
    }

    fn parse(
        &self,
        resp: Result<&[u8], atat::InternalError>,
    ) -> Result<Self::Response, atat::Error> {
        match resp {
            Ok(data) => {
                // Response format: "CONNECT <read_length>\r\n<binary_data>"
                // Parse the CONNECT line and extract both read_length and binary data

                // Try to find "CONNECT " in the response
                if let Some(connect_pos) = data.iter().position(|&b| b == b'C') {
                    // Check if this is indeed "CONNECT "
                    let connect_str = b"CONNECT ";
                    if data.len() >= connect_pos + connect_str.len()
                        && &data[connect_pos..connect_pos + connect_str.len()] == connect_str
                    {
                        let after_connect = &data[connect_pos + connect_str.len()..];

                        // Find the \r\n that separates the length from the binary data
                        if let Some(crlf_pos) = after_connect.windows(2).position(|w| w == b"\r\n")
                        {
                            // Extract the length
                            let length_bytes = &after_connect[..crlf_pos];
                            let length_str = core::str::from_utf8(length_bytes)
                                .map_err(|_| atat::Error::InvalidResponse)?;

                            if let Ok(read_length) = length_str.trim().parse::<u32>() {
                                // Extract the binary data after \r\n
                                let binary_start = crlf_pos + 2; // Skip \r\n
                                let binary_data = &after_connect[binary_start..];

                                // Strict: require at least read_length bytes in binary_data
                                // If not enough bytes, log a warning and accept what is available (modem bug workaround)
                                let to_copy = core::cmp::min(read_length as usize, 256); // Max buffer size
                                if binary_data.len() < read_length as usize {
                                    // Accept available bytes, but log a warning and document as modem bug
                                    log::warn!("[MODEM BUG?] CONNECT response: expected {} bytes, got {} bytes. Accepting available bytes as last chunk.", read_length, binary_data.len());
                                    let mut data_buffer = Bytes::<256>::new();
                                    data_buffer
                                        .extend_from_slice(
                                            &binary_data
                                                [..core::cmp::min(binary_data.len(), to_copy)],
                                        )
                                        .map_err(|_| atat::Error::InvalidResponse)?;
                                    log::debug!(
                                        "Parsed read_length: {}, extracted {} bytes (short chunk)",
                                        read_length,
                                        data_buffer.len()
                                    );
                                    return Ok(FileReadStarted {
                                        read_length: binary_data.len() as u32,
                                        data: data_buffer,
                                    });
                                }

                                let mut data_buffer = Bytes::<256>::new();
                                data_buffer
                                    .extend_from_slice(&binary_data[..to_copy])
                                    .map_err(|_| atat::Error::InvalidResponse)?;

                                log::debug!(
                                    "Parsed read_length: {}, extracted {} bytes",
                                    read_length,
                                    to_copy
                                );

                                return Ok(FileReadStarted {
                                    read_length,
                                    data: data_buffer,
                                });
                            }
                        }
                    }
                }

                log::error!("Failed to parse CONNECT response");
                Err(atat::Error::InvalidResponse)
            }
            Err(e) => {
                log::error!("Error reading file: {:?}", e);
                Err(atat::Error::from(e))
            }
        }
    }
}

/// AT+QFWRITE
///
/// Write data to a file. The modem responds with CONNECT, then accepts binary data.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QFWRITE", FileDataModeStarted, timeout_ms = 5000)]
pub struct WriteFile {
    /// <filehandle>
    /// Integer type. The handle of the file to be operated.
    #[at_arg(position = 1)]
    pub filehandle: u32,
    /// <length>
    /// Integer type. The length of the file to be written.
    #[at_arg(position = 2)]
    pub length: u32,
    /// <timeout>
    /// Integer type. The time waiting for data. Range: 1-65535. Default: 5. Unit: seconds.
    #[at_arg(position = 3)]
    pub timeout: Option<u16>,
}

/// AT+QFCLOSE
///
/// Close an opened file.
#[derive(Clone, AtatCmd)]
#[at_cmd("+QFCLOSE", NoResponse, timeout_ms = 1000)]
pub struct CloseFile {
    /// <filehandle>
    /// Integer type. The handle of the file to be closed.
    #[at_arg(position = 1)]
    pub filehandle: u32,
}
