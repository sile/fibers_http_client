//! A tiny asynchronous HTTP/1.1 client library.
//!
//! # Examples
//!
//! ```no_run
//! # extern crate fibers_global;
//! # extern crate fibers_http_client;
//! # extern crate url;
//! use fibers_http_client::connection::Oneshot;
//! use fibers_http_client::Client;
//! use url::Url;
//!
//! # fn main() {
//! let url = Url::parse("http://localhost/foo/bar").unwrap();
//! let mut client = Client::new(Oneshot);
//! let future = client.request(&url).get();
//!
//! let response = fibers_global::execute(future).unwrap();
//! println!("STATUS: {:?}", response.status_code());
//! println!("BODY: {:?}", response.body());
//! # }
//! ```
#![warn(missing_docs)]
extern crate bytecodec;
extern crate fibers;
extern crate futures;
extern crate httpcodec;
extern crate prometrics;
#[macro_use]
extern crate trackable;
extern crate url;

pub use client::Client;
pub use error::{Error, ErrorKind};
pub use request::RequestBuilder;

mod client;
mod connection_pool;
mod error;
mod request;

pub mod connection;
pub mod metrics;

/// This crate specific `Result` type.
pub type Result<T> = std::result::Result<T, Error>;
