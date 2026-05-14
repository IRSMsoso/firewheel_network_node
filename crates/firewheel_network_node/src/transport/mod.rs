use core::fmt;
use firewheel_core::node::NodeError;
use std::error::Error;

#[cfg(feature = "steam_networking_messages")]
pub mod steam_transport;

/// Trait-based catchall error type for node trait methods
#[derive(Debug)]
pub struct TransportConstructionError(pub Box<dyn Error>);

impl TransportConstructionError {
    pub const fn from_boxed(error: Box<dyn Error>) -> Self {
        Self(error)
    }
}

impl<E> From<E> for TransportConstructionError
where
    E: Error + 'static,
{
    fn from(err: E) -> Self {
        TransportConstructionError(Box::new(err))
    }
}

impl fmt::Display for TransportConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Transport Construction Error: {}", self.0)
    }
}

impl From<TransportConstructionError> for Box<dyn Error> {
    fn from(value: TransportConstructionError) -> Self {
        value.0
    }
}

impl From<TransportConstructionError> for NodeError {
    fn from(value: TransportConstructionError) -> Self {
        Self { 0: value.0 }
    }
}

pub trait NetworkNodeTransport: Send + Sized + 'static {
    type Addr: Clone + Send;
    type Config: Default;

    fn send(&mut self, data: &[u8], addr: &Self::Addr);

    fn receive(&mut self) -> Vec<(Self::Addr, Vec<u8>)>;

    fn construct(config: &Self::Config) -> Result<Self, TransportConstructionError>;
}

// pub trait NonBlockingSocket<A>: Send + Sync
// where
//     A: Clone + PartialEq + Eq + Hash + Send + Sync,
// {
//     /// Takes a [`Message`] and sends it to the given address.
//     fn send_to(&mut self, msg: &Message, addr: &A);
//
//     /// This method should return all messages received since the last time this method was called.
//     /// The pairs `(A, Message)` indicate from which address each packet was received.
//     fn receive_all_messages(&mut self) -> Vec<(A, Message)>;
// }
