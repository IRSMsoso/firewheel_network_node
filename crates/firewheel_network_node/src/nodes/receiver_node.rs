use crate::constants::{
    RECEIVER_NODE_BUFFER_SIZE, TRANSMITTER_NODE_NETWORK_MESSAGE_RINGBUFFER_SIZE,
    TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE,
};
use crate::network_io::{
    network_thread, NetworkThreadControlMessage, NetworkThreadRegistryKey,
    ReceiverNodeNetworkThreadMessage, NETWORK_THREAD_REGISTRY,
};
use crate::nodes::shared::OpusError;
use crate::transport::NetworkNodeTransport;
use circular_buffer::CircularBuffer;
use firewheel_core::channel_config::{ChannelConfig, ChannelCount};
use firewheel_core::diff::{Diff, Patch};
use firewheel_core::node::{
    AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, NodeError,
    ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
};
use opus_rs::OpusDecoder;
use std::marker::PhantomData;
use std::sync::mpsc;

pub struct NetworkReceiverNodeConfig<T>
where
    T: NetworkNodeTransport,
{
    /// The number of channels of output to pass to the Opus Encoder. Must match the transmitter node
    pub channels: usize,
    /// The configuration for the transport used to send/receive data
    pub transport_config: T::Config,
}

impl<T> Default for NetworkReceiverNodeConfig<T>
where
    T: NetworkNodeTransport,
{
    fn default() -> Self {
        Self {
            channels: 1,
            transport_config: Default::default(),
        }
    }
}

/// A node that receives data over an arbitrary networking transport from transmitter nodes
#[derive(Diff, Patch, Debug, PartialEq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Component))]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
//#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NetworkReceiverNode<T>
where
    T: NetworkNodeTransport,
{
    /// The network identifier of this receiver node. Cannot change.
    pub node_net_id: u32,

    _phantom: PhantomData<T>,
}

impl<T> NetworkReceiverNode<T>
where
    T: NetworkNodeTransport,
{
    pub fn new(node_net_id: u32) -> Self {
        Self {
            node_net_id,
            _phantom: PhantomData,
        }
    }
}

impl<T> AudioNode for NetworkReceiverNode<T>
where
    T: NetworkNodeTransport,
{
    type Configuration = NetworkReceiverNodeConfig<T>;

    fn info(&self, configuration: &Self::Configuration) -> Result<AudioNodeInfo, NodeError> {
        Ok(AudioNodeInfo::new().channel_config(ChannelConfig {
            num_inputs: ChannelCount::ZERO,
            num_outputs: configuration.channels.into(),
        }))
    }

    fn construct_processor(
        &self,
        configuration: &Self::Configuration,
        cx: ConstructProcessorContext,
    ) -> Result<impl AudioNodeProcessor, NodeError> {
        // Start the global networking thread if it hasn't already been started

        let mut network_thread_registry_lock = NETWORK_THREAD_REGISTRY.lock().unwrap();

        let sender = match network_thread_registry_lock.get::<NetworkThreadRegistryKey<T>>() {
            None => {
                // TODO: Initialize the transport outside of the construction of the processor, in some method that the user is responsible for calling beforehand. This removes the issue of "the first node to activate the spawning of the thread decies the config and all other nodes have redundant/unused transport config data
                // Initialize actual transport for this transport type
                let transport = T::construct(&configuration.transport_config)?;

                // Initialize control channel for this transport type
                let (sender, receiver) = mpsc::channel::<NetworkThreadControlMessage<T>>();

                std::thread::Builder::new()
                    .name(T::NAME.to_string())
                    .spawn(|| {
                        network_thread(transport, receiver);
                    })?;

                network_thread_registry_lock.insert::<NetworkThreadRegistryKey<T>>(sender.clone());

                sender
            }
            Some(sender) => sender.clone(),
        };

        let (producer, consumer) =
            rtrb::RingBuffer::new(TRANSMITTER_NODE_NETWORK_MESSAGE_RINGBUFFER_SIZE);

        sender
            .send(NetworkThreadControlMessage::RegisterReceiver {
                producer,
                node_net_id: self.node_net_id,
            })
            .expect("Network thread should never stop");

        Ok(NetworkReceiverNodeProcessor {
            decoder: OpusDecoder::new(
                cx.stream_info.sample_rate.get() as i32,
                configuration.channels,
            )
            .map_err(OpusError)?,
            opus_channels: configuration.channels,
            consumer,
            buffer: CircularBuffer::new(),
        })
    }
}

struct NetworkReceiverNodeProcessor {
    /// The opus encoder state
    decoder: OpusDecoder,
    /// The number of opus channels to use, Mono or Stereo
    opus_channels: usize,
    /// The consumer side of the network thread communication ringbuffer
    consumer: rtrb::Consumer<ReceiverNodeNetworkThreadMessage>,
    /// Buffer used to store decoded samples until they're consumed by the receiver node
    buffer: CircularBuffer<RECEIVER_NODE_BUFFER_SIZE, f32>,
}

impl AudioNodeProcessor for NetworkReceiverNodeProcessor {
    fn process(
        &mut self,
        info: &ProcInfo,
        buffers: ProcBuffers,
        extra: &mut ProcExtra,
    ) -> ProcessStatus {
        // Our processor inputs must equal our opus configuration
        debug_assert_eq!(self.opus_channels, buffers.outputs.len());

        // First, receive anything from network thread
        while let Ok(message) = self.consumer.pop() {
            let mut buf = [0f32; TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE];
            let len =
                match self
                    .decoder
                    .decode(&message.encoded_data, message.encoded_len, &mut buf)
                {
                    Ok(len) => len,
                    Err(_) => {
                        continue;
                    }
                };

            self.buffer.extend_from_slice(&buf[0..len]);
        }

        match buffers.outputs.len() {
            1 => {
                let mut index = 0;
                while let Some(value) = self.buffer.pop_front() {
                    buffers.outputs[0][index] = value;
                    index += 1;
                }
            }
            2 => {
                let mut index = 0;
                while let (Some(left), Some(right)) =
                    (self.buffer.pop_front(), self.buffer.pop_front())
                // This will skip left channel if right isn't also there, but w/e, that shouldn't happen anyway
                {
                    buffers.outputs[0][index] = left;
                    buffers.outputs[1][index] = right;
                    index += 1;
                }
            }
            _ => {
                unreachable!()
            }
        };

        ProcessStatus::OutputsModified
    }
}
