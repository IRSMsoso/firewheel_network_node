use crate::constants::TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE;
use crate::transport::NetworkNodeTransport;
use lazy_static::lazy_static;
use log::error;
use serde::{Deserialize, Serialize};
use std::cmp::min;
use std::marker::PhantomData;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;
use typemap_rev::TypeMap;

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
    let mut receivers: Vec<NetworkThreadReceiverNodeData> = Vec::new();

    loop {
        // First, any control messages
        while let Ok(control_message) = control_message_receiver.try_recv() {
            match control_message {
                NetworkThreadControlMessage::RegisterTransmitter { consumer } => {
                    transmitters.push(NetworkThreadTransmitterNodeData { consumer });
                }
                NetworkThreadControlMessage::RegisterReceiver {
                    producer,
                    node_net_id,
                } => receivers.push(NetworkThreadReceiverNodeData {
                    producer,
                    node_net_id,
                }),
            }
        }

        // Then, pull encoded bytes for every transmitter and transmit that data
        transmitters.retain_mut(|transmitter| {
            match transmitter.consumer.pop() {
                Ok(message) => {
                    // Receiving machine also needs to know node_net_id to properly route, include that as well
                    let final_message = SentNetworkMessage {
                        node_net_id: message.node_net_id,
                        encoded: message.encoded_data[0..message.encoded_len].to_vec(),
                    };

                    let serialized = match bincode::serde::encode_to_vec(
                        final_message,
                        bincode::config::standard(),
                    ) {
                        Ok(serialized) => serialized,
                        Err(e) => {
                            error!(
                                "Failed to encode final network message while transmitting: {e}"
                            );
                            // We skip this one
                            return true;
                        }
                    };

                    // Silently fail. TODO: Change?
                    let _ = transport.send(&serialized, &message.address);
                    true
                }
                Err(_) => {
                    // Buffer is empty, if we also are abandoned, filter from transmitters we're tracking (The transmitter node producing has been removed)
                    !transmitter.consumer.is_abandoned()
                }
            }
        });

        // Receive all messages for transport
        let Ok(messages) = transport.try_receive() else {
            // Silently fail. TODO: Change?
            continue;
        };

        let sent_messages: Vec<SentNetworkMessage> = messages
            .into_iter()
            .filter_map(|x| {
                match bincode::serde::decode_from_slice(&x.1, bincode::config::standard()) {
                    Ok(serialized) => Some(serialized.0),
                    Err(e) => {
                        error!("Failed to encode final network message while transmitting: {e}");
                        // We skip this one
                        None
                    }
                }
            })
            .collect();

        for sent_message in sent_messages {
            receivers.retain_mut(|receiver| {
                if receiver.producer.is_abandoned() {
                    return false;
                }

                if receiver.node_net_id != sent_message.node_net_id {
                    return true;
                }

                let len = min(
                    sent_message.encoded.len(),
                    TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE,
                );

                let message = ReceiverNodeNetworkThreadMessage {
                    encoded_data: {
                        let mut encoded_data = [0u8; TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE];
                        encoded_data.copy_from_slice(&sent_message.encoded);
                        encoded_data
                    },
                    encoded_len: len,
                };

                match receiver.producer.push(message) {
                    Ok(_) => true,
                    Err(_) => {
                        // Buffer is full
                        true
                    }
                }
            });
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SentNetworkMessage {
    node_net_id: u32,
    encoded: Vec<u8>,
}

struct NetworkThreadTransmitterNodeData<T>
where
    T: NetworkNodeTransport,
{
    consumer: rtrb::Consumer<TransmitterNodeNetworkThreadMessage<T>>,
}

struct NetworkThreadReceiverNodeData {
    producer: rtrb::Producer<ReceiverNodeNetworkThreadMessage>,
    node_net_id: u32,
}

pub(crate) struct TransmitterNodeNetworkThreadMessage<T>
where
    T: NetworkNodeTransport,
{
    pub(crate) address: T::Addr,
    pub(crate) node_net_id: u32,
    pub(crate) encoded_data: [u8; TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE],
    pub(crate) encoded_len: usize,
}

pub(crate) struct ReceiverNodeNetworkThreadMessage {
    pub(crate) encoded_data: [u8; TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE],
    pub(crate) encoded_len: usize,
}

pub(crate) enum NetworkThreadControlMessage<T>
where
    T: NetworkNodeTransport,
{
    RegisterTransmitter {
        consumer: rtrb::Consumer<TransmitterNodeNetworkThreadMessage<T>>,
    },
    RegisterReceiver {
        producer: rtrb::Producer<ReceiverNodeNetworkThreadMessage>,
        node_net_id: u32,
    },
}

pub(crate) struct NetworkThreadRegistryKey<T>(PhantomData<T>);

impl<T> typemap_rev::TypeMapKey for NetworkThreadRegistryKey<T>
where
    T: NetworkNodeTransport,
{
    type Value = Sender<NetworkThreadControlMessage<T>>;
}

lazy_static! {
    /// Shared transmitter to send control messages to the networking thread
    pub(crate) static ref NETWORK_THREAD_REGISTRY: Mutex<TypeMap> = Mutex::new(TypeMap::custom());
}
