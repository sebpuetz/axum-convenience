//! This library offers some convenient building blocks when developing HTTP services with
//! [`axum`].
//!
//! [`App`] is the central struct wrapping a running HTTP service, offering APIs to e.g. obtain
//! the local address of the bound socket and to kill the server.
//!
//! An [`App`] is configurable through the [`app::AppBuilder`] which can be obtained through
//! [`App::builder`]. Possible configuration currently includes registering different kinds of
//! shutdown listeners based on [`ShutdownSignal`].
//!
//! # Example
//!
//! ```
//! # fn main() {}
//! use std::net::SocketAddr;
//!
//! use axum::Router;
//! use axum_convenience::{App, ServerError, ShutdownSignal};
//!
//! async fn run() -> Result<(), ServerError> {
//!     let router = Router::new();
//!     // wait for SIGTERM (unix-only) and/or ctrl_c
//!     let shutdown_signal = ShutdownSignal::OsSignal;
//!     let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
//!     let app = App::builder(addr, router)
//!         .with_graceful_shutdown(shutdown_signal)
//!         .spawn();
//!     println!("listening at {}", app.local_addr());
//!     app.await
//! }
mod app;
pub use app::{App, AppBuilder, ServerError};
mod shutdown;
pub use shutdown::ShutdownSignal;
