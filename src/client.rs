use url::Url;

use connection::{AcquireConnection, Oneshot};
use RequestBuilder;

/// HTTP client.
#[derive(Debug, Default, Clone)]
pub struct Client<C = Oneshot> {
    connection_provider: C,
}
impl<C: AcquireConnection> Client<C> {
    /// Makes a new `Client` instance.
    pub fn new(connection_provider: C) -> Self {
        Client {
            connection_provider,
        }
    }

    pub fn request<'a>(&'a mut self, url: &'a Url) -> RequestBuilder<C> {
        RequestBuilder::new(&mut self.connection_provider, url)
    }
}
