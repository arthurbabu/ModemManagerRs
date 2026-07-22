//! atat custom-success digester hooks for the binary socket-read responses.
//!
//! Chip-agnostic: `AT+QSSLRECV`/`AT+QIRD` framing is identical across
//! BG95/BG96/EG916U, so unlike [`crate::chip`] these free functions have no
//! per-chip variant.

/// Custom-success digester hook for the binary `+QSSLRECV` data frame.
///
/// atat's `DefaultDigester` is line/token oriented: it frames a response by
/// scanning for `\r\nOK\r\n`, a `>`/`@` prompt, or an error token. That works
/// for ordinary AT replies, but the `AT+QSSLRECV` response is
/// `+QSSLRECV: <len>\r\n<len raw bytes>\r\n\r\nOK\r\n` where `<len raw bytes>`
/// is **arbitrary binary** (HTML, JS, TLS-decrypted payload…). While such a
/// frame is still streaming into the ingress buffer (before its trailing
/// `\r\nOK\r\n` has arrived) the default digester's `take_until("\r\nOK\r\n")`
/// fails as a hard no-match and control falls through to the generic prompt
/// parser, which treats a lone `>` (extremely common in HTML/JS) followed by
/// end-of-buffer as a data prompt. That mis-consumes bytes and corrupts the
/// frame, surfacing as `QSSLRECV failed: InvalidResponse`.
///
/// This hook is tried *before* the generic success/prompt/error parsers (see
/// `AtDigester::digest`). It recognises the length prefix and consumes exactly
/// `<len>` payload bytes plus the terminator, returning [`ParseError::Incomplete`]
/// (so the digester waits for more bytes instead of running the fragile generic
/// parsers) until the whole frame is buffered. For any response that is not a
/// `+QSSLRECV` data frame it returns [`ParseError::NoMatch`], leaving every
/// other command on the default parsers.
///
/// Wire it into the ingress digester in place of a bare `DefaultDigester`:
/// ```ignore
/// let digester = DefaultDigester::<Urc>::default()
///     .with_custom_success(ssl_recv_digest_hook);
/// let ingress = Ingress::new(digester, buf, &RES_SLOT, &URC_CHANNEL);
/// ```
///
/// If you use both plain TCP (`AT+QIRD`) and TLS (`AT+QSSLRECV`) sockets, use
/// [`socket_recv_digest_hook`] instead — it handles both frames.
pub fn ssl_recv_digest_hook(buf: &[u8]) -> Result<(&[u8], usize), atat::digest::ParseError> {
    recv_digest_frame(buf, b"+QSSLRECV: ")
}

/// Custom-success digester hook for the binary `+QIRD` (plain TCP) data frame.
///
/// The plain-TCP counterpart of [`ssl_recv_digest_hook`]; the `AT+QIRD` response
/// is `+QIRD: <len>\r\n<len raw bytes>\r\n\r\nOK\r\n` and suffers the exact same
/// mis-framing under atat's default digester. See [`ssl_recv_digest_hook`] for
/// the full rationale.
pub fn tcp_recv_digest_hook(buf: &[u8]) -> Result<(&[u8], usize), atat::digest::ParseError> {
    recv_digest_frame(buf, b"+QIRD: ")
}

/// Combined custom-success digester hook for **both** `+QSSLRECV` (TLS) and
/// `+QIRD` (plain TCP) binary data frames.
///
/// Use this when a single ingress carries both socket types. It tries the TLS
/// frame first and falls back to the plain-TCP frame; any response that is
/// neither defers to atat's default parsers.
pub fn socket_recv_digest_hook(buf: &[u8]) -> Result<(&[u8], usize), atat::digest::ParseError> {
    use atat::digest::ParseError;
    match ssl_recv_digest_hook(buf) {
        Err(ParseError::NoMatch) => tcp_recv_digest_hook(buf),
        other => other,
    }
}

/// Shared frame extractor for the length-prefixed socket read responses.
///
/// `header` selects the command family (`b"+QSSLRECV: "` or `b"+QIRD: "`). See
/// [`ssl_recv_digest_hook`] for why this is needed and how it interacts with the
/// digester's parser ordering.
fn recv_digest_frame<'a>(
    buf: &'a [u8],
    header: &[u8],
) -> Result<(&'a [u8], usize), atat::digest::ParseError> {
    use atat::digest::ParseError;

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || hay.len() < needle.len() {
            return None;
        }
        hay.windows(needle.len()).position(|w| w == needle)
    }

    // Locate the header. Note the trailing colon+space does not match the
    // command echo (`AT+QSSLRECV=…` / `AT+QIRD=…` use `=`), so a stray echo is
    // ignored. If the header is absent this is not one of our data frames —
    // defer to the default parsers.
    let hdr = find(buf, header).ok_or(ParseError::NoMatch)?;
    let after_hdr = &buf[hdr + header.len()..];

    // Decimal length runs up to the first CRLF. Header seen but length not yet
    // terminated => wait for more.
    let nl = find(after_hdr, b"\r\n").ok_or(ParseError::Incomplete)?;
    let len = core::str::from_utf8(&after_hdr[..nl])
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .ok_or(ParseError::NoMatch)?;

    // Payload starts right after that CRLF and is exactly `len` raw bytes.
    let payload_start = hdr + header.len() + nl + 2;
    let payload_end = payload_start + len;
    if buf.len() < payload_end {
        return Err(ParseError::Incomplete);
    }

    // The terminating `OK\r\n` follows the payload (after an intervening CRLF).
    // Search only *after* the binary payload so payload bytes can't spoof it.
    let ok = find(&buf[payload_end..], b"OK\r\n").ok_or(ParseError::Incomplete)?;
    let consumed = payload_end + ok + b"OK\r\n".len();

    // Hand the command's `parse` the header + length + payload; it re-locates the
    // header itself, so a leading CRLF is harmless.
    Ok((&buf[hdr..payload_end], consumed))
}
