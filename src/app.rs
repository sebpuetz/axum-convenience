use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use axum_server::service::SendService;
use axum_server::Handle;
use futures_util::future::FutureExt;
use futures_util::poll;
use hyper::server::conn::AddrStream;
use hyper::Body;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::task::{JoinError, JoinHandle};

use crate::shutdown::ShutdownSignal;

/// Builder struct to configure [`App`] instances.
///
/// Constructed through [`App::builder`].
pub struct AppBuilder {
    addr: SocketAddr,
    router: Router<Body>,
    shutdown_listener: Option<ShutdownSignal>,
    #[cfg(feature = "tls")]
    tls: Option<axum_server::tls_rustls::RustlsConfig>,
}

impl AppBuilder {
    pub(crate) fn new(addr: SocketAddr, router: Router<Body>) -> Self {
        Self {
            addr,
            router,
            shutdown_listener: None,
            #[cfg(feature = "tls")]
            tls: None,
        }
    }

    /// Set a graceful shutdown listener for the server.
    ///
    /// Once the `shutdown_listener` future finishes, the server will receive a signal to
    /// gracefully shut down. For more information, refer to
    /// [`axum_server::Handle::graceful_shutdown`].
    pub fn with_graceful_shutdown(mut self, shutdown: ShutdownSignal) -> Self {
        self.shutdown_listener = Some(shutdown);
        self
    }

    #[cfg(feature = "tls")]
    pub fn with_tls(mut self, tls: axum_server::tls_rustls::RustlsConfig) -> Self {
        self.tls = Some(tls);
        self
    }

    /// Spawn the [`App`] with the given configuration.
    pub async fn spawn(mut self) -> App {
        if let Some(tls_config) = self.tls.take() {
            let acceptor = axum_server::tls_rustls::RustlsAcceptor::new(tls_config);
            self.spawn_with_acceptor(acceptor).await
        } else {
            let acceptor = axum_server::accept::DefaultAcceptor::default();
            self.spawn_with_acceptor(acceptor).await
        }
    }

    async fn spawn_with_acceptor<A>(self, acceptor: A) -> App
    where
        A: axum_server::accept::Accept<AddrStream, Router> + Clone + Send + Sync + 'static,
        A::Stream: AsyncRead + AsyncWrite + Unpin + Send,
        A::Service: SendService<hyper::Request<hyper::Body>> + Send,
        A::Future: Send,
    {
        let server_handle = Handle::new();
        let service_maker = self.router.into_make_service();
        let server = axum_server::Server::bind(self.addr)
            .acceptor(acceptor)
            .handle(server_handle.clone())
            .serve(service_maker);
        let server_handle_ = server_handle.clone();
        let handle = if let Some(listener) = self.shutdown_listener {
            tokio::spawn(async move {
                let mut server = tokio::spawn(server);
                struct ShutdownGuard {
                    handle: axum_server::Handle,
                }
                impl Drop for ShutdownGuard {
                    fn drop(&mut self) {
                        self.handle.shutdown();
                    }
                }
                let _guard = ShutdownGuard {
                    handle: server_handle_.clone(),
                };
                tokio::select! {
                    _ = listener.into_signal() => {
                        server_handle_.graceful_shutdown(None);
                        server.await.map_err(ServerError::PanicError)??;
                    },
                    srv_res = &mut server => {
                        srv_res.map_err(ServerError::PanicError)??;
                    }
                };
                Ok(())
            })
        } else {
            tracing::debug!("app running without graceful shutdown listener");
            tokio::spawn(server.map(|res| res.map_err(Into::into)))
        };
        tracing::debug!("app bound to {}", self.addr);
        App {
            addr: server_handle.listening().await,
            handle,
            server_handle,
        }
    }
}

/// Handle to a running App instance.
///
/// Can be configured and spawned through an [`AppBuilder`] obtained through [`App::builder`].
pub struct App {
    addr: SocketAddr,
    server_handle: axum_server::Handle,
    handle: JoinHandle<Result<(), ServerError>>,
}

impl App {
    /// Get an [`AppBuilder`] to build and spawn the App.
    ///
    /// # Example
    ///
    /// To run the [`App`] on port `8080`:
    ///
    /// ```
    /// # fn main() {}
    /// use axum::Router;
    /// use axum_convenience::App;
    ///
    /// async fn run(router: axum::Router) {
    ///     let app = App::builder(([127, 0, 0, 1], 8080).into(), router).spawn().await;
    /// }
    /// ```
    pub fn builder(addr: SocketAddr, router: Router) -> AppBuilder {
        AppBuilder::new(addr, router)
    }

    /// Returns the local address for the socket this server is listening on.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Kills the server by aborting the underlying tokio task.
    ///
    /// After aborting the task, this function awaits the task future. If the task future returns
    /// a panic error, we forward that error. If the returned error indicates cancellation, [`Ok`]
    /// is returned. If the server had already finished running before the `kill` call, that result
    /// is returned.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() {}
    /// use axum_convenience::App;
    ///
    /// async fn run_and_kill(app: App) {
    ///     app.kill().await.unwrap()
    /// }
    /// ```
    pub fn kill(self) -> Shutdown {
        tracing::debug!("aborting server task");
        self.handle.abort();
        Shutdown {
            handle: self.handle,
        }
    }

    /// Explicitly request graceful shutdown.
    pub fn graceful_shutdown(self, grace_period: Option<Duration>) -> Shutdown {
        self.server_handle.graceful_shutdown(grace_period);
        Shutdown {
            handle: self.handle,
        }
    }
}

impl Future for App {
    type Output = Result<(), ServerError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let handle = Pin::new(&mut self.handle);
        poll_app_res(handle, cx)
    }
}

pub struct Shutdown {
    handle: JoinHandle<Result<(), ServerError>>,
}

impl Future for Shutdown {
    type Output = Result<(), ServerError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let handle = Pin::new(&mut self.handle);
        poll_app_res(handle, cx)
    }
}

fn poll_app_res(
    mut slf: Pin<&mut JoinHandle<Result<(), ServerError>>>,
    cx: &mut Context<'_>,
) -> Poll<Result<(), ServerError>> {
    if let Poll::Ready(rdy) = slf.poll_unpin(cx) {
        let res = match rdy {
            Ok(res) => res,
            Err(e) => {
                if e.is_cancelled() {
                    Ok(())
                } else if e.is_panic() {
                    Err(ServerError::PanicError(e))
                } else {
                    // JoinError's inner `Repr` type is private so we can't get exhaustiveness checks here.
                    // Panicking might not be the nicest solution, but it points out if a new kind of JoinError
                    // should be introduced.
                    unreachable!("join errors should either be panic errors or cancelled")
                }
            }
        };
        return Poll::Ready(res);
    }
    Poll::Pending
}

/// Errors encountered while running an [`App`].
#[derive(thiserror::Error, Debug)]
pub enum ServerError {
    #[error("Server task panicked while running: {0}")]
    PanicError(JoinError),
    #[error("Server errored: {0}")]
    RuntimeError(#[from] std::io::Error),
}
