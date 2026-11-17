//! Limits for the small HTTP servers that read requests by hand (the mobile gateway, the web companion):
//! a whole request has a deadline, a header line and the header block have a size, and a listener holds
//! a bounded number of connections, in all and per address.

use std::{
    collections::HashMap,
    io::{self, BufRead, Read},
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// The most a request line or the headers together may take.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
/// The most headers a request may send.
pub const MAX_HEADERS: usize = 64;

/// Refuses to read once the request's deadline has passed, so a client that sends a byte now and then
/// cannot hold a connection: each read also waits at most the socket's own read timeout.
pub struct DeadlineReader<R> {
    inner: R,
    deadline: Instant,
}

impl<R> DeadlineReader<R> {
    pub fn new(inner: R, within: Duration) -> Self {
        Self { inner, deadline: Instant::now() + within }
    }
}

impl<R: Read> Read for DeadlineReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "request took too long"));
        }
        self.inner.read(buffer)
    }
}

/// One line, at most `limit` bytes (the `\n` included); a longer one is an error, not a growing buffer.
pub fn read_line_limited(reader: &mut impl BufRead, limit: usize) -> Result<String, String> {
    let mut line = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_until(b'\n', &mut line)
        .map_err(|error| error.to_string())?;
    if line.len() > limit {
        return Err("Request header is too large".into());
    }
    String::from_utf8(line).map_err(|_| "Request header is not text".to_string())
}

/// The request line and the header lines (without the blank line that ends them), within
/// `MAX_HEADER_BYTES` and `MAX_HEADERS` together.
pub fn read_head(reader: &mut impl BufRead) -> Result<(String, Vec<String>), String> {
    let request_line = read_line_limited(reader, MAX_HEADER_BYTES)?;
    let mut used = request_line.len();
    let mut headers = Vec::new();
    loop {
        let line = read_line_limited(reader, MAX_HEADER_BYTES - used.min(MAX_HEADER_BYTES))?;
        used += line.len();
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS || used > MAX_HEADER_BYTES {
            return Err("Request header is too large".into());
        }
        headers.push(line);
    }
    Ok((request_line, headers))
}

/// Counts a listener's open connections; a connection holds its place until its guard drops.
#[derive(Clone)]
pub struct ConnectionLimit {
    total: usize,
    per_address: usize,
    open: Arc<Mutex<HashMap<IpAddr, usize>>>,
}

pub struct ConnectionGuard {
    address: IpAddr,
    open: Arc<Mutex<HashMap<IpAddr, usize>>>,
}

impl ConnectionLimit {
    pub fn new(total: usize, per_address: usize) -> Self {
        Self { total, per_address, open: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// A place for a connection from `address`, or None when the listener or that address is full.
    pub fn admit(&self, address: IpAddr) -> Option<ConnectionGuard> {
        let mut open = self.open.lock().ok()?;
        let total: usize = open.values().sum();
        let from_address = open.get(&address).copied().unwrap_or_default();
        if total >= self.total || from_address >= self.per_address {
            return None;
        }
        open.insert(address, from_address + 1);
        Some(ConnectionGuard { address, open: self.open.clone() })
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if let Ok(mut open) = self.open.lock() {
            if let Some(count) = open.get_mut(&self.address) {
                *count -= 1;
                if *count == 0 {
                    open.remove(&self.address);
                }
            }
        }
    }
}

/// A few places for large bodies read before the sender is known, so many of them at once cannot
/// fill the memory. Taken only for a body over the small limit.
#[derive(Clone)]
pub struct LargeBodies {
    limit: usize,
    open: Arc<Mutex<usize>>,
}

pub struct LargeBodyGuard {
    open: Arc<Mutex<usize>>,
}

impl LargeBodies {
    pub fn new(limit: usize) -> Self {
        Self { limit, open: Arc::new(Mutex::new(0)) }
    }

    pub fn admit(&self) -> Option<LargeBodyGuard> {
        let mut open = self.open.lock().ok()?;
        if *open >= self.limit {
            return None;
        }
        *open += 1;
        Some(LargeBodyGuard { open: self.open.clone() })
    }
}

impl Drop for LargeBodyGuard {
    fn drop(&mut self) {
        if let Ok(mut open) = self.open.lock() {
            *open = open.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[test]
    fn a_header_without_an_end_is_refused_before_it_grows() {
        let endless = Cursor::new(vec![b'a'; MAX_HEADER_BYTES * 4]);
        assert!(read_head(&mut BufReader::new(endless)).is_err());
        let many = format!("GET / HTTP/1.1\r\n{}\r\n", "X: y\r\n".repeat(MAX_HEADERS + 1));
        assert!(read_head(&mut BufReader::new(Cursor::new(many))).is_err());
        let (line, headers) = read_head(&mut BufReader::new(Cursor::new("GET / HTTP/1.1\r\nA: b\r\n\r\nbody"))).unwrap();
        assert_eq!((line.as_str(), headers.len()), ("GET / HTTP/1.1\r\n", 1));
    }

    #[test]
    fn a_reader_past_its_deadline_stops() {
        let mut reader = DeadlineReader::new(Cursor::new(vec![1u8; 8]), Duration::ZERO);
        assert_eq!(reader.read(&mut [0; 4]).unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn connections_are_counted_in_all_and_per_address() {
        let limit = ConnectionLimit::new(3, 2);
        let a: IpAddr = "10.0.0.2".parse().unwrap();
        let b: IpAddr = "10.0.0.3".parse().unwrap();
        let first = limit.admit(a).unwrap();
        let _second = limit.admit(a).unwrap();
        assert!(limit.admit(a).is_none(), "per address");
        let _third = limit.admit(b).unwrap();
        assert!(limit.admit(b).is_none(), "in all");
        drop(first);
        assert!(limit.admit(a).is_some(), "a closed connection frees its place");

        let bodies = LargeBodies::new(1);
        let held = bodies.admit().unwrap();
        assert!(bodies.admit().is_none());
        drop(held);
        assert!(bodies.admit().is_some());
    }
}
