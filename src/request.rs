use bytecodec::bytes::{BytesEncoder, RemainingBytesDecoder};
use bytecodec::io::{IoDecodeExt, IoEncodeExt};
use bytecodec::{Decode, Encode};
use fibers::time::timer::TimerExt;
use futures::future::{failed, Either};
use futures::{Async, Future, Poll};
use httpcodec::{
    BodyDecode, BodyDecoder, BodyEncoder, HeaderField, HttpVersion, Method, NoBodyDecoder, Request,
    RequestEncoder, RequestTarget, Response, ResponseDecoder,
};
use std::borrow::Cow;
use std::net::ToSocketAddrs;
use std::time::Duration;
use trackable::error::ErrorKindExt;
use url::{Position, Url};

use connection::{AcquireConnection, Connection, ConnectionState};
use {Error, ErrorKind, Result};

/// HTTP request builder.
///
/// This is created by calling [`Client::request`] method.
///
/// [`Client::request`]: ./struct.Client.html#method.request
#[derive(Debug)]
pub struct RequestBuilder<'a, C: 'a, E = BytesEncoder, D = RemainingBytesDecoder> {
    connection_provider: &'a mut C,
    url: &'a Url,
    header_fields: Vec<(Cow<'a, str>, Cow<'a, str>)>,
    encoder: E,
    decoder: D,
    timeout: Option<Duration>,
}
impl<'a, C: 'a> RequestBuilder<'a, C> {
    pub(crate) fn new(connection_provider: &'a mut C, url: &'a Url) -> Self {
        RequestBuilder {
            connection_provider,
            url,
            header_fields: Vec::new(),
            encoder: BytesEncoder::default(),
            decoder: RemainingBytesDecoder::default(),
            timeout: None,
        }
    }
}
impl<'a, C: 'a, E, D> RequestBuilder<'a, C, E, D>
where
    C: AcquireConnection,
    E: Encode,
    D: Decode,
{
    /// Executes `GET` request.
    pub fn get(mut self) -> impl Future<Item = Response<D::Item>, Error = Error> {
        let timeout = self.timeout;
        let f = move || {
            let request = track!(self.build_request("GET", Vec::new()))?;
            let connect = track!(self.connect())?;
            let decoder = ResponseDecoder::new(BodyDecoder::new(self.decoder));
            let mut encoder = RequestEncoder::new(BodyEncoder::new(BytesEncoder::new()));
            track!(encoder.start_encoding(request))?;
            Ok(connect.and_then(move |connection| Execute {
                connection,
                encoder,
                decoder,
            }))
        };
        track_err!(Self::execute(f(), timeout))
    }

    /// Executes `HEAD` request.
    pub fn head(mut self) -> impl Future<Item = Response<()>, Error = Error> {
        let timeout = self.timeout;
        let mut f = move || {
            let request = track!(self.build_request("HEAD", Vec::new()))?;
            let connect = track!(self.connect())?;
            let decoder = ResponseDecoder::new(NoBodyDecoder);
            let mut encoder = RequestEncoder::new(BodyEncoder::new(BytesEncoder::new()));
            track!(encoder.start_encoding(request))?;
            Ok(connect.and_then(move |connection| Execute {
                connection,
                encoder,
                decoder,
            }))
        };
        track_err!(Self::execute(f(), timeout))
    }

    /// Executes `DELETE` request.
    pub fn delete(mut self) -> impl Future<Item = Response<D::Item>, Error = Error> {
        let timeout = self.timeout;
        let f = move || {
            let request = track!(self.build_request("DELETE", Vec::new()))?;
            let connect = track!(self.connect())?;
            let decoder = ResponseDecoder::new(BodyDecoder::new(self.decoder));
            let mut encoder = RequestEncoder::new(BodyEncoder::new(BytesEncoder::new()));
            track!(encoder.start_encoding(request))?;
            Ok(connect.and_then(move |connection| Execute {
                connection,
                encoder,
                decoder,
            }))
        };
        track_err!(Self::execute(f(), timeout))
    }

    /// Executes `PUT` request.
    pub fn put(mut self, body: E::Item) -> impl Future<Item = Response<D::Item>, Error = Error> {
        let timeout = self.timeout;
        let f = move || {
            let request = track!(self.build_request("PUT", body))?;
            let connect = track!(self.connect())?;
            let decoder = ResponseDecoder::new(BodyDecoder::new(self.decoder));
            let mut encoder = RequestEncoder::new(BodyEncoder::new(self.encoder));
            track!(encoder.start_encoding(request))?;
            Ok(connect.and_then(move |connection| Execute {
                connection,
                encoder,
                decoder,
            }))
        };
        track_err!(Self::execute(f(), timeout))
    }

    /// Executes `POST` request.
    pub fn post(mut self, body: E::Item) -> impl Future<Item = Response<D::Item>, Error = Error> {
        let timeout = self.timeout;
        let f = move || {
            let request = track!(self.build_request("POST", body))?;
            let connect = track!(self.connect())?;
            let decoder = ResponseDecoder::new(BodyDecoder::new(self.decoder));
            let mut encoder = RequestEncoder::new(BodyEncoder::new(self.encoder));
            track!(encoder.start_encoding(request))?;
            Ok(connect.and_then(move |connection| Execute {
                connection,
                encoder,
                decoder,
            }))
        };
        track_err!(Self::execute(f(), timeout))
    }

    /// Adds a field to the tail of the HTTP header of the request.
    pub fn header_field<N, V>(mut self, name: N, value: V) -> Self
    where
        N: Into<Cow<'a, str>>,
        V: Into<Cow<'a, str>>,
    {
        self.header_fields.push((name.into(), value.into()));
        self
    }

    /// Sets the timeout of the request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Sets the encoder for serializing the body of the HTTP request.
    ///
    /// This is only meaningful at the case the method of the request is `PUT` or `POST`.
    pub fn encoder<T>(self, encoder: T) -> RequestBuilder<'a, C, T, D> {
        RequestBuilder {
            connection_provider: self.connection_provider,
            url: self.url,
            header_fields: self.header_fields,
            encoder,
            decoder: self.decoder,
            timeout: self.timeout,
        }
    }

    /// Sets the decoder for deserializing the body of the HTTP response replied from the server.
    ///
    /// The decoder is unused if the method of the request is `HEAD`.
    pub fn decoder<T>(self, decoder: T) -> RequestBuilder<'a, C, E, T> {
        RequestBuilder {
            connection_provider: self.connection_provider,
            url: self.url,
            header_fields: self.header_fields,
            encoder: self.encoder,
            decoder,
            timeout: self.timeout,
        }
    }

    fn build_request<T>(&self, method: &str, body: T) -> Result<Request<T>> {
        track_assert_eq!(self.url.scheme(), "http", ErrorKind::InvalidInput; self.url);

        let method = unsafe { Method::new_unchecked(method) };
        let target = track!(RequestTarget::new(&self.url[Position::BeforePath..]); self.url)?;
        let mut request = Request::new(method, target, HttpVersion::V1_1, body);

        let mut has_host = false;
        for (name, value) in &self.header_fields {
            if !has_host && name.eq_ignore_ascii_case("Host") {
                has_host = true;
            }
            let field = track!(HeaderField::new(&name, &value); name, value)?;
            request.header_mut().add_field(field);
        }
        if !has_host {
            let host = &self.url[Position::BeforeHost..Position::AfterPort];
            let field = track!(HeaderField::new("Host", host); host)?;
            request.header_mut().add_field(field);
        }
        Ok(request)
    }

    fn connect(&mut self) -> Result<C::Future> {
        let url = self.url;
        let mut server_addrs = track!(url.to_socket_addrs().map_err(Error::from); url)?;
        let server_addr = track_assert_some!(server_addrs.next(), ErrorKind::InvalidInput; url);
        Ok(self.connection_provider.acquire_connection(server_addr))
    }

    fn execute<F>(
        future: Result<F>,
        timeout: Option<Duration>,
    ) -> impl Future<Item = F::Item, Error = Error>
    where
        F: Future<Error = Error>,
    {
        match future {
            Err(e) => Either::B(failed(track!(e))),
            Ok(future) => {
                if let Some(timeout) = timeout {
                    let future = future.timeout_after(timeout).map_err(|e| {
                        e.unwrap_or_else(|| track!(Error::from(ErrorKind::Timeout.error())))
                    });
                    Either::A(Either::A(future))
                } else {
                    Either::A(Either::B(future))
                }
            }
        }
    }
}

#[derive(Debug)]
struct Execute<C, E, D> {
    connection: C,
    encoder: E,
    decoder: ResponseDecoder<D>,
}
impl<C, E, D> Future for Execute<C, E, D>
where
    C: AsMut<Connection>,
    E: Encode,
    D: BodyDecode,
{
    type Item = Response<D::Item>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut do_close = false;
        let mut response = None;
        loop {
            let stream = self.connection.as_mut().stream_mut();

            track!(stream.execute_io())?;
            track!(self.encoder.encode_to_write_buf(stream.write_buf_mut()))?;
            track!(self.decoder.decode_from_read_buf(stream.read_buf_mut()))?;
            if self.decoder.is_idle() {
                if !self.encoder.is_idle() {
                    do_close = true;
                }

                let res = track!(self.decoder.finish_decoding())?;
                match res.http_version() {
                    HttpVersion::V1_0 => {
                        if res.header().get_field("Connection") != Some("keep-alive") {
                            do_close = true;
                        }
                    }
                    HttpVersion::V1_1 => {
                        if res.header().get_field("Connection") == Some("close") {
                            do_close = true;
                        }
                    }
                }
                response = Some(res);
                break;
            }

            if stream.is_eos() {
                track_panic!(ErrorKind::UnexpectedEos);
            }
            if stream.would_block() {
                break;
            }
        }
        if let Some(response) = response {
            if do_close {
                self.connection.as_mut().set_state(ConnectionState::Closed);
            } else {
                self.connection
                    .as_mut()
                    .set_state(ConnectionState::Recyclable);
            }
            Ok(Async::Ready(response))
        } else {
            Ok(Async::NotReady)
        }
    }
}
