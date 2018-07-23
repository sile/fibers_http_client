extern crate bytecodec;
extern crate fibers;
extern crate futures;
extern crate httpcodec;
extern crate slog;
#[macro_use]
extern crate trackable;
extern crate url;

pub use client::Client;
pub use connection::{Connection, ConnectionPool, OneshotConnectionPool};
pub use error::{Error, ErrorKind};
pub use request::RequestBuilder;

mod client;
mod connection;
mod error;
mod request;

/// This crate specific `Result` type.
pub type Result<T> = std::result::Result<T, Error>;

pub type BoxFuture<T> = Box<futures::Future<Item = T, Error = Error> + Send + 'static>;
