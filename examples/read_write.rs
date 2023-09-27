fn main() {
    //App::new()
    //    .add_plugins((DefaultPlugins, CorentinPlugin))
    //    .add_systems(Startup, setup_access)
    //    .run();
}

//fn setup_scene(
//    mut meshes: ResMut<Assets<Mesh>>,
//    mut materials: ResMut<Assets<ColorMaterial>>,
//    mut commands: Commands,
//) {
//    commands.spawn(Camera2dBundle::default());
//    let e = world.spawn((IsClicked(0))).id();
//    coroutine(|fib: Fib, mut ex: R<IsClicked>| async move {
//        loop {
//            ex.on_change().await;
//
//        }
//    })
//    .apply(e, world);
//
//    coroutine(|_: Fib, ex: R<ExampleComponent>| async move {
//        loop {
//            ex.on_change().await;
//            println!("Change detected, value is now {}", ex.get().0);
//        }
//    })
//    .apply(e, world);
//}
