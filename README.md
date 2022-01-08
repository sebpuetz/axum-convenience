# Axum app wrapper

**Under development**

This crate is supposed to bring some convenience wrappers for `axum`.

Currently there are very few conveniences available, one of them is a configurable shutdown
listener that either waits for signals or for the completion of a provided future.

Another one is encapsulation of a running server in the `App` struct which offers an API to
retrieve the local address of the bound socket.

Internally, the crate uses [axum_server](https://github.com/programatik29/axum-server/) to serve
application.

I'd like to offer some easy to enable middleware layers that e.g. aggregate metrics like latencies
per endpoint.


# Example

```rust
use axum::Router;
use axum::routing::get;
use axum_convenience::{App, ShutdownSignal};

let router = Router::new().route(
    "/hello-world",
    get::<_, _, Body>(|| async { "hello world" }),
);

let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
let app = App::builder(addr, router)
    .with_graceful_shutdown(ShutdownSignal::OsSignal)
    .spawn();

println!("App listening at {}, waiting for shutdown signal", app.local_addr());
app.await?;
```
