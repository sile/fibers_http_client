use bytecodec::bytes::{BytesEncoder, RemainingBytesDecoder};
use bytecodec::io::{IoDecodeExt, IoEncodeExt};
use bytecodec::{Decode, Encode};
use connection::Connection;
use futures::future::{failed, Either};
use futures::{Async, Future, Poll};
use httpcodec::{
    BodyDecode, BodyDecoder, BodyEncoder, HeaderField, HttpVersion, Method, NoBodyDecoder, Request,
    RequestEncoder, RequestTarget, Response, ResponseDecoder,
};
use std::borrow::Cow;
use std::net::ToSocketAddrs;
use url::{Position, Url};

use connection::AcquireConnection;
use {Error, ErrorKind, Result};

/// TODO
///
/// This is created by calling `Client::request` method.
#[derive(Debug)]
pub struct RequestBuilder<'a, C: 'a, E = BytesEncoder, D = RemainingBytesDecoder> {
    connection_provider: &'a mut C,
    url: &'a Url,
    header_fields: Vec<(Cow<'a, str>, Cow<'a, str>)>,
    encoder: E,
    decoder: D,
}
impl<'a, C: 'a> RequestBuilder<'a, C> {
    pub(crate) fn new(connection_provider: &'a mut C, url: &'a Url) -> Self {
        RequestBuilder {
            connection_provider,
            url,
            header_fields: Vec::new(),
            encoder: BytesEncoder::default(),
            decoder: RemainingBytesDecoder::default(),
        }
    }
}
impl<'a, C: 'a, E, D> RequestBuilder<'a, C, E, D>
where
    C: AcquireConnection,
    E: Encode,
    D: Decode,
{
    pub fn encoder<T>(self, encoder: T) -> RequestBuilder<'a, C, T, D> {
        RequestBuilder {
            connection_provider: self.connection_provider,
            url: self.url,
            header_fields: self.header_fields,
            encoder,
            decoder: self.decoder,
        }
    }

    pub fn decoder<T>(self, decoder: T) -> RequestBuilder<'a, C, E, T> {
        RequestBuilder {
            connection_provider: self.connection_provider,
            url: self.url,
            header_fields: self.header_fields,
            encoder: self.encoder,
            decoder,
        }
    }

    pub fn header_field<N, V>(mut self, name: N, value: V) -> Self
    where
        N: Into<Cow<'a, str>>,
        V: Into<Cow<'a, str>>,
    {
        self.header_fields.push((name.into(), value.into()));
        self
    }

    // TODO: timeout

    pub fn get(mut self) -> impl Future<Item = Response<D::Item>, Error = Error> {
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
        match f() {
            Ok(f) => Either::A(f),
            Err(e) => Either::B(failed(e)),
        }
    }

    pub fn head(mut self) -> impl Future<Item = Response<()>, Error = Error> {
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
        match f() {
            Ok(f) => Either::A(f),
            Err(e) => Either::B(failed(e)),
        }
    }

    pub fn delete(mut self) -> impl Future<Item = Response<D::Item>, Error = Error> {
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
        match f() {
            Ok(f) => Either::A(f),
            Err(e) => Either::B(failed(e)),
        }
    }

    pub fn put(mut self, body: E::Item) -> impl Future<Item = Response<D::Item>, Error = Error> {
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
        match f() {
            Ok(f) => Either::A(f),
            Err(e) => Either::B(failed(e)),
        }
    }

    pub fn post(mut self, body: E::Item) -> impl Future<Item = Response<D::Item>, Error = Error> {
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
        match f() {
            Ok(f) => Either::A(f),
            Err(e) => Either::B(failed(e)),
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
        Ok(self.connection_provider.acqurie_connection(server_addr))
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
                if res.header().get_field("Connection") == Some("close") {
                    // TODO: HTTP/1.0
                    do_close = true;
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
                self.connection.as_mut().close();
            }
            Ok(Async::Ready(response))
        } else {
            Ok(Async::NotReady)
        }
    }
}
