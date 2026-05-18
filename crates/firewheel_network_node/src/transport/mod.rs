use core::fmt;
use firewheel_core::node::NodeError;
use std::error::Error;

#[cfg(feature = "steam_networking_messages")]
pub mod steam_transport;
pub mod udp_socket_transport;

/// Trait-based catchall error type for node trait methods
#[derive(Debug)]
pub struct TransportError(pub Box<dyn Error>);

impl TransportError {
    pub const fn from_boxed(error: Box<dyn Error>) -> Self {
        Self(error)
    }
}

impl<E> From<E> for TransportError
where
    E: Error + 'static,
{
    fn from(err: E) -> Self {
        TransportError(Box::new(err))
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Transport Error: {}", self.0)
    }
}

impl From<TransportError> for Box<dyn Error> {
    fn from(value: TransportError) -> Self {
        value.0
    }
}

impl From<TransportError> for NodeError {
    fn from(value: TransportError) -> Self {
        Self { 0: value.0 }
    }
}

pub trait NetworkNodeTransport: Send + Sized + 'static {
    type Addr: Clone + Send;
    type Config: Default;
    const NAME: &'static str;

    /// Called by the network thread for a transport to send data to some network address
    fn send(&mut self, data: &[u8], addr: &Self::Addr) -> Result<(), TransportError>;

    /// Called by the network thread for a transport to receive data. Must not block. Should receive all the messages it can until needing to block.
    fn try_receive(&mut self) -> Result<Vec<(Self::Addr, Vec<u8>)>, TransportError>;

    /// Constructs an instance of a transport via some config
    fn construct(config: &Self::Config) -> Result<Self, TransportError>;
}
