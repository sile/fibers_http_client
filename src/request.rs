use bytecodec::bytes::{BytesEncoder, RemainingBytesDecoder};
use bytecodec::io::{IoDecodeExt, IoEncodeExt};
use bytecodec::{Decode, Encode};
use connection::Connection;
use futures::{Async, Future, Poll};
use httpcodec::{
    BodyDecoder, BodyEncoder, HeaderMut, HttpVersion, Method, NoBodyDecoder, Request,
    RequestEncoder, RequestTarget, Response, ResponseDecoder,
};

use connection::TcpStream;
use {Error, ErrorKind, Result};

#[derive(Debug)]
pub struct RequestBuilder<'a, B = Vec<u8>, E = BytesEncoder, D = RemainingBytesDecoder> {
    connection: &'a mut Connection,
    encoder: E,
    decoder: D,
    request: Request<B>,
}
impl<'a, B, E: Encode, D: Decode> RequestBuilder<'a, B, E, D> {
    pub(crate) fn new(
        connection: &'a mut Connection,
        method: &str,
        path: &str,
        body: B,
        encoder: E,
        decoder: D,
    ) -> Result<Self> {
        let method = track!(Method::new(method); method)?;
        let target = track!(RequestTarget::new(path); path)?;
        let request = Request::new(method, target, HttpVersion::V1_1, body);
        Ok(RequestBuilder {
            connection,
            encoder,
            decoder,
            request,
        })
    }

    // TODO: timeout

    pub fn encoder<T: Encode<Item = E::Item>>(self, encoder: T) -> RequestBuilder<'a, B, T, D> {
        RequestBuilder {
            connection: self.connection,
            encoder,
            decoder: self.decoder,
            request: self.request,
        }
    }

    pub fn decoder<T: Decode>(self, decoder: T) -> RequestBuilder<'a, B, E, T> {
        RequestBuilder {
            connection: self.connection,
            encoder: self.encoder,
            decoder,
            request: self.request,
        }
    }

    pub fn header_mut(&mut self) -> HeaderMut {
        self.request.header_mut()
    }

    pub fn inner_ref(&self) -> &Request<B> {
        &self.request
    }

    pub fn inner_mut(&mut self) -> &mut Request<B> {
        &mut self.request
    }
}
impl<'a, E: Encode, D: Decode> RequestBuilder<'a, E::Item, E, D> {
    pub fn execute(self) -> impl Future<Item = Response<D::Item>, Error = Error> {
        let request = self.request;
        let mut encoder = RequestEncoder::new(BodyEncoder::new(self.encoder));
        let decoder = ResponseDecoder::new(BodyDecoder::new(self.decoder));
        self.connection
            .connect()
            .and_then(move |stream| {
                track!(encoder.start_encoding(request))?;
                Ok((stream, encoder))
            })
            .and_then(move |(stream, encoder)| Execute {
                stream,
                encoder,
                decoder,
            })
    }
}

#[derive(Debug)]
struct Execute<E, D>
where
    E: Encode,
    D: Decode,
{
    stream: TcpStream,
    encoder: RequestEncoder<BodyEncoder<E>>,
    decoder: ResponseDecoder<BodyDecoder<D>>, // TODO: support head
}
impl<E, D> Future for Execute<E, D>
where
    E: Encode,
    D: Decode,
{
    type Item = Response<D::Item>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            track!(self.encoder.encode_to_write_buf(self.stream.write_buf()))?;
            track!(self.decoder.decode_from_read_buf(self.stream.read_buf()))?;
            if self.decoder.is_idle() {
                if self.encoder.is_idle() {
                    // TODO: return connection to client (todo: connection header)
                }
                let response = track!(self.decoder.finish_decoding())?;
                return Ok(Async::Ready(response));
            }

            if self.stream.state().is_eos() {
                track_panic!(ErrorKind::UnexpectedEos);
            }
            if self.stream.state().would_block() {
                break;
            }
        }
        Ok(Async::NotReady)
    }
}
