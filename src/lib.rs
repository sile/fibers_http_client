extern crate bytecodec;
extern crate fibers;
extern crate futures;
extern crate httpcodec;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate trackable;
extern crate url;

pub use client::Client;
pub use connection_pool::{ConnectionPool, ConnectionPoolBuilder, ConnectionPoolHandle};
pub use error::{Error, ErrorKind};
pub use request::RequestBuilder;

mod client;
mod connection;
mod connection_pool;
mod error;
mod request;

/// This crate specific `Result` type.
pub type Result<T> = std::result::Result<T, Error>;
