use futures::Future;
use std::net::SocketAddr;

use connection::{AcquireConnection, Connection};
use Error;

#[derive(Debug)]
pub struct ConnectionPoolBuilder;

#[derive(Debug)]
pub struct ConnectionPool;

#[derive(Debug, Clone)]
pub struct ConnectionPoolHandle;
impl AcquireConnection for ConnectionPoolHandle {
    type Connection = Connection;
    type Future = Box<Future<Item = Self::Connection, Error = Error> + Send + 'static>;

    fn acquire_connection(&mut self, addr: SocketAddr) -> Self::Future {
        unimplemented!()
    }
}
