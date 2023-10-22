use std::time::Duration;

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
};
use corentin::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, CorentinPlugin))
        .add_plugins(LogDiagnosticsPlugin::default())
        .add_plugins(FrameTimeDiagnosticsPlugin)
        .add_systems(Startup, setup_scene)
        .run();
}

#[derive(Component)]
struct Example(u32);

fn setup_scene(mut commands: Commands) {
    for _ in 0..10000 {
        let tracker: ChangeTracker<Example> = ChangeTracker::new();
        commands
            .spawn((Example(0), tracker))
            .add(coroutine(|mut s: Scope| async move {
                s.start_local(|mut s: Scope, mut ex: Wr<Example>| async move {
                    loop {
                        s.next_tick().await;
                        ex.get_mut(&s).0 += 1;
                    }
                });

                s.duration(Duration::from_secs(5)).await;

                loop {
                    let c1 = s.start(wait_on_example);
                    let c2 = s.start(wait_on_example);
                    s.all((c1, c2)).await;
                }
            }));
    }
}

async fn wait_on_example(mut s: Scope, on_change: OnChange<Example>) {
    on_change.observe(&mut s).await;
}
