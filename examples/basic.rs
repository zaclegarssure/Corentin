use std::time::Duration;

use bevy::{prelude::*, sprite::MaterialMesh2dBundle};
use corentin::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, CorentinPlugin))
        .add_systems(Startup, setup_scene)
        .run();
}

fn setup_scene(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut commands: Commands,
) {
    commands.spawn(Camera2dBundle::default());

    // Circle
    commands
        .spawn(MaterialMesh2dBundle {
            mesh: meshes.add(shape::Circle::new(50.).into()).into(),
            material: materials.add(ColorMaterial::from(Color::PURPLE)),
            transform: Transform::from_translation(Vec3::new(-150., 0., 0.)),
            ..default()
        })
        .add(coroutine(
            |mut s: Scope, mut transform: Wr<Transform>| async move {
                loop {
                    let dt = s.next_tick().await;
                    transform.get_mut(&s).translation.x += 100.0 * dt.as_secs_f32();
                }
            },
        ))
        .add(coroutine(
            |mut s: Scope, transform: Rd<Transform>| async move {
                let mut i = 0;
                let original_x = transform.get(&s).translation.x;
                loop {
                    s.duration(Duration::from_secs(1)).await;
                    i += 1;
                    println!(
                        "After {} seconds, we moved {} to the right",
                        i,
                        transform.get(&s).translation.x - original_x
                    );
                }
            },
        ));
}
