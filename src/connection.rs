//! TCP connection.
use bytecodec::io::BufferedIo;
use fibers::net::TcpStream;
use futures::Future;
use std::net::SocketAddr;

use Error;

pub use connection_pool::{
    ConnectionPool, ConnectionPoolBuilder, ConnectionPoolHandle, RentedConnection,
};

const BUF_SIZE: usize = 4096; // FIXME: parameterize

/// This trait allows for acquiring TCP connections.
pub trait AcquireConnection {
    /// TCP connection.
    type Connection: AsMut<Connection>;

    /// `Future` for acquiring a connection to communicate with the specified TCP server.
    type Future: Future<Item = Self::Connection, Error = Error>;

    /// Returns a `Future` for acquiring a connection to communicate with the specified TCP server.
    fn acquire_connection(&mut self, addr: SocketAddr) -> Self::Future;
}

/// An implementation of [`AcquireConnection`] that always establishes new TCP connection
/// when `acqurie_connection` method called.
///
/// [`AcquireConnection`]: ./trait.AcquireConnection.html
#[derive(Debug, Default, Clone)]
pub struct Oneshot;
impl AcquireConnection for Oneshot {
    type Connection = Connection;
    type Future = Box<dyn Future<Item = Connection, Error = Error> + Send + 'static>;

    fn acquire_connection(&mut self, addr: SocketAddr) -> Self::Future {
        let future = TcpStream::connect(addr)
            .map_err(move |e| track!(Error::from(e); addr))
            .map(move |stream| Connection::new(addr, stream));
        Box::new(future)
    }
}

/// TCP connection.
#[derive(Debug)]
pub struct Connection {
    stream: BufferedIo<TcpStream>,
    peer_addr: SocketAddr,
    state: ConnectionState,
}
impl Connection {
    /// Makes a new `Connection` instance.
    pub fn new(peer_addr: SocketAddr, stream: TcpStream) -> Self {
        let _ = stream.set_nodelay(true);
        Connection {
            peer_addr,
            stream: BufferedIo::new(stream, BUF_SIZE, BUF_SIZE),
            state: ConnectionState::InUse,
        }
    }

    /// Returns the TCP address of the peer.
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    pub(crate) fn state(&self) -> ConnectionState {
        self.state
    }

    pub(crate) fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
    }

    pub(crate) fn stream_mut(&mut self) -> &mut BufferedIo<TcpStream> {
        &mut self.stream
    }
}
impl AsMut<Connection> for Connection {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionState {
    InUse,
    Recyclable,
    Closed,
}
