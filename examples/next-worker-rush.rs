//! `examples/worker-rush.rs`, ported to the `next` architecture.

use rust_sc2::next::{Context, Player};
use rust_sc2::prelude::*;

#[path = "next_args/mod.rs"]
mod next_args;

#[derive(Default)]
struct WorkerRushAI {
	mineral_forward: u64,
	mineral_back: u64,
}

impl Player for WorkerRushAI {
	fn settings(&self) -> PlayerSettings<'_> {
		PlayerSettings::new(Race::Protoss).with_name("RustyWorkers")
	}

	fn on_start(&mut self, ctx: &mut Context) -> SC2Result<()> {
		let world = &ctx.world;
		if let Some(townhall) = world.my().townhalls().first() {
			ctx.cmds.train(townhall, UnitTypeId::Probe, false);
		}
		if let Some(closest) = world.mineral_fields().closest(world.map.enemy_start) {
			self.mineral_forward = closest.tag;
		}
		if let Some(closest) = world.mineral_fields().closest(world.map.start_location) {
			self.mineral_back = closest.tag;
		}
		Ok(())
	}

	fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
		let Context { world, cmds, .. } = ctx;
		let enemy_start = world.map.enemy_start;
		let workers = world.my().workers();

		let ground_attackers = world
			.enemy()
			.units()
			.filter(|u| !u.is_flying && u.can_attack_ground() && u.is_closer(45.0, enemy_start));
		if !ground_attackers.is_empty() {
			for u in &workers {
				let closest = ground_attackers.closest(u).unwrap();
				if u.shield > Some(5) {
					if !u.on_cooldown() {
						cmds.attack(u, Target::Tag(closest.tag), false);
					} else {
						cmds.gather(u, self.mineral_back, false);
					}
				} else if u.in_range_of(closest, 2.0) {
					cmds.gather(u, self.mineral_back, false);
				} else {
					cmds.gather(u, self.mineral_forward, false);
				}
			}
			return Ok(());
		}

		let ground_structures = world
			.enemy()
			.structures()
			.filter(|u| !u.is_flying && u.is_closer(45.0, enemy_start));
		for u in &workers {
			match ground_structures.closest(u) {
				Some(closest) => cmds.attack(u, Target::Tag(closest.tag), false),
				None => cmds.gather(u, self.mineral_forward, false),
			}
		}
		Ok(())
	}
}

fn main() -> SC2Result<()> {
	next_args::run(WorkerRushAI::default())
}
