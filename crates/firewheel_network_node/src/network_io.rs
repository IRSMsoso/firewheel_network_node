use crate::constants::TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE;
use crate::transport::NetworkNodeTransport;
use lazy_static::lazy_static;
use std::marker::PhantomData;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;
use typemap::ShareMap;

/// Network thread that manages transmitting audio data from network transmitter nodes and sending received audio data to network receiver nodes
///
/// One network thread exists per transport type used
///
/// This thread gets spun up when the first network node using a particular transport is created and never dies
pub(crate) fn network_thread<T>(
    mut transport: T,
    control_message_receiver: Receiver<NetworkThreadControlMessage<T>>,
) where
    T: NetworkNodeTransport,
{
    let mut transmitters: Vec<NetworkThreadTransmitterNodeData<T>> = Vec::new();

    // First, any control messages
    while let Ok(control_message) = control_message_receiver.try_recv() {
        match control_message {
            NetworkThreadControlMessage::RegisterTransmitter { consumer } => {
                transmitters.push(NetworkThreadTransmitterNodeData { consumer });
            }
            NetworkThreadControlMessage::RegisterReceiver => {}
        }
    }

    // Then, pull encoded bytes for every transmitter and transmit that data
    transmitters.retain_mut(|transmitter| {
        match transmitter.consumer.pop() {
            Ok(message) => {
                transport.send(
                    &message.encoded_data[0..message.encoded_len],
                    &message.address,
                );
                true
            }
            Err(_) => {
                // Buffer is empty, if we also are abandoned, filter from transmitters we're tracking (The transmitter node producing has been removed)
                if transmitter.consumer.is_abandoned() {
                    false
                } else {
                    true
                }
            }
        }
    });
}

struct NetworkThreadTransmitterNodeData<T>
where
    T: NetworkNodeTransport,
{
    consumer: rtrb::Consumer<TransmitterNodeNetworkMessage<T>>,
}

pub(crate) struct TransmitterNodeNetworkMessage<T>
where
    T: NetworkNodeTransport,
{
    pub(crate) address: T::Addr,
    pub(crate) encoded_data: [u8; TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE],
    pub(crate) encoded_len: usize,
}

pub(crate) enum NetworkThreadControlMessage<T>
where
    T: NetworkNodeTransport,
{
    RegisterTransmitter {
        consumer: rtrb::Consumer<TransmitterNodeNetworkMessage<T>>,
    },
    RegisterReceiver,
}

pub(crate) struct NetworkThreadRegistryKey<T>(PhantomData<T>);

impl<T> typemap::Key for NetworkThreadRegistryKey<T>
where
    T: NetworkNodeTransport,
{
    type Value = Sender<NetworkThreadControlMessage<T>>;
}

lazy_static! {
    /// Shared transmitter to send control messages to the networking thread
    pub(crate) static ref NETWORK_THREAD_REGISTRY: Mutex<ShareMap> = Mutex::new(ShareMap::custom());
}
