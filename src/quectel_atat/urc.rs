use atat::atat_derive::AtatUrc;

use crate::quectel_atat::responses::*;

#[derive(Clone, AtatUrc, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Urc {
    #[at_urc("APP RDY")]
    Ready,
    #[at_urc("RDY")]
    AppReady,
    #[at_urc("+UMWI")]
    MessageWaitingIndication(MessageWaitingIndication),

    /// File Manager Data Mode Started
    #[at_urc("CONNECT")]
    FileDataModeStarted,

    /// File Manager Upload Done
    /// +QFUPL: <upload_size>,<checksum>
    #[at_urc("+QFUPL")]
    FileUploadDone(FileUploadDoneResponse),

    /// File Manager Write Done
    /// +QFWRITE: <written_length>,<total_length>
    #[at_urc("+QFWRITE")]
    FileWriteDone(FileWriteResponse),

    #[at_urc("+QNTP")]
    NtpTime(NtpTimeResponse),

    /// MQTT open URC
    /// +QMTOPEN: <link_id>,<result> where <link_id> is the link identifier and <result> is the result of the MQTT Open operation.
    #[at_urc("+QMTOPEN")]
    MqttOpen(MqttOpenResponse),

    /// MQTT status URC
    /// +QMTSTAT: <link_id>,<status> where <link_id> is the link identifier and <status> is the status of the MQTT connection.
    #[at_urc("+QMTSTAT")]
    MqttStatus(MqttStatusResponse),

    /// MQTT connection URC
    /// +QMTCONN: <tcpconnectID>,<result>[,<ret_code>]
    #[at_urc("+QMTCONN")]
    MqttConnect(MqttConnectResponse),

    /// MQTT publish URC
    /// +QMTPUB: <tcpconnectID>,<messageID>,<result>[,<value>]
    #[at_urc("+QMTPUB")]
    MqttPublish(MqttPublishResponse),

    /// MQTT Disconnection URC
    /// +QMTDISC: <tcpconnectID>,<result>
    #[at_urc("+QMTDISC")]
    MqttDisconnect(MqttDisconnectResponse),

    /// MQTT Close URC
    /// +QMTCLOSE: <tcpconnectID>,<result>
    #[at_urc("+QMTCLOSE")]
    MqttClose(MqttCloseResponse),

    /// SSL socket open result URC
    /// +QSSLOPEN: <clientID>,<err>
    #[at_urc("+QSSLOPEN")]
    SslOpen(SslOpenResponse),

    /// SSL socket unsolicited event URC (e.g. data received or peer closed)
    /// +QSSLURC: "<type>",<clientID>
    #[at_urc("+QSSLURC")]
    SslUrc(SslUrcResponse),

    /// Plain-TCP socket open result URC
    /// +QIOPEN: <connectID>,<err>
    #[at_urc("+QIOPEN")]
    TcpOpen(TcpOpenResponse),

    /// Plain-TCP socket unsolicited event URC (data received or peer closed)
    /// +QIURC: "<type>",<connectID>
    #[at_urc("+QIURC")]
    TcpUrc(TcpUrcResponse),

    /// Power Down URC
    /// +QPOWD: POWERED DOWN
    #[at_urc("POWERED DOWN")]
    PowerDown,

    /// Final result code URC
    /// indicates an error related to mobile equipment or network.
    /// +CME ERROR: <err>
    ///
    /// Between other uses, the "no SIM URC" message is returned as a CME error when
    /// the user sends a AT+CPIN? and no SIM is inserted
    #[at_urc("+CME ERROR")]
    CmeError(CmeError),
}
