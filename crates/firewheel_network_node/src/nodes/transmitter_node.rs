use crate::constants::{
    ENCODED_OPUS_BUFFER_SIZE, NETWORK_MESSAGE_RINGBUFFER_SIZE, OPUS_FRAME_BUFFER_SIZE,
};
use crate::network_io::{
    network_thread, NetworkThreadControlMessage, NetworkThreadRegistryKey,
    TransmitterNodeNetworkThreadMessage, NETWORK_THREAD_REGISTRY,
};
use crate::nodes::shared::{OpusApplicationType, OpusChannels};
use crate::transport::NetworkNodeTransport;
use firewheel_core::channel_config::{ChannelConfig, ChannelCount};
use firewheel_core::diff::{Diff, Patch};
use firewheel_core::node::{
    AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, NodeError,
    ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
};
use opus2::Encoder;
use rubato::{Fft, FixedSync, Resampler};
use std::fmt::Write;
use std::sync::mpsc;

pub struct NetworkTransmitterNodeConfig<T>
where
    T: NetworkNodeTransport,
{
    /// The number of channels of input to pass to the Opus Encoder. Must match the receiver node
    pub channels: OpusChannels,
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
            channels: OpusChannels::Mono,
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
        Ok(AudioNodeInfo::new()
            .debug_name("Network Transmitter")
            .channel_config(ChannelConfig {
                num_inputs: match configuration.channels {
                    OpusChannels::Mono => ChannelCount::MONO,
                    OpusChannels::Stereo => ChannelCount::STEREO,
                },
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
                // TODO: Initialize the transport outside of the construction of the processor, in some method that the user is responsible for calling beforehand. This removes the issue of "the first node to activate the spawning of the thread decides the config and all other nodes have redundant/unused transport config data
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

        let (producer, consumer) = rtrb::RingBuffer::new(NETWORK_MESSAGE_RINGBUFFER_SIZE);

        sender
            .send(NetworkThreadControlMessage::RegisterTransmitter { consumer })
            .expect("Network thread should never stop");

        let fft = match cx.stream_info.sample_rate.get() {
            48000 => None,
            sample_rate => Some(Fft::new(
                cx.stream_info.sample_rate.get() as usize,
                48000,
                256,
                1,
                match configuration.channels {
                    OpusChannels::Mono => 1,
                    OpusChannels::Stereo => 2,
                },
                FixedSync::Both,
            )?),
        };

        Ok(NetworkTransmitterNodeProcessor::<T> {
            encoder: Encoder::new(
                cx.stream_info.sample_rate.get(),
                configuration.channels.into(),
                configuration.opus_application_type.into(),
            )?,
            producer,
            encoding_buffer: [0; ENCODED_OPUS_BUFFER_SIZE],
            opus_frame_buffer: vec![
                0.0f32;
                match configuration.channels {
                    OpusChannels::Mono => OPUS_FRAME_BUFFER_SIZE,
                    OpusChannels::Stereo => OPUS_FRAME_BUFFER_SIZE * 2,
                }
            ],
            opus_frame_buffer_len: 0,
            resampler: fft.map(|fft| FftResampler {
                resampler_input: vec![0.0f32; fft.input_frames_max()],
                resampler_output: vec![0.0f32; fft.output_frames_max()],
                fft,
            }),
            address: self.address.clone(),
            node_net_id: self.node_net_id,
        })
    }
}

struct FftResampler {
    /// Rubato resampler used to coerce the sample rate of the firewheel graph to a sample rate that opus supports
    fft: Fft<f32>,
    /// The pre-allocated input buffer for the resampler
    resampler_input: Vec<f32>,
    /// The pre-allocated output buffer for the resampler
    resampler_output: Vec<f32>,
}

struct NetworkTransmitterNodeProcessor<T>
where
    T: NetworkNodeTransport,
{
    /// The opus encoder state
    encoder: Encoder,
    /// The producer side of the network thread communication ringbuffer
    producer: rtrb::Producer<TransmitterNodeNetworkThreadMessage<T>>,
    /// Encoding buffer - The buffer that opus frames are encoded into
    encoding_buffer: [u8; ENCODED_OPUS_BUFFER_SIZE],
    /// Opus frame buffer - buffers input into the transmitter node until it hits a certain frame size compatible with the opus codec
    opus_frame_buffer: Vec<f32>,
    /// Opus frame buffer index
    opus_frame_buffer_len: usize,

    /// Resampling info, including the actual resampler, the input buffer, and the output buffer
    resampler: Option<FftResampler>,

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
        _info: &ProcInfo,
        buffers: ProcBuffers,
        extra: &mut ProcExtra,
    ) -> ProcessStatus {
        let num_samples = buffers.inputs[0].len();

        for sample_index in 0..num_samples {
            match buffers.inputs.len() {
                1 => {
                    // In the mono case, we simply copy from the processor input buffer and increment the buffer len
                    match &mut self.resampler {
                        Some(fft_resampler) => {
                            fft_resampler.fft.process_into_buffer();
                        }
                        None => {
                            self.opus_frame_buffer[self.opus_frame_buffer_len] =
                                buffers.inputs[0][sample_index];

                            self.opus_frame_buffer_len += 1;
                        }
                    }
                }
                2 => {
                    // In the stereo case, we interleave the two input buffers for the opus encoder
                    self.opus_frame_buffer[self.opus_frame_buffer_len] =
                        buffers.inputs[0][sample_index];
                    self.opus_frame_buffer[self.opus_frame_buffer_len + 1] =
                        buffers.inputs[1][sample_index];

                    self.opus_frame_buffer_len += 2;
                }
                _ => unreachable!(),
            }

            // If we've reached our max capacity
            if self.opus_frame_buffer_len == self.opus_frame_buffer.len() {
                match self
                    .encoder
                    .encode_float(&self.opus_frame_buffer, &mut self.encoding_buffer)
                {
                    Ok(len) => {
                        // Push our encoded data to the networking thread via ringbuffer
                        // If the ringbuffer is full, we do nothing and allow network thread to catchup at the cost of losing some audio
                        // TODO: Is this a valid strategy?
                        if self
                            .producer
                            .push(TransmitterNodeNetworkThreadMessage {
                                address: self.address.clone(),
                                node_net_id: self.node_net_id,
                                encoded_data: self.encoding_buffer,
                                encoded_len: len,
                            })
                            .is_err()
                        {
                            let _ = extra
                                .logger
                                .try_error("Transmitter node -> network thread producer is full");
                        }
                    }
                    Err(e) => {
                        let _ = extra.logger.try_error_with(|string| {
                            let _ = write!(string, "Opus encoding failed: {}", e);
                        });

                        self.opus_frame_buffer_len = 0;
                        return ProcessStatus::ClearAllOutputs;
                    }
                };

                self.opus_frame_buffer_len = 0;
            }
        }

        ProcessStatus::ClearAllOutputs
    }
}
