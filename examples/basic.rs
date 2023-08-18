use std::time::Duration;

use bevy::prelude::*;
use corentin::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(Executor::new())
        .add_systems(Startup, setup_coroutines)
        .add_systems(Update, run_coroutines)
        .run();
}

fn setup_coroutines(mut executor: ResMut<Executor>) {
    executor.add(|mut fib| async move {
        let mut i = 0;
        loop {
            let dt = fib.next_tick().await;
            println!("Last frame lasted for {}", dt.as_secs_f32());
            fib.duration(Duration::from_secs(1)).await;
            i += 1;
            println!("This coroutine has started since {} seconds", i);
        }
    });
}

fn run_coroutines(world: &mut World) {
    world.resource_scope(|w, mut exec: Mut<Executor>| {
        exec.tick(w);
    })
}
