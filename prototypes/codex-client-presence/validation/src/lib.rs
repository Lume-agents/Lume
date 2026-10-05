#[cfg(all(test, unix))]
#[path = "../producer/client_presence.rs"]
pub(crate) mod client_presence;

pub mod presence_reader;

#[cfg(all(test, unix))]
#[path = "../tests/lifecycle.rs"]
mod lifecycle;
