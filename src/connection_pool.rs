#![allow(missing_docs)] // TODO
use fibers::net::TcpStream;
use fibers::sync::{mpsc, oneshot};
use fibers::time::timer::TimerExt;
use fibers::{BoxSpawn, Spawn};
use futures::{Async, Future, Poll, Stream};
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};
use trackable::error::ErrorKindExt;

use connection::{AcquireConnection, Connection};
use {Error, ErrorKind, Result};

#[derive(Debug)]
pub struct ConnectionPoolBuilder;
impl ConnectionPoolBuilder {
    pub fn new() -> Self {
        ConnectionPoolBuilder
    }

    pub fn finish<S>(&self, spawner: S) -> ConnectionPool
    where
        S: Spawn + Send + 'static,
    {
        let (command_tx, command_rx) = mpsc::channel();
        // TODO: keepalive-timeout, remove closed connection
        ConnectionPool {
            spawner: spawner.boxed(),
            command_tx,
            command_rx,
            connections: HashMap::new(),
            timeout_queue: VecDeque::new(),
            pool_size: 0,
            max_pool_size: 4096, // TODO
        }
    }
}
// TODO: add connect timeout

#[derive(Debug)]
struct ConnectionState {
    // addr: SocketAddr,
    connection: Option<Connection>,
    last_used_time: SystemTime,
}
impl ConnectionState {
    fn new() -> Self {
        ConnectionState {
            connection: None,
            last_used_time: SystemTime::now(),
        }
    }

    fn key(&self) -> Option<Reverse<SystemTime>> {
        self.connection
            .as_ref()
            .map(|_| Reverse(self.last_used_time))
    }
}
impl PartialOrd for ConnectionState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.key().partial_cmp(&other.key())
    }
}
impl Ord for ConnectionState {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}
impl PartialEq for ConnectionState {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}
impl Eq for ConnectionState {}

#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct ConnectionPool {
    spawner: BoxSpawn,
    command_tx: mpsc::Sender<Command>,
    command_rx: mpsc::Receiver<Command>,
    connections: HashMap<SocketAddr, BinaryHeap<ConnectionState>>,
    pool_size: usize,
    max_pool_size: usize,
    timeout_queue: VecDeque<(SystemTime, SocketAddr)>,
}
impl ConnectionPool {
    pub fn new<S>(spawner: S) -> Self
    where
        S: Spawn + Send + 'static,
    {
        ConnectionPoolBuilder::new().finish(spawner)
    }

    pub fn handle(&self) -> ConnectionPoolHandle {
        ConnectionPoolHandle {
            command_tx: self.command_tx.clone(),
        }
    }

    fn acquire(&mut self, addr: SocketAddr) -> Result<Option<RentedConnection>> {
        if let Some(queue) = self.connections.get_mut(&addr) {
            let mut state = queue.pop().expect("never fails");
            let connection = state.connection.take();
            queue.push(state);

            if let Some(connection) = connection {
                let rented = RentedConnection {
                    connection: Some(connection),
                    command_tx: self.command_tx.clone(),
                };
                return Ok(Some(rented));
            }
        }
        if self.pool_size == self.max_pool_size {
            // TODO: kicked-out
        }

        self.pool_size += 1;
        self.connections
            .entry(addr)
            .or_insert_with(BinaryHeap::new)
            .push(ConnectionState::new());
        Ok(None)
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Acquire { addr, reply_tx } => match track!(self.acquire(addr)) {
                Err(e) => reply_tx.exit(Err(e)),
                Ok(Some(c)) => reply_tx.exit(Ok(c)),
                Ok(None) => {
                    let future = Connect::new(addr, self.command_tx.clone())
                        .then(move |result| Ok(reply_tx.exit(result)));
                    self.spawner.spawn(future);
                }
            },
            Command::Release { connection } => {}
        }
    }
}
impl Future for ConnectionPool {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        while let Async::Ready(command) = self.command_rx.poll().expect("never fails") {
            let command = command.expect("never fails");
            self.handle_command(command);
        }
        Ok(Async::NotReady)
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionPoolHandle {
    command_tx: mpsc::Sender<Command>,
}
impl AcquireConnection for ConnectionPoolHandle {
    type Connection = RentedConnection;
    type Future = Box<Future<Item = Self::Connection, Error = Error> + Send + 'static>;

    fn acquire_connection(&mut self, addr: SocketAddr) -> Self::Future {
        let (reply_tx, reply_rx) = oneshot::monitor();
        let command = Command::Acquire { addr, reply_tx };
        let _ = self.command_tx.send(command);

        let future = reply_rx.map_err(|e| {
            e.unwrap_or_else(|| {
                track!(ErrorKind::Other.cause("`ConnectionPool` has been dropped")).into()
            })
        });
        Box::new(future)
    }
}

#[derive(Debug)]
enum Command {
    Acquire {
        addr: SocketAddr,
        reply_tx: oneshot::Monitored<RentedConnection, Error>,
    },
    Release {
        connection: Connection,
    },
}

#[derive(Debug)]
pub struct RentedConnection {
    connection: Option<Connection>,
    command_tx: mpsc::Sender<Command>,
}
impl RentedConnection {
    fn new(connection: Connection, command_tx: mpsc::Sender<Command>) -> Self {
        RentedConnection {
            connection: Some(connection),
            command_tx,
        }
    }
}
impl AsMut<Connection> for RentedConnection {
    fn as_mut(&mut self) -> &mut Connection {
        self.connection.as_mut().expect("never fails")
    }
}
impl Drop for RentedConnection {
    fn drop(&mut self) {
        // TODO: return error if failed or eos
        let connection = self.connection.take().expect("never fails");
        let command = Command::Release { connection };
        let _ = self.command_tx.send(command);
    }
}

//#[derive(Debug)]
struct Connect {
    future: Box<Future<Item = TcpStream, Error = Error> + Send + 'static>,
    addr: SocketAddr,
    command_tx: mpsc::Sender<Command>,
}
impl Connect {
    fn new(addr: SocketAddr, command_tx: mpsc::Sender<Command>) -> Self {
        let future = TcpStream::connect(addr)
            .map_err(|e| track!(Error::from(e)))
            .timeout_after(Duration::from_secs(5)) // TODO
            .map_err(|e| {
                e.unwrap_or_else(|| track!(ErrorKind::Timeout.cause("TCP connect timeout")).into())
            });
        Connect {
            future: Box::new(future),
            addr,
            command_tx,
        }
    }
}
impl Future for Connect {
    type Item = RentedConnection;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match track!(self.future.poll(); self.addr) {
            Err(e) => {
                // TODO: self.command_tx.end(Release::Error)
                Err(e)
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(stream)) => {
                let connection = Connection::new(self.addr, stream);
                Ok(Async::Ready(RentedConnection::new(
                    connection,
                    self.command_tx.clone(),
                )))
            }
        }
    }
}
