use std::fmt::{Display, Formatter};
use crate::message::NetworkNodeMessage;
use crate::network_io::SendMessage;
use crate::transport::NetworkNodeTransport;
use firewheel_core::channel_config::{ChannelConfig, ChannelCount};
use firewheel_core::diff::{Diff, Patch, PatchError};
use firewheel_core::event::ParamData;
use firewheel_core::node::{
    AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, NodeError,
    ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use opus_rs::{Application, OpusEncoder};
use thiserror::Error;

struct NetworkTransmitterNodeConfig<T>
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
            channels: Channels::Mono,
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
    address: T::Addr,
    /// The network identifier of the node to send audio to
    node_net_id: u32,
    phantom_data: PhantomData<T>,
}

impl<T> Patch for NetworkTransmitterNode<T>
where
    T: NetworkNodeTransport,
{
    type Patch = ();

    fn patch(data: &ParamData, path: &[u32]) -> Result<Self::Patch, PatchError> {
        todo!()
    }

    fn apply(&mut self, patch: Self::Patch) {
        todo!()
    }
}

impl<T> NetworkTransmitterNode<T>
where
    T: NetworkNodeTransport,
{
    pub fn new(address: T::Addr, node_net_id: u32) -> Self {
        Self {
            address,
            node_net_id,
            phantom_data: Default::default(),
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
        Ok(NetworkTransmitterNodeProcessor::<T> {
            transport: T::construct(&configuration.transport_config)?,
            encoder: OpusEncoder::new(
                cx.stream_info.sample_rate.get() as i32,
                configuration.channels,
                configuration.application,
            ).map_err(|e| OpusError(e))?,
            channels: configuration.channels,
            address: self.address.clone(),
        })
    }
}

pub struct NetworkTransmitterNodeDestination<T>
where
    T: NetworkNodeTransport,
{
    /// The address to send audio data to
    address: Arc<Mutex<T::Addr>>,
    /// The network identifier of the node to send audio to
    node_net_id: u32,
}

struct NetworkTransmitterNodeProcessor<T>
where
    T: NetworkNodeTransport,
{
    /// The network transport to use to actually send the data
    transport: T,
    /// The opus encoder state
    encoder: OpusEncoder,
    /// The number of opus channels to use, Mono or Stereo
    channels: usize,
    /// The destination of the audio data
    address: Arc<Mutex<T::Addr>>,
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
        let mut encoded = [0; 256];

        let len = match buffers.inputs.len() {
            1 => {
                let Ok(len) = self.encoder.encode(buffers.inputs[0], info.frames, &mut encoded) else {
                    return ProcessStatus::Bypass;
                };

                len
            }
            2 => {
                // For stereo, we must interleave the channels for opus. Use scratch buffers to do this.
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
                    &mut encoded,
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

        let message = SendMessage {
            address: self.address,
            node_net_id: self.node_net_id,
            encoded,
        };

        let Ok(serialized) = bincode::serde::encode_to_vec(message, bincode::config::standard())
        else {
            return ProcessStatus::Bypass;
        };

        self.transport.send(&serialized, &self.address);

        ProcessStatus::Bypass
    }
}
