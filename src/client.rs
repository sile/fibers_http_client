use connection::Connection;

use {ConnectionPool, OneshotConnectionPool, RequestBuilder, Result};

/// HTTP client.
#[derive(Debug, Clone, Default)]
pub struct Client<P> {
    pool: P,
}
impl<P: ConnectionPool> Client<P> {
    pub fn new(connection_pool: P) -> Self {
        Client {
            pool: connection_pool,
        }
    }

    pub fn get_request(&mut self, path: &str) -> Result<RequestBuilder<P>> {
        track!(RequestBuilder::new(
            &mut self.pool,
            "GET",
            path,
            vec![],
            Default::default(),
            Default::default()
        ))
    }

    // pub fn put_request<B>(&mut self, path: &str, body: B) -> Result<RequestBuilder<B>> {
    //     track!(RequestBuilder::new(
    //         &mut self.connection,
    //         "PUT",
    //         path,
    //         body,
    //         Default::default(),
    //         Default::default()
    //     ))
    // }
}
