use bevy::prelude::{Mut, Plugin, Update, World};

use crate::{coroutine::CoroWrites, prelude::Executor};

pub struct CorentinPlugin;

impl Plugin for CorentinPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<Executor>()
            .init_resource::<CoroWrites>()
            .add_systems(Update, run_coroutines);
    }
}

fn run_coroutines(world: &mut World) {
    world.resource_scope(|w, mut exec: Mut<Executor>| {
        exec.tick(w);
    })
}
