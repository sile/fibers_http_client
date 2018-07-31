//! [Prometheus] metrics.
//!
//! [Prometheus]: https://prometheus.io/
use prometrics::metrics::{Counter, Gauge, MetricBuilder};

/// [`ConnectionPool`] metrics.
///
/// [`ConnectionPool`]: ../connection/struct.ConnectionPool.html
#[derive(Debug, Clone)]
pub struct ConnectionPoolMetrics {
    pub(crate) max_pool_size: Gauge,

    // allocated
    pub(crate) allocated_connections: Counter,

    // released
    pub(crate) closed_connections: Counter,
    pub(crate) connect_failed_connections: Counter,
    pub(crate) request_failed_connections: Counter,
    pub(crate) expired_connections: Counter,
    pub(crate) kicked_out_connections: Counter,

    // lent
    pub(crate) lent_connections: Counter,

    // returned
    pub(crate) returned_connections: Counter,

    // error
    pub(crate) no_available_connection_errors: Counter,
}
impl ConnectionPoolMetrics {
    /// Maximum number of pooled connections.
    ///
    /// Metric: `fibers_http_client_connection_pool_max_pool_size <GAUGE>`
    pub fn max_pool_size(&self) -> usize {
        self.max_pool_size.value() as usize
    }

    /// Current number of pooled connections.
    ///
    /// This includes the connections that being used by clients.
    ///
    /// Metric: `sum(fibers_http_client_connection_allocated_connections_total) - sum(fibers_http_client_connection_released_connections_total)`
    pub fn pool_size(&self) -> usize {
        let released = self.closed_connections()
            + self.connect_failed_connections()
            + self.request_failed_connections()
            + self.expired_connections()
            + self.kicked_out_connections();
        let allocated = self.allocated_connections();
        allocated.saturating_sub(released) as usize
    }

    /// Number of connections allocated by the pool.
    ///
    /// Metric: `fibers_http_client_connection_allocated_connections_total <COUNTER>`
    pub fn allocated_connections(&self) -> u64 {
        self.allocated_connections.value() as u64
    }

    /// Number of connections released from the pool due to TCP closure.
    ///
    /// Metric: `fibers_http_client_connection_released_connections_total { reason="closed" } <COUNTER>`
    pub fn closed_connections(&self) -> u64 {
        self.closed_connections.value() as u64
    }

    /// Number of connections released from the pool due to TCP connect error.
    ///
    /// Metric: `fibers_http_client_connection_released_connections_total { reason="connect_failed" } <COUNTER>`
    pub fn connect_failed_connections(&self) -> u64 {
        self.connect_failed_connections.value() as u64
    }

    /// Number of connections released from the pool due to HTTP request error.
    ///
    /// Metric: `fibers_http_client_connection_released_connections_total { reason="request_failed" } <COUNTER>`
    pub fn request_failed_connections(&self) -> u64 {
        self.request_failed_connections.value() as u64
    }

    /// Number of connections released from the pool due to keepalive expiration.
    ///
    /// Metric: `fibers_http_client_connection_released_connections_total { reason="expired" } <COUNTER>`
    pub fn expired_connections(&self) -> u64 {
        self.expired_connections.value() as u64
    }

    /// Number of connections kicked out from the pool.
    ///
    /// Metric: `fibers_http_client_connection_released_connections_total { reason="kicked_out" } <COUNTER>`
    pub fn kicked_out_connections(&self) -> u64 {
        self.kicked_out_connections.value() as u64
    }

    /// Number of connections lent to clients.
    ///
    /// Metric: `fibers_http_client_connection_lent_connections_total <COUNTER>`
    pub fn lent_connections(&self) -> u64 {
        self.lent_connections.value() as u64
    }

    /// Number of connections returned from clients.
    ///
    /// Metric: `fibers_http_client_connection_returned_connections_total <COUNTER>`
    pub fn returned_connections(&self) -> u64 {
        self.returned_connections.value() as u64
    }

    /// Currently used connections by clients.
    ///
    /// Metric: `fibers_http_client_connection_lent_connections_total - fibers_http_client_connection_returned_connections_total`
    pub fn in_use_connetions(&self) -> u64 {
        self.lent_connections()
            .saturating_sub(self.returned_connections())
    }

    /// Number of connection acquisition failures.
    ///
    /// Metric: `fibers_http_client_connection_errors_total { reason="no_available_connection" } <COUNTER>`
    pub fn no_available_connection_errors(&self) -> u64 {
        self.no_available_connection_errors.value() as u64
    }

    pub(crate) fn new(mut builder: MetricBuilder) -> Self {
        builder
            .namespace("fibers_http_client")
            .subsystem("connection_pool");
        ConnectionPoolMetrics {
            max_pool_size: builder
                .gauge("max_pool_size")
                .help("Maximum number of pooled connections")
                .finish()
                .expect("never fails"),
            allocated_connections: builder
                .counter("allocated_connections_total")
                .help("Number of connections allocated by pools so far")
                .finish()
                .expect("never fails"),
            closed_connections: builder
                .counter("released_connections_total")
                .help("Number of connections released from pools so far")
                .label("reason", "closed")
                .finish()
                .expect("never fails"),
            connect_failed_connections: builder
                .counter("released_connections_total")
                .help("Number of connections released from pools so far")
                .label("reason", "connect_failed")
                .finish()
                .expect("never fails"),
            request_failed_connections: builder
                .counter("released_connections_total")
                .help("Number of connections released from pools so far")
                .label("reason", "request_failed")
                .finish()
                .expect("never fails"),
            expired_connections: builder
                .counter("released_connections_total")
                .help("Number of connections released from pools so far")
                .label("reason", "expired")
                .finish()
                .expect("never fails"),
            kicked_out_connections: builder
                .counter("released_connections_total")
                .help("Number of connections released from pools so far")
                .label("reason", "kicked_out")
                .finish()
                .expect("never fails"),
            lent_connections: builder
                .counter("lent_connections_total")
                .help("Number of connections lent to clients so far")
                .finish()
                .expect("never fails"),
            returned_connections: builder
                .counter("returned_connections_total")
                .help("Number of connections returned from clients so far")
                .finish()
                .expect("never fails"),
            no_available_connection_errors: builder
                .counter("no_available_connection_errors_total")
                .help("Number of errors")
                .label("reason", "no_available_connection")
                .finish()
                .expect("never fails"),
        }
    }
}
