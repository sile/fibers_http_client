extern crate bytecodec;
#[macro_use]
extern crate clap;
extern crate fibers;
extern crate fibers_http_client;
extern crate futures;
#[macro_use]
extern crate trackable;
extern crate url;

use bytecodec::bytes::Utf8Decoder;
use clap::Arg;
use fibers::sync::oneshot::MonitorError;
use fibers::{Executor, InPlaceExecutor, Spawn};
use fibers_http_client::connection::Oneshot;
use fibers_http_client::Client;
use std::time::Duration;
use trackable::error::MainError;
use url::Url;

fn main() -> Result<(), MainError> {
    let matches = app_from_crate!()
        .arg(Arg::with_name("URL").index(1).required(true))
        .arg(
            Arg::with_name("TIMEOUT_MILLIS")
                .long("timeout")
                .takes_value(true),
        )
        .get_matches();
    let url: Url = track_any_err!(matches.value_of("URL").unwrap().parse())?;

    let mut client = Client::new(Oneshot);
    let mut request = client.request(&url).decoder(Utf8Decoder::new());
    if let Some(timeout) = matches.value_of("TIMEOUT_MILLIS") {
        let timeout = Duration::from_millis(track_any_err!(timeout.parse())?);
        request = request.timeout(timeout);
    }
    let future = request.get();

    let mut executor = track_any_err!(InPlaceExecutor::new())?;
    let monitor = executor.spawn_monitor(future);
    match track_any_err!(executor.run_fiber(monitor))? {
        Err(MonitorError::Aborted) => panic!(),
        Err(MonitorError::Failed(e)) => Err(track!(e).into()),
        Ok(response) => {
            println!("{}", response.body());
            Ok(())
        }
    }
}
