use crate::transport::NetworkNodeTransport;

pub(crate) fn network_thread<T>(address: T::Addr)
where
    T: NetworkNodeTransport,
{
}
