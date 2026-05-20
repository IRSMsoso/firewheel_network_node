use bevy::prelude::*;
use bevy_seedling::firewheel::cpal::cpal::StreamConfig;
use bevy_seedling::prelude::*;

fn main() {
    App::default()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::<CpalBackend> {
                config: FirewheelConfig {
                    num_graph_inputs: ChannelCount::STEREO,
                    num_graph_outputs: ChannelCount::STEREO,
                    ..default()
                },
                stream_config: default(),
                graph_config: GraphConfiguration::Empty,
            },
        ))
        .add_systems(Startup, route_input)
        .run();
}

// fn play_sound(mut commands: Commands, server: Res<AssetServer>) {
//     commands.spawn((
//         SamplePlayer::new(server.load("music/arcadia.mp3")).looping(),
//         sample_effects![FreeverbNode::default()],
//     ));
// }

fn route_input(
    input: Single<Entity, With<AudioGraphInput>>,
    output: Single<Entity, With<AudioGraphOutput>>,
    mut commands: Commands,
) {
    commands.entity(*input).connect(*output);
    info!("Routed input");
}
