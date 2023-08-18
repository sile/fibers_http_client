use bytecodec;
use std;
use trackable::error::TrackableError;
use trackable::error::{ErrorKind as TrackableErrorKind, ErrorKindExt};
use url;

/// This crate specific `Error` type.
#[derive(Debug, Clone, trackable::TrackableError)]
pub struct Error(TrackableError<ErrorKind>);
impl From<std::io::Error> for Error {
    fn from(f: std::io::Error) -> Self {
        ErrorKind::Other.cause(f).into()
    }
}
impl From<std::sync::mpsc::RecvError> for Error {
    fn from(f: std::sync::mpsc::RecvError) -> Self {
        ErrorKind::Other.cause(f).into()
    }
}
impl From<bytecodec::Error> for Error {
    fn from(f: bytecodec::Error) -> Self {
        let bytecodec_error_kind = *f.kind();
        let kind = match *f.kind() {
            bytecodec::ErrorKind::InvalidInput => ErrorKind::InvalidInput,
            bytecodec::ErrorKind::UnexpectedEos => ErrorKind::UnexpectedEos,
            _ => ErrorKind::Other,
        };
        track!(kind.takes_over(f); bytecodec_error_kind).into()
    }
}
impl From<url::ParseError> for Error {
    fn from(f: url::ParseError) -> Self {
        ErrorKind::InvalidInput.cause(f).into()
    }
}

/// Possible error kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum ErrorKind {
    InvalidInput,
    UnexpectedEos,
    Timeout,
    TemporarilyUnavailable,
    Other,
}
impl TrackableErrorKind for ErrorKind {}
