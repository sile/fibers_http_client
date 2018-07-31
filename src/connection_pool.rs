use fibers::net::TcpStream;
use fibers::sync::{mpsc, oneshot};
use fibers::time::timer::{self, Timeout, TimerExt};
use fibers::{BoxSpawn, Spawn};
use futures::{Async, Future, Poll, Stream};
use prometrics::metrics::MetricBuilder;
use std;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use trackable::error::ErrorKindExt;

use connection::{AcquireConnection, Connection, ConnectionState};
use metrics::ConnectionPoolMetrics;
use {Error, ErrorKind, Result};

const TIMER_INTERVAL_SECS: u64 = 1;

/// [`ConnectionPool`] builder.
///
/// [`ConnectionPool`]: ./struct.ConnectionPool.html
#[derive(Debug)]
pub struct ConnectionPoolBuilder {
    max_pool_size: usize,
    connect_timeout: Duration,
    keepalive_timeout: Duration,
    metrics: MetricBuilder,
}
impl ConnectionPoolBuilder {
    /// Makes a new `ConnectionPoolBuilder` instance with the default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum size (i.e., the number of connections) of the pool.
    ///
    /// The default value is `4096`.
    pub fn max_pool_size(&mut self, size: usize) -> &mut Self {
        self.max_pool_size = size;
        self
    }

    /// Sets the timeout duration of TCP connect operation issued by the pool.
    ///
    /// The default value is `Duration::from_secs(5)`.
    pub fn connect_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the retention duration of a pooled (inactive) connection.
    ///
    /// If a connection is inactive (i.e., unused by any clients) beyond the duration, it will removed from the pool.
    ///
    /// The default value is `Duration::from_secs(10)`.
    pub fn keepalive_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.keepalive_timeout = timeout;
        self
    }

    /// Sets the metrics builder used by the pool.
    ///
    /// The default value is `MetricBuilder::new()`.
    pub fn metrics(&mut self, metrics: MetricBuilder) -> &mut Self {
        self.metrics = metrics;
        self
    }

    /// Makes a new [`ConnectionPool`] instance with the given settings.
    ///
    /// [`ConnectionPool`]: ./struct.ConnectionPool.html
    pub fn finish<S>(&self, spawner: S) -> ConnectionPool
    where
        S: Spawn + Send + 'static,
    {
        let (command_tx, command_rx) = mpsc::channel();
        let metrics = ConnectionPoolMetrics::new(self.metrics.clone());
        metrics.max_pool_size.set(self.max_pool_size as f64);
        ConnectionPool {
            spawner: spawner.boxed(),
            command_tx,
            command_rx,
            max_pool_size: self.max_pool_size,
            timer: timer::timeout(Duration::from_secs(TIMER_INTERVAL_SECS)),
            connect_timeout: self.connect_timeout,
            keepalive_timeout: self.keepalive_timeout,
            metrics,
            state: ConnectionPoolState::new(),
        }
    }
}
impl Default for ConnectionPoolBuilder {
    fn default() -> Self {
        ConnectionPoolBuilder {
            max_pool_size: 4096,
            connect_timeout: Duration::from_secs(5),
            keepalive_timeout: Duration::from_secs(10),
            metrics: MetricBuilder::new(),
        }
    }
}

/// Connection pool.
///
/// # Examples
///
/// TODO
///
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct ConnectionPool {
    spawner: BoxSpawn,
    command_tx: mpsc::Sender<Command>,
    command_rx: mpsc::Receiver<Command>,
    max_pool_size: usize,
    timer: Timeout,
    connect_timeout: Duration,
    keepalive_timeout: Duration,
    metrics: ConnectionPoolMetrics,
    state: ConnectionPoolState,
}
impl ConnectionPool {
    /// Makes a new `ConnectionPool` instance with the default settings.
    ///
    /// If you want to customize the settings, please use [`ConnectionPoolBuilder`] instead.
    ///
    /// [`ConnectionPoolBuilder`]: ./struct.ConnectionPoolBuilder.html
    pub fn new<S>(spawner: S) -> Self
    where
        S: Spawn + Send + 'static,
    {
        ConnectionPoolBuilder::new().finish(spawner)
    }

    /// Returns the handle of the pool.
    pub fn handle(&self) -> ConnectionPoolHandle {
        ConnectionPoolHandle {
            command_tx: self.command_tx.clone(),
        }
    }

    /// Returns a reference to the metrics of the pool.
    pub fn metrics(&self) -> &ConnectionPoolMetrics {
        &self.metrics
    }

    fn acquire(&mut self, addr: SocketAddr) -> Result<Option<RentedConnection>> {
        if let Some(mut connection) = self.state.lend_pooled_connection(addr) {
            connection.set_state(ConnectionState::InUse);
            let rented = RentedConnection {
                connection: Some(connection),
                command_tx: self.command_tx.clone(),
            };
            return Ok(Some(rented));
        }

        if self.state.pool_size == self.max_pool_size {
            if self.state.discard_oldest_pooled_connection() {
                self.metrics.kicked_out_connections.increment();
            } else {
                self.metrics.no_available_connection_errors.increment();
                track_panic!(
                    ErrorKind::TemporarilyUnavailable,
                    "Max connection pool size reached: {}",
                    self.max_pool_size
                );
            }
        }
        self.state.lend_new_connection(addr);
        self.metrics.allocated_connections.increment();
        Ok(None)
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Acquire { addr, reply_tx } => match track!(self.acquire(addr)) {
                Err(e) => reply_tx.exit(Err(e)),
                Ok(Some(c)) => {
                    self.metrics.lent_connections.increment();
                    reply_tx.exit(Ok(c))
                }
                Ok(None) => {
                    self.metrics.lent_connections.increment();
                    let future = Connect::new(addr, self.command_tx.clone(), self.connect_timeout)
                        .then(move |result| Ok(reply_tx.exit(result)));
                    self.spawner.spawn(future);
                }
            },
            Command::Discard { addr, reason } => {
                self.metrics.returned_connections.increment();
                self.state.discard_connection(addr);
                match reason {
                    DiscardReason::Closed => {
                        self.metrics.closed_connections.increment();
                    }
                    DiscardReason::ConnectFailed => {
                        self.metrics.connect_failed_connections.increment();
                    }
                    DiscardReason::RequestFailed => {
                        self.metrics.request_failed_connections.increment();
                    }
                }
            }
            Command::Reuse { connection } => {
                self.metrics.returned_connections.increment();
                self.state.pool_connection(connection);
            }
        }
    }
}
impl Future for ConnectionPool {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        while let Async::Ready(()) = track!(self.timer.poll().map_err(Error::from))? {
            let interval = Duration::from_secs(TIMER_INTERVAL_SECS);
            let removed = self.state.tick(interval, self.keepalive_timeout);
            self.metrics.expired_connections.add_u64(removed as u64);
            self.timer = timer::timeout(interval);
        }
        while let Async::Ready(command) = self.command_rx.poll().expect("never fails") {
            let command = command.expect("never fails");
            self.handle_command(command);
        }
        Ok(Async::NotReady)
    }
}

/// Handle for operating [`ConnectionPool`].
///
/// [`ConnectionPool`]: ./struct.ConnectionPool.html
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

/// A connection rented to a client.
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
        let command = match connection.state() {
            ConnectionState::Recyclable => Command::Reuse { connection },
            ConnectionState::Closed => Command::Discard {
                addr: connection.peer_addr(),
                reason: DiscardReason::Closed,
            },
            ConnectionState::InUse => Command::Discard {
                addr: connection.peer_addr(),
                reason: DiscardReason::RequestFailed,
            },
        };
        let _ = self.command_tx.send(command);
    }
}

#[derive(Debug)]
enum Command {
    Acquire {
        addr: SocketAddr,
        reply_tx: oneshot::Monitored<RentedConnection, Error>,
    },
    Reuse {
        connection: Connection,
    },
    Discard {
        addr: SocketAddr,
        reason: DiscardReason,
    },
}

struct Connect {
    future: Box<Future<Item = TcpStream, Error = Error> + Send + 'static>,
    addr: SocketAddr,
    command_tx: mpsc::Sender<Command>,
}
impl Connect {
    fn new(addr: SocketAddr, command_tx: mpsc::Sender<Command>, timeout: Duration) -> Self {
        let future = TcpStream::connect(addr)
            .map_err(|e| track!(Error::from(e)))
            .timeout_after(timeout)
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
                let command = Command::Discard {
                    addr: self.addr,
                    reason: DiscardReason::ConnectFailed,
                };
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

#[derive(Debug)]
struct ConnectionPoolState {
    pooled_connections: BTreeMap<PoolKey, Connection>,
    timeout_queue: BinaryHeap<QueueEntry>,
    elapsed_time: Duration, // Approximate elapsed time since the pool was created
    pool_size: usize,
}
impl ConnectionPoolState {
    fn new() -> Self {
        ConnectionPoolState {
            pooled_connections: BTreeMap::new(),
            timeout_queue: BinaryHeap::new(),
            elapsed_time: Duration::from_secs(0),
            pool_size: 0,
        }
    }

    fn lend_new_connection(&mut self, _addr: SocketAddr) {
        self.pool_size += 1;
    }

    fn lend_pooled_connection(&mut self, addr: SocketAddr) -> Option<Connection> {
        // Tries to select the most recently used connection
        let (lower, upper) = PoolKey::range(addr);
        let selected = self.pooled_connections
            .range(lower..upper)
            .rev()
            .nth(0)
            .map(|(key, _)| key.clone());
        if let Some(key) = selected {
            let connection = self.pooled_connections.remove(&key).expect("never fails");
            Some(connection)
        } else {
            None
        }
    }

    fn discard_oldest_pooled_connection(&mut self) -> bool {
        while let Some(entry) = self.timeout_queue.pop() {
            let removed = self.pooled_connections
                .remove(&entry.to_pool_key())
                .is_some();
            if let Some(key) = self.get_oldest(entry.socket_addr()) {
                self.timeout_queue.push(key.to_queue_entry());
            }
            if removed {
                self.pool_size -= 1;
                return true;
            }
        }
        false
    }

    fn get_oldest(&self, addr: SocketAddr) -> Option<PoolKey> {
        let (lower, upper) = PoolKey::range(addr);
        self.pooled_connections
            .range(lower..upper)
            .nth(0)
            .map(|(key, _)| key.clone())
    }

    fn pool_connection(&mut self, connection: Connection) {
        let addr = connection.peer_addr();
        let key = PoolKey::new(addr, self.elapsed_time);
        if !self.pool_contains(addr) {
            self.timeout_queue.push(key.to_queue_entry());
        }
        self.pooled_connections.insert(key, connection);
    }

    fn pool_contains(&self, addr: SocketAddr) -> bool {
        let (lower, upper) = PoolKey::range(addr);
        self.pooled_connections.range(lower..upper).nth(0).is_some()
    }

    fn discard_connection(&mut self, _addr: SocketAddr) {
        self.pool_size -= 1;
    }

    fn tick(&mut self, duration: Duration, keepalive_timeout: Duration) -> usize {
        self.elapsed_time += duration;
        let now = self.elapsed_time;
        let mut removed_count = 0;
        while let Some(entry) = self.timeout_queue.peek().cloned() {
            if entry.pooled_time.0 + keepalive_timeout < now {
                let _ = self.timeout_queue.pop();
                let removed = self.pooled_connections
                    .remove(&entry.to_pool_key())
                    .is_some();
                if removed {
                    self.pool_size -= 1;
                    removed_count += 1;
                }
                if let Some(key) = self.get_oldest(entry.socket_addr()) {
                    self.timeout_queue.push(key.to_queue_entry());
                }
            } else {
                break;
            }
        }
        removed_count
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PoolKey {
    addr: IpAddr,
    port: u16,
    pooled_time: Duration,
}
impl PoolKey {
    fn new(addr: SocketAddr, now: Duration) -> Self {
        PoolKey {
            addr: addr.ip(),
            port: addr.port(),
            pooled_time: now,
        }
    }

    fn range(addr: SocketAddr) -> (Self, Self) {
        let lower = PoolKey::new(addr, Duration::from_secs(0));
        let upper = PoolKey::new(addr, Duration::from_secs(std::u64::MAX));
        (lower, upper)
    }

    fn to_queue_entry(&self) -> QueueEntry {
        QueueEntry {
            pooled_time: Reverse(self.pooled_time),
            addr: self.addr,
            port: self.port,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct QueueEntry {
    pooled_time: Reverse<Duration>,
    addr: IpAddr,
    port: u16,
}
impl QueueEntry {
    fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.addr, self.port)
    }

    fn to_pool_key(&self) -> PoolKey {
        PoolKey {
            addr: self.addr,
            port: self.port,
            pooled_time: self.pooled_time.0,
        }
    }
}

#[derive(Debug)]
enum DiscardReason {
    Closed,
    ConnectFailed,
    RequestFailed,
}

#[cfg(test)]
mod tests {
    // TODO
}
