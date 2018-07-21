use bytecodec::io::{ReadBuf, StreamState, WriteBuf};
use fibers;
use futures::{Future, Poll};
use std::sync::Arc;

use Error;

#[derive(Debug)]
pub enum Connection {
    NotConnected { host: Arc<String> },
    Connected,
    Waiting,
}
impl Connection {
    pub fn new(host: String) -> Self {
        Connection::NotConnected {
            host: Arc::new(host),
        }
    }

    pub fn connect(&mut self) -> Connect {
        Connect
    }
}

#[derive(Debug)]
pub struct Connect;
impl Future for Connect {
    type Item = TcpStream;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        panic!()
    }
}

#[derive(Debug)]
pub struct TcpStream(fibers::net::TcpStream);
impl TcpStream {
    pub fn read_buf(&mut self) -> &mut ReadBuf<Vec<u8>> {
        panic!()
    }

    pub fn write_buf(&mut self) -> &mut WriteBuf<Vec<u8>> {
        panic!()
    }

    pub fn state(&self) -> StreamState {
        panic!()
    }
}
