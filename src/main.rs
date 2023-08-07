use std::time::Duration;

use bevy::prelude::*;
use flep::{Executor, coroutine::Fib};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_non_send_resource(Executor::new())
        .insert_resource(ExampleTimer {
            change_timer: Timer::new(Duration::from_secs(1), TimerMode::Repeating),
            despawn_timer: Timer::new(Duration::from_secs(5), TimerMode::Once),
        })
        .add_systems(Startup, (setup_coroutines, spawn_example))
        .add_systems(Update, (mutate_example_every_second, run_coroutines).chain())
        .run();
}

fn setup_coroutines(mut executor: NonSendMut<Executor>) {
    executor.add(move |mut fib| async move {
        let mut a = 0;
        loop {
            println!("It runs since {} seconds", a);
            fib.duration(Duration::from_secs(1)).await;
            a +=1;
        }
    });
}


#[derive(Component)]
struct Example(i32);

#[derive(Resource)]
struct ExampleTimer {
    change_timer: Timer,
    despawn_timer: Timer,
}

fn spawn_example(mut commands: Commands, mut executor: NonSendMut<Executor>) {
    let id = commands.spawn(Example(0)).id();
    executor.add(move |mut fib| async move {
        let mut times = 0;
        loop {
            fib.change::<Example>(id).await;
            times += 1;
            println!("Example from {:?} has changed {} times", id, times);
        }
    });
    executor.add_to_entity(id, bound_to_the_entity);
}

async fn bound_to_the_entity(mut fib: Fib, entity: Entity) {
    let mut times = 0;
    loop {
        fib.change::<Example>(entity).await;
        times += 1;
        println!("Example from {:?} has changed {} times", entity, times);
    }
}

fn mutate_example_every_second(
    mut q: Query<(Entity ,&mut Example)>,
    mut timer: ResMut<ExampleTimer>,
    mut commands: Commands,
    time: Res<Time>,
) {
    timer.change_timer.tick(time.delta());
    timer.despawn_timer.tick(time.delta());
    if timer.despawn_timer.just_finished() {
        for (e, _) in &q {
            commands.entity(e).despawn();
        }
    } else if timer.change_timer.just_finished() {
        for (_, mut example) in q.iter_mut() {
            println!("Changed example");
            example.0 += 1;
        }
    }
}

fn run_coroutines(world: &mut World) {
    world.non_send_resource_scope(|w, mut exec: Mut<Executor>| {
        exec.run(w);
    })
}
