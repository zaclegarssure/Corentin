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
        .add(coroutine(|fib: Fib, mut transform: W<Transform>| async move {
            loop {
                let dt = fib.next_tick().await;
                transform.get_mut().translation.x += 100.0 * dt.as_secs_f32();
            }
        }));
}
