//! Bounded I/O shared by the Node listener, paired client and loopback runtimes.

use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

use serde_json::Value;

const IDLE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_HEADER_BYTES: usize = 16 * 1024;

pub(crate) struct DeadlineStream {
    stream: TcpStream,
    deadline: Instant,
}

impl DeadlineStream {
    pub(crate) fn new(stream: TcpStream, deadline: Instant) -> Self {
        Self { stream, deadline }
    }

    fn timeout(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .map(|remaining| remaining.min(IDLE_TIMEOUT))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "Node request deadline exceeded")
            })
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.timeout()?))?;
        self.stream.read(buffer)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.timeout()?))?;
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.set_write_timeout(Some(self.timeout()?))?;
        self.stream.flush()
    }
}

pub(crate) fn read_json_response(
    stream: &mut impl Read,
    maximum_bytes: usize,
) -> Result<Value, String> {
    let mut response = Vec::new();
    stream
        .take((maximum_bytes + 1) as u64)
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    if response.len() > maximum_bytes {
        return Err("Node response is too large".into());
    }
    parse_json_response(&response)
}

pub(crate) fn parse_json_response(response: &[u8]) -> Result<Value, String> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .filter(|position| *position <= MAX_HEADER_BYTES)
        .map(|position| position + 4)
        .ok_or_else(|| "invalid Node HTTP response".to_string())?;
    let headers = std::str::from_utf8(&response[..header_end])
        .map_err(|_| "invalid Node HTTP response".to_string())?;
    let mut lines = headers.lines();
    let status_line = lines.next().ok_or("missing HTTP status")?;
    let mut status_fields = status_line.split_whitespace();
    if !matches!(status_fields.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
        return Err("invalid Node HTTP version".into());
    }
    let status = status_fields
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "invalid Node HTTP status".to_string())?;
    let mut content_length = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or("invalid HTTP header")?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            // Node responses have a length; loopback probes request HTTP/1.0.
            return Err("unsupported HTTP transfer encoding".into());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err("duplicate HTTP content length".into());
            }
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "invalid HTTP content length")?,
            );
        }
    }
    let payload = &response[header_end..];
    if content_length.is_some_and(|length| length != payload.len()) {
        return Err("incomplete Node HTTP response".into());
    }
    let body = serde_json::from_slice::<Value>(payload)
        .map_err(|_| "invalid Node JSON response".to_string())?;
    if !(200..300).contains(&status) {
        let message = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Node request failed");
        let message = message.chars().take(160).collect::<String>();
        return Err(format!("{message} (HTTP {status})"));
    }
    Ok(body)
}
