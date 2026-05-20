# Status

This crate is not yet ready to use

# Firewheel Network Node

This is a pair of [firewheel](https://github.com/BillyDM/firewheel) nodes which together can be
used to send audio data over any arbitrary network. Audio data is encoded via the opus codec, resulting in
significantly reduced bandwidth usage comparable to other modern VOIP services like Discord.

## Network Transports

This crate is network transport agnostic. You can supply your own transport by implementing
the [NetworkNodeTransport](crates/firewheel_network_node/src/transport/mod.rs) trait. The only requirements is that the
send and receive methods don't block and that the transport behaves "like udp". That is, that it "fire and forgets" and
it doesn't have ordering guarantees. The lack of ordering requirement isn't *as* strict as the other requirements, but
forced ordering (like when using TCP), can introduce delay and the OPUS codec is designed to withstand out-of-order
delivery, so your mileage may vary.

Additionally, this crate must have full access to the transport. That is to say, any data that it reads or writes must
be exclusive to this crate. It interprets everything coming in as encoded opus bytes. To use this crate alongside other
networking (for example networked gameplay data for games), you must abstract them into separate streams. For example,
in the built-in `UdpSocketTransport` case, using an additional separate port for audio data. Or for the
`SteamNetworkingMessagesTransport` case, using a
different [channel](https://partner.steamgames.com/doc/api/ISteamNetworkingMessages) than your other gameplay network
traffic.

I can see this crate being integrated into other crates that provide transports
like [aeronet](https://github.com/aecsocket/aeronet) and being used alongside crates
like [bevy_replicon](https://github.com/simgine/bevy_replicon), [lightyear](https://github.com/cBournhonesque/lightyear),
and [bevy_ggrs](https://github.com/gschup/bevy_ggrs). The transport trait is actually very similar to and inspired by
the [ggrs](https://github.com/gschup/ggrs) crate.

### Current Network Transports

The crate contains two transports.

- [UDP Sockets](crates/firewheel_network_node/src/transport/udp_socket_transport.rs), which uses plain UDP sockets
- [Steam Networking](crates/firewheel_network_node/src/transport/steam_transport.rs), which uses
  the [Steam Networking Messages](https://partner.steamgames.com/doc/api/ISteamNetworkingMessages) API to send UDP-like
  messages over steam's relays, implemented via the [steamworks](https://github.com/Noxime/steamworks-rs) crate

If these two do not fit your needs, it is fairly easy to implement your own. Take a look at how the ones above are
implemented for reference.

## Audio Graph Constraints

### Transmitter Node

- Has exactly 1 or 2 inputs and 0 outputs

### Receiver Node

- Has exactly 0 inputs and 1 or 2 outputs

## Security

This crate employs no encryption or other security measures. Use a secure transport and hook this crate into that.

## Technical Details

This crate spins up 1 networking thread per Transport. This networking thread is responsible for receiving encoded audio
data from transmitter nodes in the audio graph and sending them to their appropriate address. It is also responsible for
receiving any encoded data from the network and sending the bytes to receiver nodes to be decoded.

## License

Licensed under either of

* Apache License, Version 2.0, (LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0), or
* MIT license (LICENSE-MIT or http://opensource.org/licenses/MIT)

at your option.