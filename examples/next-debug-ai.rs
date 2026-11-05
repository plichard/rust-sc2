//! `DebugAI` from the `bot` crate, on the `next` architecture.
//!
//! The two workarounds the original needs are gone: `self.expansions.clone()`, and
//! collecting labels into a `Vec` so that `self.debug` can be borrowed mutably.

use rust_sc2::next::prelude::*;
use std::collections::HashSet;

#[path = "next_args/mod.rs"]
mod next_args;

/// Stand-in for `bot::base::Base`, which is only a set of tags.
#[derive(Default)]
struct Base {
	idle_workers: HashSet<UnitTag>,
}
impl Base {
	fn add_unit(&mut self, tag: UnitTag) {
		self.idle_workers.insert(tag);
	}
}

#[derive(Default)]
struct DebugAI {
	base: Base,
}

impl Player for DebugAI {
	fn settings(&self) -> PlayerSettings<'_> {
		PlayerSettings::new(Race::Random)
	}

	fn on_start(&mut self, ctx: &mut Context) -> SC2Result<()> {
		for u in ctx.world.my().workers() {
			self.base.add_unit(u.tag);
		}
		Ok(())
	}

	fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
		let Context { world, debug, .. } = ctx;

		// Debug expansion locations
		for exp in &world.expansions {
			let z = world.get_z_height(exp.loc) + 1.5;
			debug.draw_sphere(exp.loc.to3(z), 0.6, Some((255, 128, 255)));
			let z = world.get_z_height(exp.center) + 1.5;
			debug.draw_sphere(exp.center.to3(z), 0.5, Some((255, 128, 64)));
		}

		// Debug unit types
		for u in world.all() {
			debug.draw_text_world(
				&format!("{:?}", u.type_id),
				u.position3d,
				Some((255, 128, 128)),
				None,
			);
		}
		Ok(())
	}
}

fn main() -> SC2Result<()> {
	next_args::run(DebugAI::default())
}
