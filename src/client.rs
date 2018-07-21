use connection::Connection;

use {RequestBuilder, Result};

/// HTTP client.
#[derive(Debug)]
pub struct Client {
    connection: Connection,
}
impl Client {
    pub fn new(host: &str) -> Self {
        Client {
            connection: Connection::new(host.to_owned()),
        }
    }

    pub fn get_request(&mut self, path: &str) -> Result<RequestBuilder> {
        track!(RequestBuilder::new(
            &mut self.connection,
            "GET",
            path,
            vec![],
            Default::default(),
            Default::default()
        ))
    }

    pub fn put_request<B>(&mut self, path: &str, body: B) -> Result<RequestBuilder<B>> {
        track!(RequestBuilder::new(
            &mut self.connection,
            "PUT",
            path,
            body,
            Default::default(),
            Default::default()
        ))
    }
}
