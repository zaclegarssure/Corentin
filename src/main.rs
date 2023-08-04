use std::{cell::RefCell, rc::Rc};

use bevy::prelude::*;
use flep::{Duration, DurationUnit, Executor};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_non_send_resource(Executor::new())
        .add_systems(Startup, setup_fibers)
        .add_systems(Update, (run_tamer, spawn_some_stuff).chain())
        .run();
}

fn setup_fibers(mut executor: NonSendMut<Executor>) {
    executor.push(move |mut fib| async move {
        let a = RefCell::new(0);
        let mut b = 10;
        for i in 1..=10 {
            fib.next_tick().await;
            fib.with_component(|t: Transform| todo!()).await;
            //println!("Frame {} with a = {}", i, a);
            async {
                fib.seconds(1.0).await;
                a.replace_with(|&mut o| o+1);
                println!("From inside a is {}", a.borrow());
                b+=1;
            }
            .await;
            *a.borrow_mut() += 1;
        }
    });
}

fn spawn_some_stuff(mut commands: Commands) {
    commands.spawn_empty();
}

fn run_fiber(mut executor: NonSendMut<Executor>, time: Res<Time>) {
    executor.run(&time);
}

fn run_tamer(world: &mut World) {
    let cell = Rc::new(world.cell());
    let mut executor = cell.non_send_resource_mut::<Executor>();
    executor.run_with_world(Rc::clone(&cell));
}
