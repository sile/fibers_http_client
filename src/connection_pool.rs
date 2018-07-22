use fibers::sync::mpsc;
use fibers::sync::oneshot;
use futures::{Future, Poll};
use slog::{Discard, Logger};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};

use connection::Connection;
use Error;

type ConnectionId = u64;

#[derive(Debug)]
pub struct AcquiredConnection {
    connection: Option<Connection>,
    addr: SocketAddr,
    pool: ConnectionPoolHandle,
}
impl Drop for AcquiredConnection {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            self.pool.release(self.addr, connection);
        }
    }
}

#[derive(Debug)]
struct ConnectionState {
    connection: Connection,
    addr: SocketAddr,
    expiry_time: SystemTime,
}

#[derive(Debug)]
pub struct ConnectionPoolBuilder {
    logger: Logger,
    max_pool_size: usize,
    keep_alive_timeout: Duration,
}
impl ConnectionPoolBuilder {
    pub fn new() -> Self {
        ConnectionPoolBuilder {
            logger: Logger::root(Discard, o!()),
            max_pool_size: 4096,
            keep_alive_timeout: Duration::from_secs(10),
        }
    }

    pub fn logger(&mut self, logger: Logger) -> &mut Self {
        self.logger = logger;
        self
    }

    pub fn max_pool_size(&mut self, n: usize) -> &mut Self {
        self.max_pool_size = n;
        self
    }

    pub fn keep_alive_timeout(&mut self, d: Duration) -> &mut Self {
        self.keep_alive_timeout = d;
        self
    }

    pub fn finish(&self) -> ConnectionPool {
        let (command_tx, command_rx) = mpsc::channel();
        ConnectionPool {
            logger: self.logger.clone(),
            max_pool_size: self.max_pool_size,
            connections: HashMap::new(),
            addr_to_connections: HashMap::new(),
            expiration_queue: VecDeque::new(),
            command_tx,
            command_rx,
        }
    }
}

#[derive(Debug)]
pub struct ConnectionPool {
    logger: Logger,
    max_pool_size: usize,
    connections: HashMap<ConnectionId, ConnectionState>,
    addr_to_connections: HashMap<SocketAddr, Vec<ConnectionId>>,
    expiration_queue: VecDeque<(SystemTime, ConnectionId)>,
    command_tx: mpsc::Sender<Command>,
    command_rx: mpsc::Receiver<Command>,
}
impl ConnectionPool {
    pub fn handle(&self) -> ConnectionPoolHandle {
        ConnectionPoolHandle {
            command_tx: self.command_tx.clone(),
        }
    }
}
impl Future for ConnectionPool {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        panic!()
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionPoolHandle {
    command_tx: mpsc::Sender<Command>,
}
impl ConnectionPoolHandle {
    pub fn acquire(&self, addr: SocketAddr) {
        let (reply_tx, _reply_rx) = oneshot::monitor();
        let command = Command::Acquire { addr, reply_tx };
        let _ = self.command_tx.send(command);
    }

    pub fn release(&self, addr: SocketAddr, connection: Connection) {
        let command = Command::Release { addr, connection };
        let _ = self.command_tx.send(command);
    }
}

#[derive(Debug)]
enum Command {
    Acquire {
        addr: SocketAddr,
        reply_tx: oneshot::Monitored<AcquiredConnection, Error>,
    },
    Release {
        addr: SocketAddr,
        connection: Connection,
    },
}
