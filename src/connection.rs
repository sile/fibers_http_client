use bytecodec::io::BufferedIo;
use fibers::net::TcpStream;
use futures::Future;
use std::net::SocketAddr;

use {BoxFuture, Error};

pub trait ConnectionPool {
    fn acqurie_connection(&mut self, addr: SocketAddr) -> BoxFuture<Connection>;
    fn release_connection(&mut self, connection: Connection);
}

#[derive(Debug, Default, Clone)]
pub struct OneshotConnectionPool(());
impl OneshotConnectionPool {
    pub fn new() -> Self {
        OneshotConnectionPool(())
    }
}
impl ConnectionPool for OneshotConnectionPool {
    fn acqurie_connection(&mut self, addr: SocketAddr) -> BoxFuture<Connection> {
        let future = TcpStream::connect(addr)
            .map_err(move |e| track!(Error::from(e); addr))
            .map(move |stream| Connection::new(addr, stream));
        Box::new(future)
    }

    fn release_connection(&mut self, _connection: Connection) {}
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

    pub fn stream_ref(&self) -> &BufferedIo<TcpStream> {
        &self.stream
    }

    pub fn stream_mut(&mut self) -> &mut BufferedIo<TcpStream> {
        &mut self.stream
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }
}
