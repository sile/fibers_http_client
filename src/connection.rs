use bytecodec::io::{BufferedIo, StreamState};
use fibers::net::TcpStream;
use futures::Future;
use std::net::SocketAddr;

use Error;

pub trait AcquireConnection {
    type Connection: AsMut<Connection>;
    type Future: Future<Item = Self::Connection, Error = Error>;

    fn acqurie_connection(&mut self, addr: SocketAddr) -> Self::Future;
}

#[derive(Debug, Default, Clone)]
pub struct Oneshot;
impl AcquireConnection for Oneshot {
    type Connection = Connection;
    type Future = Box<Future<Item = Self::Connection, Error = Error> + Send + 'static>;
    fn acqurie_connection(&mut self, addr: SocketAddr) -> Self::Future {
        let future = TcpStream::connect(addr)
            .map_err(move |e| track!(Error::from(e); addr))
            .map(move |stream| Connection::new(addr, stream));
        Box::new(future)
    }
}

#[derive(Debug)]
pub struct Connection {
    stream: BufferedIo<TcpStream>,
    peer_addr: SocketAddr,
}
impl Connection {
    pub fn new(peer_addr: SocketAddr, stream: TcpStream) -> Self {
        let _ = stream.set_nodelay(true);
        Connection {
            peer_addr,
            stream: BufferedIo::new(stream, 4096, 4096), // TODO
        }
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    pub fn stream_ref(&self) -> &BufferedIo<TcpStream> {
        &self.stream
    }

    pub fn stream_mut(&mut self) -> &mut BufferedIo<TcpStream> {
        &mut self.stream
    }

    pub(crate) fn close(&mut self) {
        *self.stream.read_buf_mut().stream_state_mut() = StreamState::Eos;
        *self.stream.write_buf_mut().stream_state_mut() = StreamState::Eos;
    }
}
impl AsMut<Connection> for Connection {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}
