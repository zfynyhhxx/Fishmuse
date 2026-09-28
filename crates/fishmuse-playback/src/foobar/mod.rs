#[cfg(windows)]
mod backend;
#[cfg(windows)]
mod client;
pub mod framing;
pub mod protocol;
mod reconcile;
#[cfg(windows)]
mod transport;

#[cfg(windows)]
pub use backend::{
    ConnectionState, FoobarBackend, FoobarConfig, MediaPathResolver, ReconnectPolicy,
};
#[cfg(windows)]
pub use client::{ClientEvent, FoobarClient};
pub use reconcile::StateReconciler;
