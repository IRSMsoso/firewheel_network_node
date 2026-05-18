use firewheel::cpal::CpalStream;
use firewheel::nodes::sampler::{SamplerNode, SamplerState};
use firewheel::FirewheelContext;
use firewheel_network_node::nodes::receiver_node::{
    NetworkReceiverNode, NetworkReceiverNodeConfig,
};
use firewheel_network_node::nodes::shared::{OpusApplicationType, OpusChannels};
use firewheel_network_node::nodes::transmitter_node::{
    NetworkTransmitterNode, NetworkTransmitterNodeConfig,
};
use firewheel_network_node::transport::udp_socket_transport::{
    UdpSocketTransport, UdpSocketTransportConfig,
};
use log::error;
use std::net::Ipv4Addr;
use std::time::Duration;

const UPDATE_INTERVAL: Duration = Duration::from_millis(15);

fn main() {
    simple_logger::SimpleLogger::new().env().init().unwrap();

    // --- Start the context and get the sample rate of the audio stream. ----------------

    let mut cx = FirewheelContext::new(Default::default());
    let mut stream = CpalStream::new(&mut cx, Default::default()).unwrap();

    println!("Sample rate: {}", cx.stream_info().unwrap().sample_rate);

    let sample_rate = cx.stream_info().unwrap().sample_rate;

    let mut sampler_node = SamplerNode::default();

    let sampler_id = cx
        .add_node(sampler_node, None)
        .expect("Sampler node should construct without error");

    let transmitter_node: NetworkTransmitterNode<UdpSocketTransport> =
        NetworkTransmitterNode::new(Ipv4Addr::new(127, 0, 0, 1).into(), 0);

    let transmitter_id = cx
        .add_node(
            transmitter_node,
            Some(NetworkTransmitterNodeConfig {
                channels: OpusChannels::Mono,
                opus_application_type: OpusApplicationType::Audio,
                transport_config: UdpSocketTransportConfig { port: 1680 },
            }),
        )
        .unwrap();

    let receiver_node: NetworkReceiverNode<UdpSocketTransport> = NetworkReceiverNode::new(0);

    let receiver_id = cx
        .add_node(
            receiver_node,
            Some(NetworkReceiverNodeConfig {
                channels: OpusChannels::Mono,
                transport_config: UdpSocketTransportConfig { port: 1680 },
            }),
        )
        .unwrap();

    let graph_out_id = cx.graph_out_node_id();

    // Connect sampler to transmitter
    cx.connect(sampler_id, transmitter_id, &[(0, 0), (1, 0)], false)
        .unwrap();

    // Connect receiver to output
    cx.connect(receiver_id, graph_out_id, &[(0, 0), (0, 1)], true)
        .unwrap();

    // --- Simulated update loop ---------------------------------------------------------
    loop {
        // Update the firewheel context.
        // This must be called regularly (i.e. once every frame).
        if let Err(e) = cx.update() {
            error!("{:?}", &e);
        }

        // Log any stream errors/warnings that have occurred.
        stream.log_status();

        // The stream has stopped unexpectedly (i.e. the user has
        // unplugged their headphones.)
        //
        // Typically, you should start a new stream as soon as
        // possible to resume processing (even if it's a dummy
        // output device).
        //
        // In this example we just quit the application.
        if !stream.all_streams_ok() {
            break;
        }

        std::thread::sleep(UPDATE_INTERVAL);
    }

    println!("finished");
}
