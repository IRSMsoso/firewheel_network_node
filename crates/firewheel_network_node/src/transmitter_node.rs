use crate::constants::{
    TRANSMITTER_NODE_NETWORK_MESSAGE_RINGBUFFER_SIZE, TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE,
};
use crate::network_io::{
    network_thread, NetworkThreadControlMessage, NetworkThreadRegistryKey,
    TransmitterNodeNetworkMessage, NETWORK_THREAD_REGISTRY,
};
use crate::transport::NetworkNodeTransport;
use firewheel_core::channel_config::{ChannelConfig, ChannelCount};
use firewheel_core::diff::{Diff, Patch};
use firewheel_core::node::{
    AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, NodeError,
    ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
};
use opus_rs::{Application, OpusEncoder};
use std::fmt::{Display, Formatter};
use std::sync::mpsc;
use std::thread::spawn;
use thiserror::Error;

pub struct NetworkTransmitterNodeConfig<T>
where
    T: NetworkNodeTransport,
{
    /// The number of channels of input to pass to the Opus Encoder. Must match the receiver node
    channels: usize,
    /// Type of application passed to the Opus Encoder. Must match the receiver node
    application: Application,
    /// The configuration for the transport used to send data
    transport_config: T::Config,
}

impl<T> Default for NetworkTransmitterNodeConfig<T>
where
    T: NetworkNodeTransport,
{
    fn default() -> Self {
        Self {
            channels: 1,
            application: Application::Audio,
            transport_config: Default::default(),
        }
    }
}

/// A node that sends data over an arbitrary networking transport to receiver nodes
#[derive(Diff, Patch, Debug, PartialEq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Component))]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
//#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NetworkTransmitterNode<T>
where
    T: NetworkNodeTransport,
{
    /// The network address of the node to send audio to
    address: T::Addr,
    /// The network identifier of the node to send audio to
    node_net_id: u32,
}

impl<T> NetworkTransmitterNode<T>
where
    T: NetworkNodeTransport,
{
    pub fn new(address: T::Addr, node_net_id: u32) -> Self {
        Self {
            address,
            node_net_id,
        }
    }
}

#[derive(Debug, Copy, Clone, Error)]
pub struct OpusError(&'static str);

impl Display for OpusError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Opus Error: {}", self.0)
    }
}

impl<T> AudioNode for NetworkTransmitterNode<T>
where
    T: NetworkNodeTransport,
{
    type Configuration = NetworkTransmitterNodeConfig<T>;

    fn info(&self, configuration: &Self::Configuration) -> Result<AudioNodeInfo, NodeError> {
        Ok(AudioNodeInfo::new().channel_config(ChannelConfig {
            num_inputs: configuration.channels.into(),
            num_outputs: ChannelCount::ZERO,
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
                // Initialize actual transport for this transpor type
                let transport = T::construct(&configuration.transport_config)?;

                // Initialize control channel for this transport type
                let (sender, receiver) = mpsc::channel::<NetworkThreadControlMessage<T>>();

                spawn(|| {
                    network_thread(transport, receiver);
                });

                network_thread_registry_lock.insert::<NetworkThreadRegistryKey<T>>(sender.clone());

                sender
            }
            Some(sender) => sender.clone(),
        };

        let (producer, consumer) =
            rtrb::RingBuffer::new(TRANSMITTER_NODE_NETWORK_MESSAGE_RINGBUFFER_SIZE);

        sender
            .send(NetworkThreadControlMessage::RegisterTransmitter { consumer })
            .expect("Network thread should never stop");

        Ok(NetworkTransmitterNodeProcessor::<T> {
            encoder: OpusEncoder::new(
                cx.stream_info.sample_rate.get() as i32,
                configuration.channels,
                configuration.application,
            )
            .map_err(|e| OpusError(e))?,
            opus_channels: configuration.channels,
            producer,
            encoding_buffer: [0; TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE],
            address: self.address.clone(),
            node_net_id: self.node_net_id,
        })
    }
}

struct NetworkTransmitterNodeProcessor<T>
where
    T: NetworkNodeTransport,
{
    /// The opus encoder state
    encoder: OpusEncoder,
    /// The number of opus channels to use, Mono or Stereo
    opus_channels: usize,
    /// The producer side of the network thread communication ringbuffer
    producer: rtrb::Producer<TransmitterNodeNetworkMessage<T>>,
    /// Encoding buffer
    encoding_buffer: [u8; TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE],

    // Patched
    /// The network address of the node to send audio to
    address: T::Addr,
    /// The network identifier of the node to send audio to
    node_net_id: u32,
}

impl<T> AudioNodeProcessor for NetworkTransmitterNodeProcessor<T>
where
    T: NetworkNodeTransport,
{
    fn process(
        &mut self,
        info: &ProcInfo,
        buffers: ProcBuffers,
        extra: &mut ProcExtra,
    ) -> ProcessStatus {
        // Our processor inputs must equal our opus configuration
        debug_assert_eq!(self.opus_channels, buffers.inputs.len());

        let len = match buffers.inputs.len() {
            1 => {
                let Ok(len) =
                    self.encoder
                        .encode(buffers.inputs[0], info.frames, &mut self.encoding_buffer)
                else {
                    return ProcessStatus::Bypass;
                };

                len
            }
            2 => {
                // For stereo, we must interleave the channels for opus. Use scratch buffers provided by firewheel to do this.
                let num_samples = buffers.inputs[0].len();

                assert!(extra.scratch_buffers.first_mut().len() >= num_samples * 2);

                // Assumption: buffers.inputs[0].len() == buffers.inputs[1].len()
                assert_eq!(buffers.inputs[0].len(), buffers.inputs[1].len());

                for sample_index in 0..buffers.inputs[0].len() {
                    extra.scratch_buffers.first_mut()[sample_index * 2] =
                        buffers.inputs[0][sample_index];
                    extra.scratch_buffers.first_mut()[sample_index * 2 + 1] =
                        buffers.inputs[1][sample_index];
                }

                let Ok(len) = self.encoder.encode(
                    &extra.scratch_buffers.first()[0..(num_samples * 2)],
                    info.frames,
                    &mut self.encoding_buffer,
                ) else {
                    return ProcessStatus::Bypass;
                };

                len
            }
            _ => {
                // Opus can only support mono or stereo
                return ProcessStatus::Bypass;
            }
        };

        // Push our encoded data to the networking thread via ringbuffer
        // If the ringbuffer is full, we do nothing and allow network thread to catchup at the cost of losing some audio
        // TODO: Is this a valid strategy?
        let _ = self.producer.push(TransmitterNodeNetworkMessage {
            address: self.address.clone(),
            node_net_id: self.node_net_id,
            encoded_data: self.encoding_buffer,
            encoded_len: len,
        });

        ProcessStatus::Bypass
    }
}
