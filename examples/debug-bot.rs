//! Draws expansion locations and unit types on screen.

use rust_sc2::prelude::*;

mod ex_main;

struct DebugAI;

impl Player for DebugAI {
	fn settings(&self) -> PlayerSettings<'_> {
		PlayerSettings::new(Race::Random)
	}

	fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
		let world = ctx.world;

		// Debug expansion locations
		for exp in &world.expansions {
			let z = world.get_z_height(exp.loc) + 1.5;
			ctx.debug.draw_sphere(exp.loc.to3(z), 0.6, Some((255, 128, 255)));
			let z = world.get_z_height(exp.center) + 1.5;
			ctx.debug
				.draw_sphere(exp.center.to3(z), 0.5, Some((255, 128, 64)));
		}

		// Debug unit types
		for u in world.all() {
			let label = format!("{:?}", u.type_id);
			ctx.debug
				.draw_text_world(&label, u.position3d, Some((255, 128, 128)), None);
		}
		Ok(())
	}
}

fn main() -> SC2Result<()> {
	ex_main::main(DebugAI)
}
