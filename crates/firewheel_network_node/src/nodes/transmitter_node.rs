use crate::constants::{
    TRANSMITTER_NODE_NETWORK_MESSAGE_RINGBUFFER_SIZE, TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE,
    TRANSMITTER_NODE_OPUS_FRAME_BUFFER_SIZE,
};
use crate::network_io::{
    network_thread, NetworkThreadControlMessage, NetworkThreadRegistryKey,
    TransmitterNodeNetworkThreadMessage, NETWORK_THREAD_REGISTRY,
};
pub use crate::nodes::shared::{OpusApplicationType, OpusError};
use crate::transport::NetworkNodeTransport;
use circular_buffer::CircularBuffer;
use firewheel_core::channel_config::{ChannelConfig, ChannelCount};
use firewheel_core::diff::{Diff, Patch};
use firewheel_core::node::{
    AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, NodeError,
    ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
};
use log::warn;
use opus_rs::{Application, OpusEncoder};
use std::sync::mpsc;

pub struct NetworkTransmitterNodeConfig<T>
where
    T: NetworkNodeTransport,
{
    /// The number of channels of input to pass to the Opus Encoder. Must match the receiver node
    pub channels: usize,
    /// Type of application passed to the Opus Encoder. Must match the receiver node
    pub opus_application_type: OpusApplicationType,
    /// The configuration for the transport used to send/receive data
    pub transport_config: T::Config,
}

impl<T> Default for NetworkTransmitterNodeConfig<T>
where
    T: NetworkNodeTransport,
{
    fn default() -> Self {
        Self {
            channels: 1,
            opus_application_type: OpusApplicationType::Audio,
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
            .send(NetworkThreadControlMessage::RegisterTransmitter { consumer })
            .expect("Network thread should never stop");

        Ok(NetworkTransmitterNodeProcessor::<T> {
            encoder: OpusEncoder::new(
                cx.stream_info.sample_rate.get() as i32,
                configuration.channels,
                match configuration.opus_application_type {
                    OpusApplicationType::Voip => Application::Voip,
                    OpusApplicationType::Audio => Application::Audio,
                    OpusApplicationType::RestrictedLowDelay => Application::RestrictedLowDelay,
                },
            )
            .map_err(OpusError)?,
            opus_channels: configuration.channels,
            producer,
            encoding_buffer: [0; TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE],
            interleaving_buffer: match configuration.channels {
                1 => None,
                2 => {
                    // Preallocate interleaving space
                    Some(vec![
                        0.0;
                        cx.stream_info.max_block_frames.get() as usize * 2
                    ])
                }
                _ => unreachable!(),
            },
            opus_frame_buffer: [0.0; TRANSMITTER_NODE_OPUS_FRAME_BUFFER_SIZE * 2],
            opus_frame_buffer_len: 0,
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
    producer: rtrb::Producer<TransmitterNodeNetworkThreadMessage<T>>,
    /// Encoding buffer - The buffer that opus frames are encoded into
    encoding_buffer: [u8; TRANSMITTER_NODE_OPUS_ENCODING_BUFFER_SIZE],
    /// Interleaving buffer - Used to interleave two channels into one for the opus encoder to process
    interleaving_buffer: Option<Vec<f32>>,
    /// Opus frame buffer - buffers input into the transmitter node until it hits a certain frame size compatible with the opus codec
    opus_frame_buffer: [f32; TRANSMITTER_NODE_OPUS_FRAME_BUFFER_SIZE * 2],
    /// Opus frame buffer index
    opus_frame_buffer_len: usize,

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
                let interleaving_buffer = self.interleaving_buffer.as_mut().expect(
                    "If two channels are presence, should have allocated interleaving buffer",
                );

                // For stereo, we must interleave the channels for opus.
                let num_samples = buffers.inputs[0].len();

                debug_assert!(interleaving_buffer.len() >= num_samples * 2);

                // Assumption: buffers.inputs[0].len() == buffers.inputs[1].len()
                assert_eq!(buffers.inputs[0].len(), buffers.inputs[1].len());

                for sample_index in 0..buffers.inputs[0].len() {
                    interleaving_buffer[sample_index * 2] = buffers.inputs[0][sample_index];
                    interleaving_buffer[sample_index * 2 + 1] = buffers.inputs[1][sample_index];
                }

                let mut len = 0;

                for sample_index in 0..(num_samples * 2) {
                    self.opus_frame_buffer[self.opus_frame_buffer_len] =
                        interleaving_buffer[sample_index];

                    self.opus_frame_buffer_len += 1;

                    if self.opus_frame_buffer_len == (TRANSMITTER_NODE_OPUS_FRAME_BUFFER_SIZE * 2) {
                        len += match self.encoder.encode(
                            &self.opus_frame_buffer,
                            TRANSMITTER_NODE_OPUS_FRAME_BUFFER_SIZE,
                            &mut self.encoding_buffer[len..],
                        ) {
                            Ok(len) => len,
                            Err(e) => {
                                warn!("Opus Encoding Error: {e}");
                                self.opus_frame_buffer_len = 0;
                                return ProcessStatus::Bypass;
                            }
                        };

                        self.opus_frame_buffer_len = 0;
                    }
                }

                len
            }
            _ => {
                unreachable!()
            }
        };

        // Push our encoded data to the networking thread via ringbuffer
        // If the ringbuffer is full, we do nothing and allow network thread to catchup at the cost of losing some audio
        // TODO: Is this a valid strategy?
        let _ = self.producer.push(TransmitterNodeNetworkThreadMessage {
            address: self.address.clone(),
            node_net_id: self.node_net_id,
            encoded_data: self.encoding_buffer,
            encoded_len: len,
        });

        ProcessStatus::Bypass
    }
}
