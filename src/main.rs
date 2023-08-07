use std::time::Duration;

use bevy::prelude::*;
use flep::Executor;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_non_send_resource(Executor::new())
        .add_systems(Startup, spawn_example)
        .add_systems(Update, run_coroutines)
        .run();
}

fn run_coroutines(world: &mut World) {
    world.non_send_resource_scope(|w, mut exec: Mut<Executor>| {
        exec.run(w);
    })
}
