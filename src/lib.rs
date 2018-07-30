//! A tiny asynchronous HTTP/1.1 client library.
//!
//! # Examples
//!
//! ```no_run
//! # extern crate fibers;
//! # extern crate fibers_http_client;
//! # extern crate futures;
//! # extern crate url;
//! use fibers::{Executor, InPlaceExecutor, Spawn};
//! use fibers_http_client::connection::Oneshot;
//! use fibers_http_client::Client;
//! use url::Url;
//!
//! # fn main() {
//! let url = Url::parse("http://localhost/foo/bar").unwrap();
//! let mut client = Client::new(Oneshot);
//! let future = client.request(&url).get();
//!
//! let mut executor = InPlaceExecutor::new().unwrap();
//! let monitor = executor.spawn_monitor(future);
//! let response = executor.run_fiber(monitor).unwrap().unwrap();
//! println!("STATUS: {:?}", response.status_code());
//! println!("BODY: {:?}", response.body());
//! # }
//! ```
#![warn(missing_docs)]
extern crate bytecodec;
extern crate fibers;
#[cfg(test)]
extern crate fibers_global;
#[cfg(test)]
extern crate fibers_http_server;
extern crate futures;
extern crate httpcodec;
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

/// This crate specific `Result` type.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod test {
    use bytecodec::bytes::Utf8Encoder;
    use bytecodec::null::NullDecoder;
    use fibers::{Executor, InPlaceExecutor, Spawn};
    use fibers_http_server::{HandleRequest, Reply, Req, Res, ServerBuilder, Status};
    use futures::future::{ok, Future};
    use httpcodec::{BodyDecoder, BodyEncoder};
    use url::Url;

    use super::*;
    use connection::ConnectionPool;

    struct Hello;
    impl HandleRequest for Hello {
        const METHOD: &'static str = "GET";
        const PATH: &'static str = "/hello";

        type ReqBody = ();
        type ResBody = String;
        type Decoder = BodyDecoder<NullDecoder>;
        type Encoder = BodyEncoder<Utf8Encoder>;
        type Reply = Reply<Self::ResBody>;

        fn handle_request(&self, _req: Req<Self::ReqBody>) -> Self::Reply {
            Box::new(ok(Res::new(Status::Ok, "hello".to_owned())))
        }
    }

    #[test]
    fn oneshot_connection_works() {
        let addr = "127.0.0.1:14757".parse().unwrap();

        // server
        let mut builder = ServerBuilder::new(addr);
        builder.add_handler(Hello).unwrap();
        let server = builder.finish(fibers_global::handle());
        fibers_global::spawn(server.map_err(|e| panic!("{}", e)));

        // client: GET => 200
        let url = Url::parse(&format!("http://{}/hello", addr)).unwrap();
        let mut client = Client::new(connection::Oneshot);
        let future = client.request(&url).get();
        let response = fibers_global::execute(future).unwrap();
        assert_eq!(response.status_code().as_u16(), 200);
        assert_eq!(response.body(), b"hello");

        // client: DELETE => 405
        let url = Url::parse(&format!("http://{}/hello", addr)).unwrap();
        let mut client = Client::new(connection::Oneshot);
        let future = client.request(&url).delete();
        let response = fibers_global::execute(future).unwrap();
        assert_eq!(response.status_code().as_u16(), 405);

        // client: PUT => 404
        let url = Url::parse(&format!("http://{}/world", addr)).unwrap();
        let mut client = Client::new(connection::Oneshot);
        let future = client.request(&url).put(vec![1, 2, 3]);
        let response = fibers_global::execute(future).unwrap();
        assert_eq!(response.status_code().as_u16(), 404);
    }

    #[test]
    fn connection_pool_works() {
        let addr = "127.0.0.1:14758".parse().unwrap();

        // server
        let mut builder = ServerBuilder::new(addr);
        builder.add_handler(Hello).unwrap();
        let server = builder.finish(fibers_global::handle());
        fibers_global::spawn(server.map_err(|e| panic!("{}", e)));

        // connection pool
        let pool = ConnectionPool::new(fibers_global::handle());
        let pool_handle = pool.handle();
        fibers_global::spawn(pool.map_err(|e| panic!("{}", e)));

        // client: GET => 200
        let url = Url::parse(&format!("http://{}/hello", addr)).unwrap();
        let mut client = Client::new(pool_handle);
        let future = client.request(&url).get();
        let response = fibers_global::execute(future).unwrap();
        assert_eq!(response.status_code().as_u16(), 200);
        assert_eq!(response.body(), b"hello");

        // client: DELETE => 405
        let url = Url::parse(&format!("http://{}/hello", addr)).unwrap();
        let mut client = Client::new(connection::Oneshot);
        let future = client.request(&url).delete();
        let response = fibers_global::execute(future).unwrap();
        assert_eq!(response.status_code().as_u16(), 405);

        // client: PUT => 404
        let url = Url::parse(&format!("http://{}/world", addr)).unwrap();
        let mut client = Client::new(connection::Oneshot);
        let future = client.request(&url).put(vec![1, 2, 3]);
        let response = fibers_global::execute(future).unwrap();
        assert_eq!(response.status_code().as_u16(), 404);
    }
}
