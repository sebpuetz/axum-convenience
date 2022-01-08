use std::time::Duration;

use futures_util::future::BoxFuture;
use futures_util::{Future, FutureExt};

/// Graceful shutdown signals for an [`crate::app::App`].
#[derive(Debug)]
pub struct ShutdownSignal {
    kind: ShutdownSignalKind,
    grace_period: Option<Duration>,
}

impl ShutdownSignal {
    /// Shutdown is signalled by SIGTERM or Ctrl-c
    pub fn os_signal() -> Self {
        ShutdownSignal {
            kind: ShutdownSignalKind::OsSignal,
            grace_period: None,
        }
    }

    /// Custom shutdown handler triggered by the future finishing.
    pub fn custom<F>(signal: F) -> Self
    where
        F: Future<Output = ()> + 'static + Send,
    {
        ShutdownSignal {
            kind: ShutdownSignalKind::Custom(signal.boxed()),
            grace_period: None,
        }
    }

    /// Configure a grace period that allows connections to finish.
    pub fn grace_period(mut self, duration: Duration) -> Self {
        self.grace_period = Some(duration);
        self
    }

    pub(crate) async fn wait_for_signal(self) -> Option<Duration> {
        self.kind.into_signal().await;
        self.grace_period
    }
}

pub(crate) enum ShutdownSignalKind {
    OsSignal,
    Custom(BoxFuture<'static, ()>),
}

impl ShutdownSignalKind {
    pub(crate) async fn into_signal(self) {
        match self {
            ShutdownSignalKind::OsSignal => Self::listen_for_signals().await,
            ShutdownSignalKind::Custom(custom_signal) => custom_signal.await,
        }
    }

    fn listen_for_signals() -> BoxFuture<'static, ()> {
        #[cfg(unix)]
        let signal = async {
            use tokio::signal::unix::*;
            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
            tokio::select! {
                _ = sigterm.recv() => {
                    tracing::info!("received SIGTERM")
                },
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("received Ctrl-C")
                }
            }
        };
        #[cfg(not(unix))]
        let signal = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        signal.boxed()
    }
}

impl std::fmt::Debug for ShutdownSignalKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OsSignal => write!(f, "OsSignal"),
            Self::Custom(_) => f.debug_tuple("Custom").field(&"Future").finish(),
        }
    }
}
