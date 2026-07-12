use atat::atat_derive::AtatResp;
use atat::heapless::String;
use atat::heapless_bytes::Bytes;

#[derive(Clone, AtatResp)]
pub struct NoResponse;

#[derive(Clone, AtatResp)]
pub struct OkResponse {
    pub code: String<2>,
}

#[derive(Clone, AtatResp)]
pub struct Ready;

#[derive(Clone, AtatResp)]
pub struct AppReady;

#[derive(Clone, AtatResp, Debug)]
pub struct MessageWaitingIndication;

/// Imei
///
/// International Mobile Equipment Identity (IMEI) number of the module.
#[derive(Clone, Debug, AtatResp)]
pub struct Imei {
    pub imei: Bytes<15>,
}

/// ICCID
///
/// Integrated Circuit Card Identifier number of the (U)SIM card.
#[derive(Clone, Debug, AtatResp)]
pub struct Iccid {
    pub iccid: Bytes<20>,
}

/// Respoonse to a AT+CPIN? command
///
/// +CPIN: <code>
/// It can also return +CMS ERROR: <err> if an error occurs.
#[derive(Clone, Debug, AtatResp)]
pub struct SimStatus {
    pub code: String<32>,
}

/// Firmware version identification
///
/// Returns the firmware version of the module.
#[derive(Clone, Debug, AtatResp)]
pub struct VersionInfo {
    #[at_arg(position = 0)]
    pub code: Bytes<64>,
}

/// Network Information
#[derive(Clone, Debug, AtatResp)]
pub struct NetworkInfo {
    /// Access technology
    /// String type. Access technology selected.
    /// "No Service"
    /// "GSM"
    /// "GPRS"
    /// "EDGE"
    /// "eMTC"
    /// "NBIoT"
    #[at_arg(position = 1)]
    pub act: String<32>,
    /// Operator
    /// String type. Operator name in numeric format.
    #[at_arg(position = 2)]
    pub oper: Option<String<32>>,
    /// Band
    /// String type. Band selected.
    /// "GSM 850"
    /// "GSM 900"
    /// "GSM 1800"
    /// "GSM 1900"
    /// "LTE BAND 1" – "LTE BAND 85"
    #[at_arg(position = 3)]
    pub band: Option<String<32>>,
    /// Channel
    /// Integer type. Channel selected.
    #[at_arg(position = 4)]
    pub channel: Option<u32>,
}

/// Network Registration Status (LTE-M)
/// When <n>=0, 1, or 2 and the command is executed successfully:
/// +CEREG: <n>,<stat>[,[<tac>],[<ci>],[<AcT>[,<cause_type>,<reject_cause>]]]
///
/// When <n>=4 and the command is executed successfully :
/// +CEREG: <n>,<stat>[,[<tac>],[<ci>],[<AcT>][,[<cause_type>],[<reject_cause>][,[<Active-Time>],[<Periodic-TAU>]]]]
#[derive(Clone, Debug, AtatResp)]
pub struct EPSNetworkRegistrationStatusResponse {
    /// <n>
    /// Integer type. The type of unsolicited result code presentation.
    /// 0 Disable network registration unsolicited result code
    /// 1 Enable network registration unsolicited result code: +CEREG: <stat>
    /// 2 Enable network registration and location information unsolicited result code:
    ///   +CEREG: <stat>[,[<tac>],[<ci>],[<AcT>]]
    /// 4 For a UE that has applied PSM, and network assigns T3324 to UE, enable
    /// network registration and location information unsolicited result code:
    ///   +CEREG: <stat>[,[<tac>],[<ci>],[<AcT>][,,[,[<Active-Time>],[<Periodic-TAU>]]]]
    #[at_arg(position = 1)]
    pub n: u8,
    /// <stat>
    /// Integer type. The EPS network registration status.
    /// 0: Not registered, ME is not currently searching a new operator to register to
    /// 1: Registered, home network
    /// 2: Not registered, but ME is currently searching a new operator to register to
    /// 3: Registration denied
    /// 4: Unknown (e.g. out of E-UTRAN coverage)
    /// 5: Registered, roaming
    #[at_arg(position = 2)]
    pub stat: u8,
    /// <tac>
    /// String type. Two-byte tracking area code in hexadecimal format.
    #[at_arg(position = 3)]
    pub tac: Option<String<4>>,
    /// <ci>
    /// String type. Four-byte E-UTRAN cell ID in hexadecimal format.
    #[at_arg(position = 4)]
    pub ci: Option<String<8>>,
    /// <AcT>
    /// Integer type. The access technology.
    /// 0: GSM (Not applicable)
    /// 8: eMTC
    /// 9: NB-IoT
    #[at_arg(position = 5)]
    pub act: Option<u8>,
    /// <cause_type>
    /// Integer type. The type of <reject_cause>.
    /// 0: Indicates that <reject_cause> contains an EMM cause value.
    /// 1: Indicates that <reject_cause> contains a manufacturer-specific cause.
    #[at_arg(position = 6)]
    pub cause_type: Option<u8>,
    /// <reject_cause>
    /// Integer type. Contains the cause of the failed registration. The value is of type as
    /// defined by <cause_type>.
    #[at_arg(position = 7)]
    pub reject_cause: Option<u8>,
    /// <Active-Time>
    /// String type. One byte in an 8-bit format. Active Time value (T3324) to be allocated to
    /// the UE. (e.g. "00001111" equals to 1 minute)
    /// Bits 5 to 1 represent the binary coded timer value.
    /// Bits 6 to 8 define the timer value unit as follows:
    /// Bits
    /// 8 7 6
    /// 0 0 0 value is incremented in multiples of 2 seconds
    /// 0 0 1 value is incremented in multiples of 1 minute
    /// 0 1 0 value is incremented in multiples of decihours
    /// 1 1 1 value indicates that the timer is deactivated.
    #[at_arg(position = 8)]
    pub active_time: Option<String<8>>,
    /// <Periodic-TAU>
    /// String type. One byte in an 8-bit format. Extend periodic TAU value (T3412_ext) to
    /// be allocated to the UE in E-UTRAN.
    /// (e.g. "00001010" equals to 100 minutes)
    /// Bits 5 to 1 represent the binary coded timer value.
    /// Bits 6 to 8 define the timer value unit as follows:
    /// Bits
    /// 8 7 6
    /// 0 0 0 value is incremented in multiples of 10 minutes
    /// 0 0 1 value is incremented in multiples of 1 hour
    /// 0 1 0 value is incremented in multiples of 10 hours
    /// 0 1 1 value is incremented in multiples of 2 seconds
    /// 1 0 0 value is incremented in multiples of 30 seconds
    /// 1 0 1 value is incremented in multiples of 1 minute
    #[at_arg(position = 9)]
    pub periodic_tau: Option<String<8>>,
}

/// Network Registration Status (GPRS)
///
/// When <n>=0, 1, or 2 and the command is executed successfully:
///   +CGREG: <n>,<stat>[,[<lac>],[<ci>],[<AcT>],[<rac>][,<cause_type>,<reject_cause>]]
///
/// When <n>=4 and the command is executed successfully :
///   +CGREG: <n>,<stat>[,[<lac>],[<ci>],[<AcT>],[<rac>][,[<cause_type>],[<reject_cause>][,[<Active-Time>],[<Periodic-RAU>],[<GPRS-READY-timer>]]]]
#[derive(Clone, Debug, AtatResp)]
pub struct EGPRSNetworkRegistrationStatusResponse {
    /// <n>
    /// Integer type. The type of unsolicited result code presentation.
    /// 0 Disable network registration unsolicited result code
    /// 1 Enable network registration unsolicited result code: +CGREG: <stat>
    /// 2 Enable network registration and location information unsolicited result code:
    ///   +CGREG: <stat>[,[<lac>],[<ci>],[<AcT>],[<rac>]]
    /// 4 For a UE that has applied PSM, and network assigns T3324 to UE, enable
    /// network registration and location information unsolicited result code:
    ///   +CGREG: <stat>[,[<lac>],[<ci>],[<AcT>],[<rac>][,,[,[<Active-Time>],[<Periodic-RAU>],[<GPRS-READY-timer>]]]]
    #[at_arg(position = 1)]
    pub n: u8,
    /// <stat>
    /// Integer type. The EGPRS network registration status.
    /// 0: Not registered, MT is not currently searching a new operator to register to
    /// 1: Registered, home network
    /// 2: Not registered, but MT is currently searching a new operator to register to
    /// 3: Registration denied
    /// 4: Unknown (e.g. out of GERAN coverage)
    /// 5: Registered, roaming
    #[at_arg(position = 2)]
    pub stat: u8,
    /// <lac>
    /// String type. Two-byte location area code in hexadecimal format.
    #[at_arg(position = 3)]
    pub lac: Option<String<4>>,
    /// <ci>
    /// String type. Four-byte cell ID in hexadecimal format.
    #[at_arg(position = 4)]
    pub ci: Option<String<8>>,
    /// <AcT>
    /// Integer type. The access technology.
    /// 0: GSM
    /// 8: eMTC (Not applicable)
    /// 9: NB-IoT (Not applicable)
    #[at_arg(position = 5)]
    pub act: Option<u8>,
    /// <rac>
    /// Integer type. Routing Area Code.
    #[at_arg(position = 6)]
    pub rac: Option<u8>,
    /// <cause_type>
    /// Integer type. The type of <reject_cause>.
    /// 0: Indicates that <reject_cause> contains an EMM cause value.
    /// 1: Indicates that <reject_cause> contains a manufacturer-specific cause.
    #[at_arg(position = 6)]
    pub cause_type: Option<u8>,
    /// <reject_cause>
    /// Integer type. Contains the cause of the failed registration. The value is of type as
    /// defined by <cause_type>.
    #[at_arg(position = 7)]
    pub reject_cause: Option<u8>,
    /// <Active-Time>
    /// String type. One byte in an 8-bit format. Active Time value (T3312) to be allocated to
    /// the UE. (e.g. "00001111" equals to 1 minute)
    /// Bits 5 to 1 represent the binary coded timer value.
    /// Bits 6 to 8 define the timer value unit as follows:
    /// Bits
    /// 8 7 6
    /// 0 0 0 value is incremented in multiples of 2 seconds
    /// 0 0 1 value is incremented in multiples of 1 minute
    /// 0 1 0 value is incremented in multiples of decihours
    /// 1 1 1 value indicates that the timer is deactivated.
    #[at_arg(position = 8)]
    pub active_time: Option<String<8>>,
    /// <Periodic-RAU>
    /// String type(?) Not documented in the Quectel BG95 AT Commands Manual, should be the same as <Periodic-TAU>
    #[at_arg(position = 9)]
    pub periodic_rau: Option<String<8>>,
    /// <GPRS-READY-timer>
    /// String type(?) Not documented in the Quectel BG95 AT Commands Manual
    #[at_arg(position = 10)]
    pub gprs_ready_timer: Option<String<8>>,
}

/// Signal Information
#[derive(Clone, Debug, AtatResp)]
pub struct GetSignalStrengthResponse {
    /// String type. Service mode in which the MT will unsolicitedly report the signal strength.
    pub mode: String<32>,
    /// Integer type. Received signal strength, available in GSM and LTE modes.
    pub rssi: Option<i16>,
    /// Integer type. Reference signal received power, available in LTE mode.
    pub lte_rsrp: Option<i16>,
    /// Integer type. Signal to interference plus noise ratio in in 1/5th of a dB, available in LTE mode.
    pub lte_sinr: Option<i16>,
    /// Integer type. Reference signal received quality in dB, available in LTE mode.
    pub lte_rsrq: Option<i16>,
}

/// Information of the current Packet Data Protocol Context
///
/// List of the currently activated contexts and their IP addresses:
/// +QIACT: 1,<context_state>,<context_type>[,<IP_address>]
/// [.....]
/// +QIACT: 16,<context_state>,<context_type>[,<IP_address>]]
///
/// NOTE: we will only parse the first context
#[derive(Clone, Debug, AtatResp)]
pub struct PDPContextInfo {
    /// <contextID>
    /// Integer type. The PDP context identifier.
    #[at_arg(position = 1)]
    pub context_id: u8,
    /// <context_state>
    /// Integer type. The PDP context state.
    /// 0: Deactivated
    /// 1: Activated
    #[at_arg(position = 2)]
    pub context_state: u8,
    /// <context_type>
    /// Integer type. The PDP context type.
    /// 1: IPV4
    /// 2: IPV6
    #[at_arg(position = 3)]
    pub context_type: u8,
    /// <IP_address>
    /// String type. The IP address.
    #[at_arg(position = 4)]
    pub ip_address: Option<String<64>>,
}

/// Latest Time Synchronized Through NITZ Network
#[derive(Clone, Debug, AtatResp)]
pub struct NitzTimeResponse {
    /// String type: "<time>,<dst>""
    /// Time format: String type "yy/MM/dd,hh:mm:ss±zz", where characters indicate year (two last
    /// digits), month, day, hour, minutes, seconds and time zone (indicates the difference,
    /// expressed in quarters of an hour, between the local time and GMT; range -48...+48). E.g. 6th
    /// of May 2004, 22:10:00 GMT+2 hours equals “04/05/06,22:10:00+08”.
    /// DST format: Integer type with the daylight saving time.
    #[at_arg(position = 1)]
    pub time_and_dst: String<32>,
}

/// Latest Time Synchronized Through NTP Network
#[derive(Clone, Debug, AtatResp)]
pub struct NtpTimeResponse {
    /// Error code of operation.
    pub err: u8,
    /// <time>
    /// String type. Format: "yy/MM/dd,hh:mm:ss±zz", where characters indicate year (two last
    /// digits), month, day, hour, minutes, seconds and time zone (indicates the difference,
    /// expressed in quarters of an hour, between the local time and GMT; range -48...+48). E.g. 6th
    /// of May 2004, 22:10:00 GMT+2 hours equals “04/05/06,22:10:00+08”.
    #[at_arg(position = 2)]
    pub time: String<32>,
}

#[derive(Clone, AtatResp)]
pub struct FileDataModeStarted;

/// CONNECT <read_length> response from AT+QFREAD
/// Includes a buffer to store the binary data read from the file
#[derive(Clone, Debug)]
pub struct FileReadStarted {
    /// <read_length>
    /// Integer type. The actual read length. Unit: byte.
    pub read_length: u32,
    /// Binary data read from the file (max 256 bytes per read)
    pub data: Bytes<256>,
}

// Manual AtatResp implementation since we handle parsing in the command
impl atat::AtatResp for FileReadStarted {}

#[derive(Clone, Debug, AtatResp)]
pub struct FileUploadDoneResponse {
    /// <upload_size>
    /// Integer type. The size of the uploaded file.
    #[at_arg(position = 1)]
    pub upload_size: u32,
    /// <checksum>
    /// 16 bit checksum based on bitwise XOR in hex format
    pub checksum: Bytes<4>,
}

/// File Download Done Response
/// +QFDWL: <download_size>,<checksum>
#[derive(Clone, Debug, AtatResp)]
pub struct FileDownloadDoneResponse {
    /// <download_size>
    /// Integer type. The size of the downloaded file.
    #[at_arg(position = 1)]
    pub download_size: u32,
    /// <checksum>
    /// 16 bit checksum based on bitwise XOR
    #[at_arg(position = 2)]
    pub checksum: u32,
}

/// File List Entry Response
/// +QFLST: <filename>,<file_size>
#[derive(Clone, Debug, AtatResp)]
pub struct FileListEntry {
    /// <filename>
    /// String type. Filename. The maximum length is 80 bytes.
    #[at_arg(position = 1)]
    pub filename: String<80>,
    /// <file_size>
    /// Integer type. Size of the file in bytes.
    #[at_arg(position = 2)]
    pub file_size: u32,
}

/// File List Response (wrapper for multiple entries)
/// Can contain up to 5 file entries
#[derive(Clone, Debug, AtatResp)]
pub struct FileListResponse {
    #[at_arg(position = 0)]
    pub files: atat::heapless::Vec<FileListEntry, 5>,
}

/// File Open Response
/// +QFOPEN: <filehandle>
#[derive(Clone, Debug, AtatResp)]
pub struct FileOpenResponse {
    /// <filehandle>
    /// Integer type. The handle of the file to be operated.
    #[at_arg(position = 1)]
    pub filehandle: u32,
}

/// File Write Response
/// +QFWRITE: <written_length>,<total_length>
#[derive(Clone, Debug, AtatResp)]
pub struct FileWriteResponse {
    /// <written_length>
    /// Integer type. The actual written length. Unit: byte.
    #[at_arg(position = 1)]
    pub written_length: u32,
    /// <total_length>
    /// Integer type. The total length of the file. Unit: byte.
    #[at_arg(position = 2)]
    pub total_length: u32,
}

/// MQTT Open Response
#[derive(Clone, Debug, AtatResp)]
pub struct MqttOpenResponse {
    /// <tcpconnectID>
    /// Integer type. The MQTT socket identifier from 0 to 5.
    #[at_arg(position = 1)]
    pub tcpconnect_id: u8,
    /// <result>
    /// Integer type. The result of the operation.
    /// -1 Failed to open network
    /// 0 Opened network successfully
    /// 1 Wrong parameter
    /// 2 MQTT identifier is occupied
    /// 3 Failed to activate PDP
    /// 4 Failed to parse domain name
    /// 5 Network disconnection error
    #[at_arg(position = 2)]
    pub result: i8,
}

/// URC +QMTSTAT response
#[derive(Clone, Debug, AtatResp)]
pub struct MqttStatusResponse {
    /// <tcpconnectID>
    /// Integer type. The MQTT socket identifier from 0 to 5.
    #[at_arg(position = 1)]
    pub tcpconnect_id: u8,
    /// <err>
    /// Integer type. The status of the MQTT connection.
    /// 1: Connection is closed or reset by the peer.
    /// 2: Sending PINGREQ packet timed or failed.
    /// 3: Sending CONNECT packet timed out or failed.
    /// 4: Received CONNACK packet timed out or failed.
    /// 5: Client sends DISCONNECT packet but server is initiative to close MQTT.
    /// 6: Client is initiative to close MQTT connection due to packet sending failure all the time.
    /// 7: The link is not alive or the server is unavailable.
    #[at_arg(position = 2)]
    pub err: u8,
}

/// URC +QMTCONN response
#[derive(Clone, Debug, AtatResp)]
pub struct MqttConnectResponse {
    /// <tcpconnectID>
    /// Integer type. The MQTT socket identifier from 0 to 5.
    #[at_arg(position = 1)]
    pub tcpconnect_id: u8,
    /// <result>
    /// Integer type. The result of the operation.
    /// 0: Sent CONNECT packet successfully.
    /// 1: Packet retrasnmission.
    /// 2: Failed to send CONNECT packet.
    #[at_arg(position = 2)]
    pub result: u8,
    /// <ret_code>
    /// Integer type. The return code of the CONNACK packet.
    /// 0: Connection accepted
    /// 1: Connection refused, unacceptable protocol version
    /// 2: Connection refused, identifier rejected
    /// 3: Connection refused, server unavailable
    /// 4: Connection refused, bad user name or password
    /// 5: Connection refused, not authorized
    #[at_arg(position = 3)]
    pub ret_code: u8,
}

/// URC +QMTPUB response
#[derive(Clone, Debug, AtatResp)]
pub struct MqttPublishResponse {
    /// <tcpconnectID>
    /// Integer type. The MQTT socket identifier from 0 to 5.
    #[at_arg(position = 1)]
    pub tcpconnect_id: u8,
    /// <messageID>
    /// Integer type. The message identifier.
    #[at_arg(position = 2)]
    pub message_id: u16,
    /// <result>
    /// Integer type. The result of the operation.
    /// 0: Sent PUBLISH packet successfully.
    /// 1: Packet retrasnmission.
    /// 2: Failed to send PUBLISH packet.
    #[at_arg(position = 3)]
    pub result: u8,
    /// <value>
    /// Integer type.
    /// If result is 1, the value is the number of retransmissions.
    /// If 0 or 2, the value is not present.
    #[at_arg(position = 4)]
    pub value: Option<u8>,
}

/// URC +QMTDISC response
#[derive(Clone, Debug, AtatResp)]
pub struct MqttDisconnectResponse {
    /// <tcpconnectID>
    /// Integer type. The MQTT socket identifier from 0 to 5.
    #[at_arg(position = 1)]
    pub tcpconnect_id: u8,
    /// <result>
    /// Integer type. The result of the operation.
    /// -1: Failed to close network
    /// 0: Closed network successfully
    #[at_arg(position = 2)]
    pub result: i8,
}

/// URC +QMTCLOSE response
#[derive(Clone, Debug, AtatResp)]
pub struct MqttCloseResponse {
    /// <tcpconnectID>
    /// Integer type. The MQTT socket identifier from 0 to 5.
    #[at_arg(position = 1)]
    pub tcpconnect_id: u8,
    /// <result>
    /// Integer type. The result of the operation.
    /// -1: Failed to close network
    /// 0: Closed network successfully
    #[at_arg(position = 2)]
    pub result: i8,
}

/// URC +CME ERROR response
///
/// Indicates an error related to mobile equipment or network.
/// +CME ERROR: <err>
/// There are many possible errors, the most common are:
/// 3: Operation not allowed
/// 10: SIM not inserted
/// 11: SIM PIN required
/// 12: SIM PUK required
/// 13: SIM failure
/// 14: SIM busy
/// 15: SIM wrong
/// 16: Incorrect password
#[derive(Clone, Debug, AtatResp)]
pub struct CmeError {
    /// <err>
    /// Integer type. The error code.
    #[at_arg(position = 1)]
    pub err: u8,
}

/// Response for the AT+QGPSLOC=2 command
#[derive(Debug, AtatResp)]
pub struct GnssPositionInformationResponse {
    /// <UTC>
    /// String type. UTC time.
    /// Format: hhmmss.sss
    #[at_arg(position = 1)]
    pub utc: Bytes<10>,
    /// <latitude>
    /// Float type. Latitude position.
    #[at_arg(position = 2)]
    pub latitude: f32,
    /// <longitude>
    /// Float type. Longitude position.
    #[at_arg(position = 3)]
    pub longitude: f32,
    /// <HDOP>
    /// Float type. Horizontal precision.
    #[at_arg(position = 4)]
    pub hdop: f32,
    /// <altitude>
    /// Float type. Antenna altitude from sea level.
    #[at_arg(position = 5)]
    pub altitude: f32,
    /// <fix>
    /// Integer type. GNSS positioning mode (2D/3D)
    #[at_arg(position = 6)]
    pub fix: u8,
    /// <COG>
    /// String type. Course over ground based on true north
    /// Format: ddd.mm
    #[at_arg(position = 7)]
    pub cog: Bytes<6>,
    /// <spkm>
    /// Float type. Speed over ground (km/h)
    #[at_arg(position = 8)]
    pub spkm: f32,
    /// <spkn>
    /// Float type. Speed over ground (knots)
    #[at_arg(position = 9)]
    pub spkn: f32,
    /// <date>
    /// String type. UTC time after fixing position
    /// Format: ddmmyy
    #[at_arg(position = 10)]
    pub date: Bytes<6>,
    /// <nsat>
    /// Number of satellites
    #[at_arg(position = 11)]
    pub nsat: Bytes<2>,
}

/// Response for the AT+QGPSGNMEA="GGA" command.
/// It uses the GGA NMEA sentence format
#[derive(Debug, AtatResp)]
pub struct GnssGgaNmeaSentenceResponse {
    #[at_arg(position = 1)]
    _header: Bytes<6>,
    #[at_arg(position = 2)]
    pub utc: Option<f32>,
    #[at_arg(position = 3)]
    pub lat: Option<f64>,
    #[at_arg(position = 4)]
    pub lat_dir: Option<char>,
    #[at_arg(position = 5)]
    pub lon: Option<f64>,
    #[at_arg(position = 6)]
    pub lon_di: Option<char>,
    #[at_arg(position = 7)]
    pub quality: u8,
    #[at_arg(position = 8)]
    pub satellites: Option<Bytes<2>>,
    #[at_arg(position = 9)]
    pub hdop: Option<f32>,
    #[at_arg(position = 10)]
    pub alt: Option<f32>,
    #[at_arg(position = 11)]
    pub alt_units: Option<char>,
    #[at_arg(position = 12)]
    pub undulation: Option<f32>,
    #[at_arg(position = 13)]
    pub undulation_units: Option<char>,
    #[at_arg(position = 14)]
    _age: Option<u16>,
    #[at_arg(position = 15)]
    _checksum: Option<Bytes<3>>,
}

// ---------------------------------------------------------------------------
// TCP / SSL socket responses (AT+QSSLOPEN / QSSLRECV / QSSLURC)
// ---------------------------------------------------------------------------

/// `+QSSLOPEN: <clientID>,<err>` URC emitted after `AT+QSSLOPEN`.
///
/// `err == 0` means the (TLS) connection opened successfully; any other value
/// is a Quectel error code (network/DNS/TLS handshake failure, etc.).
#[derive(Clone, Debug, AtatResp)]
pub struct SslOpenResponse {
    /// <clientID> — socket identifier (0-11).
    #[at_arg(position = 1)]
    pub client_id: u8,
    /// <err> — 0 on success, otherwise a Quectel error code.
    #[at_arg(position = 2)]
    pub err: i32,
}

/// `+QSSLURC: <type>,<clientID>` unsolicited socket event.
///
/// `type` is a quoted keyword: `"recv"` (data available to read),
/// `"closed"` (peer closed the connection), etc.
#[derive(Clone, Debug, AtatResp)]
pub struct SslUrcResponse {
    /// <type> — event keyword, e.g. "recv" or "closed".
    #[at_arg(position = 1)]
    pub urc_type: String<16>,
    /// <clientID> — socket identifier the event refers to.
    #[at_arg(position = 2)]
    pub client_id: u8,
}

/// Data returned by `AT+QSSLRECV` in buffer access mode.
///
/// Filled by a custom parser (see `SslRecv::parse`) from the
/// `+QSSLRECV: <len>\r\n<binary>` response. `length` is the number of valid
/// bytes in `data`; `0` means no data was currently buffered.
#[derive(Clone, Debug, AtatResp)]
pub struct SslRecvResponse {
    /// Number of valid bytes in `data`.
    #[at_arg(position = 1)]
    pub length: u16,
    /// The received bytes (up to the requested read length, max 512).
    #[at_arg(position = 2)]
    pub data: Bytes<512>,
}
