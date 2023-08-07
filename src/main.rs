use std::time::Duration;

use bevy::prelude::*;
use flep::Executor;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_non_send_resource(Executor::new())
        .insert_resource(ExampleTimer {
            timer: Timer::new(Duration::from_secs(1), TimerMode::Repeating),
        })
        .add_systems(Startup, (setup_coroutines, spawn_example))
        .add_systems(Update, (mutate_example_every_second, run_coroutines).chain())
        .run();
}

fn setup_coroutines(commands: Commands, mut executor: NonSendMut<Executor>) {
    executor.add(commands, move |mut fib| async move {
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
    timer: Timer,
}

fn spawn_example(mut commands: Commands, mut executor: NonSendMut<Executor>) {
    let id = commands.spawn(Example(0)).id();
    executor.add(commands, move |mut fib| async move {
        let mut times = 0;
        loop {
            fib.change::<Example>(id).await;
            times += 1;
            println!("Example from {:?} has changed {} times", id, times);
        }
    });
}

fn mutate_example_every_second(
    mut q: Query<&mut Example>,
    mut timer: ResMut<ExampleTimer>,
    time: Res<Time>,
) {
    timer.timer.tick(time.delta());
    if timer.timer.just_finished() {
        for mut e in q.iter_mut() {
            println!("Changed example");
            e.0 += 1;
        }
    }
}

fn run_coroutines(world: &mut World) {
    world.non_send_resource_scope(|w, mut exec: Mut<Executor>| {
        exec.run(w);
    })
}
