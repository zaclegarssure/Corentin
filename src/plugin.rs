use bevy::prelude::{Mut, Plugin, Update, World};

use crate::executor::Executor;

pub struct CorentinPlugin;

impl Plugin for CorentinPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<Executor>()
            .add_systems(Update, run_coroutines);
    }
}

fn run_coroutines(world: &mut World) {
    world.resource_scope(|w, mut exec: Mut<Executor>| {
        exec.tick(w);
    })
}
