//! The simplest competitive bot: send all workers at the enemy.

use rust_sc2::prelude::*;

mod ex_main;

struct WorkerRushAI;

impl Player for WorkerRushAI {
	fn settings(&self) -> PlayerSettings<'_> {
		PlayerSettings::new(Race::Protoss).with_name("RustyWorkers")
	}

	fn on_start(&mut self, ctx: &mut Context) -> SC2Result<()> {
		if let Some(townhall) = ctx.world.my().townhalls().first() {
			ctx.cmds.train(townhall, UnitTypeId::Probe, false);
		}
		Ok(())
	}

	fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
		let world = ctx.world;
		let map = &world.map;
		let (enemy_start, start) = (map.enemy_start, map.start_location);
		let minerals = world.mineral_fields();

		// SC2 changes a mineral's tag as it goes in and out of vision,
		// so these have to be looked up every step instead of being remembered.
		let (Some(forward), Some(back)) = (minerals.closest(enemy_start), minerals.closest(start)) else {
			return Ok(());
		};
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
						ctx.cmds.attack(u, closest, false);
					} else {
						ctx.cmds.gather(u, back, false);
					}
				} else if u.in_range_of(closest, 2.0) {
					ctx.cmds.gather(u, back, false);
				} else {
					ctx.cmds.gather(u, forward, false);
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
				Some(closest) => ctx.cmds.attack(u, closest, false),
				None => ctx.cmds.gather(u, forward, false),
			}
		}
		Ok(())
	}
}

fn main() -> SC2Result<()> {
	ex_main::main(WorkerRushAI)
}
