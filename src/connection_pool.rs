#![allow(missing_docs)] // TODO
use fibers::net::TcpStream;
use fibers::sync::{mpsc, oneshot};
use fibers::time::timer::TimerExt;
use fibers::{BoxSpawn, Spawn};
use futures::{Async, Future, Poll, Stream};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant, SystemTime};
use trackable::error::ErrorKindExt;

use connection::{AcquireConnection, Connection};
use {Error, ErrorKind, Result};

const CONNECT_TIMEOUT_SECS: u64 = 5; // TODO: parameter
const KEEP_ALIVE_TIMEOUT_SECS: u64 = 10; // TODO

// TODO: metrics

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
        ConnectionPool {
            spawner: spawner.boxed(),
            command_tx,
            command_rx,
            pooled_connections: BTreeMap::new(),
            lending_connections: HashMap::new(),
            timeout_queue: VecDeque::new(),
            max_pool_size: 4096, // TODO
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectionKey {
    addr: SocketAddr,
    pooled_time: Option<Instant>,
}
impl ConnectionKey {
    fn new(addr: SocketAddr) -> Self {
        ConnectionKey {
            addr,
            pooled_time: Some(Instant::now()),
        }
    }

    fn lower_bound(addr: SocketAddr) -> Self {
        ConnectionKey {
            addr,
            pooled_time: None,
        }
    }

    fn key(&self) -> (IpAddr, u16, Option<Instant>) {
        (self.addr.ip(), self.addr.port(), self.pooled_time)
    }
}
impl PartialOrd for ConnectionKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.key().partial_cmp(&other.key())
    }
}
impl Ord for ConnectionKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct ConnectionPool {
    spawner: BoxSpawn,
    command_tx: mpsc::Sender<Command>,
    command_rx: mpsc::Receiver<Command>,
    pooled_connections: BTreeMap<ConnectionKey, Connection>,
    lending_connections: HashMap<SocketAddr, usize>,
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

    fn pool_size(&self) -> usize {
        // TODO
        self.pooled_connections.len() + self.lending_connections.values().sum::<usize>()
    }

    fn lend_pooled_connection(&mut self, addr: SocketAddr) -> Option<Connection> {
        let lower = ConnectionKey::lower_bound(addr);
        if let Some(key) = self.pooled_connections
            .range(lower..)
            .nth(0)
            .and_then(|(key, _)| {
                if key.addr == addr {
                    Some(key.clone())
                } else {
                    None
                }
            }) {
            let connection = self.pooled_connections.remove(&key).expect("never fails");
            *self.lending_connections.entry(addr).or_insert(0) += 1;
            Some(connection)
        } else {
            None
        }
    }

    fn acquire(&mut self, addr: SocketAddr) -> Result<Option<RentedConnection>> {
        if let Some(connection) = self.lend_pooled_connection(addr) {
            let rented = RentedConnection {
                connection: Some(connection),
                command_tx: self.command_tx.clone(),
            };
            return Ok(Some(rented));
        }

        if self.pool_size() == self.max_pool_size {
            // TODO: kicked-out
        }

        *self.lending_connections.entry(addr).or_insert(0) += 1;
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
            Command::Missing { addr } => {
                *self.lending_connections
                    .get_mut(&addr)
                    .expect("never fails") -= 1;
                if self.lending_connections[&addr] == 0 {
                    self.lending_connections.remove(&addr);
                }
            }
            Command::Release { connection } => {
                let addr = connection.peer_addr();

                *self.lending_connections
                    .get_mut(&addr)
                    .expect("never fails") -= 1;
                if self.lending_connections[&addr] == 0 {
                    self.lending_connections.remove(&addr);
                }

                self.pooled_connections
                    .insert(ConnectionKey::new(addr), connection);
                // TODO: add to timeout queue
            }
        }
    }
}
impl Future for ConnectionPool {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        // TODO: keep-alive timeout
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
    Missing {
        addr: SocketAddr,
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
        let connection = self.connection.take().expect("never fails");
        let command = if connection.is_recyclable() {
            Command::Release { connection }
        } else {
            Command::Missing {
                addr: connection.peer_addr(),
            }
        };
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
            .timeout_after(Duration::from_secs(CONNECT_TIMEOUT_SECS))
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
                let command = Command::Missing { addr: self.addr };
                let _ = self.command_tx.send(command);
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
