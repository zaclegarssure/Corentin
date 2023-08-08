use std::time::Duration;

use bevy::prelude::*;
use flep::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_non_send_resource(Executor::new())
        .add_systems(Startup, setup_coroutines)
        .add_systems(Update, run_coroutines)
        .run();
}

fn setup_coroutines(mut executor: NonSendMut<Executor>) {
    executor.add(|mut fib| async move {
        let mut i = 0;
        loop {
            fib.duration(Duration::from_secs(1)).await;
            i += 1;
            println!("This coroutine has started since {} seconds", i);
        }
    });
}

fn run_coroutines(world: &mut World) {
    world.non_send_resource_scope(|w, mut exec: Mut<Executor>| {
        exec.run(w);
    })
}
