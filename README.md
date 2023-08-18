fibers_http_client
==================

[![fibers_http_client](http://meritbadge.herokuapp.com/fibers_http_client)](https://crates.io/crates/fibers_http_client)
[![Documentation](https://docs.rs/fibers_http_client/badge.svg)](https://docs.rs/fibers_http_client)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A tiny asynchronous HTTP/1.1 client library for Rust.

[Documentation](https://docs.rs/fibers_http_client)


Examples
---------

```rust
use fibers_http_client::connection::Oneshot;
use fibers_http_client::Client;
use url::Url;

let url = Url::parse("http://localhost/foo/bar").unwrap();
let mut client = Client::new(Oneshot);
let future = client.request(&url).get();

let response = fibers_global::execute(future).unwrap();
println!("STATUS: {:?}", response.status_code());
println!("BODY: {:?}", response.body());
```
