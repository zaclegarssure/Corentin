use std::time::Duration;

use bevy::prelude::*;
use corentin::{coroutine::PrimitiveVoid, prelude::*};

#[derive(Component)]
struct ExampleComponent(u32);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(Executor::new())
        .add_systems(Startup, setup_access)
        .add_systems(Update, run_coroutines)
        .run();
}

fn setup_access(world: &mut World) {
    world.resource_scope(|w, mut exec: Mut<Executor>| {
        let e = w.spawn(ExampleComponent(0)).id();
        exec.add_to_entity(e, move |mut fib, this| async move {
            loop {
                let mut b = fib
                    .duration(Duration::from_secs(1))
                    .then_grab::<&mut ExampleComponent>(this)
                    .await;
                b.0 += 1;
            }
        });
        exec.add(|mut fib| async move {
            loop {
                let c = fib
                    .change::<ExampleComponent>(e)
                    .then_grab::<&ExampleComponent>(e)
                    .await;
                println!("Change detected, value is now {}", c.0);
            }
        });
    })
}

fn run_coroutines(world: &mut World) {
    world.resource_scope(|w, mut exec: Mut<Executor>| {
        exec.tick(w);
    })
}
