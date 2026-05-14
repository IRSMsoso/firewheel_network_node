// /// Sent from the transmitter nodes to the network thread to be sent to a certain node at a certain network address
// pub(crate) struct TransmitterMessage<T>
// where
//     T: NetworkNodeTransport,
// {
//     /// The network address of the node that will receive this message
//     pub(crate) address: T::Addr,
//     /// The net identifier of the node that will receive this message
//     pub(crate) node_net_id: u32,
//     /// The opus-encoded bytes of audio
//     pub(crate) encoded: [u8; 256],
// }
//
// pub(crate) struct NetworkNodeThreadData<T>
// where
//     T: NetworkNodeTransport,
// {
//     transmitter_nodes: HashSet<Consumer<TransmitterMessage<T>>>,
//     receiver_nodes: HashSet<Producer<u8>>,
// }
//
// pub(crate) struct NetworkNodeSharedState<T>
// where
//     T: NetworkNodeTransport,
// {
//     /// Contains ringbuffer consumers the networking thread uses to receive data from audio nodes
//     thread_data: Arc<Mutex<NetworkNodeThreadData<T>>>,
// }
//
// impl<T> NetworkNodeSharedState<T> {
//     pub(crate) fn register_receiver_node() {}
// }
