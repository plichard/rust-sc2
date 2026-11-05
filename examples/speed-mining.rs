//! Mines faster than the built-in behaviour, by walking workers in and out of the mineral line.

use rust_sc2::prelude::*;
use std::collections::{HashMap, HashSet};

mod ex_main;

#[derive(Default)]
struct LightningMcQueen {
	base_indices: HashMap<UnitTag, usize>, // (base tag, expansion index)
	assigned: HashMap<UnitTag, HashSet<UnitTag>>, // (mineral, workers)
	free_workers: HashSet<UnitTag>,        // tags of workers which aren't assigned to any work
	harvesters: HashMap<UnitTag, (UnitTag, UnitTag)>, // (worker, (target mineral, nearest townhall))
	targets: HashMap<UnitTag, Point2>,     // (mineral, target move location)
}

impl Player for LightningMcQueen {
	fn settings(&self) -> PlayerSettings<'_> {
		PlayerSettings::new(Race::Random).raw_crop_to_playable_area(true)
	}

	fn on_event(&mut self, ctx: &mut Context, event: Event) -> SC2Result<()> {
		let world = ctx.world;
		match event {
			Event::UnitCreated(tag) => {
				if let Some(u) = world.my().units().get(tag) {
					if u.type_id == world.map.race_values.worker {
						self.free_workers.insert(tag);
					}
				}
			}
			Event::ConstructionComplete(tag) => {
				if let Some(u) = world.my().structures().get(tag) {
					if u.type_id == world.map.race_values.start_townhall {
						if let Some(idx) = world.expansions.iter().position(|exp| exp.base == Some(tag)) {
							self.base_indices.insert(tag, idx);
						}
					}
				}
			}
			Event::UnitDestroyed(tag, alliance) => match alliance {
				Some(Alliance::Own) => {
					// townhall destroyed
					if let Some(idx) = self.base_indices.remove(&tag) {
						for m in world.minerals_of(&world.expansions[idx]) {
							self.remove_mineral(m.tag);
						}
					// harvester died
					} else if let Some((m, _)) = self.harvesters.remove(&tag) {
						if let Some(workers) = self.assigned.get_mut(&m) {
							workers.remove(&tag);
						}
					// free worker died
					} else {
						self.free_workers.remove(&tag);
					}
				}
				// mineral mined out
				Some(Alliance::Neutral) => self.remove_mineral(tag),
				_ => {}
			},
			_ => {}
		}
		Ok(())
	}

	fn on_start(&mut self, ctx: &mut Context) -> SC2Result<()> {
		self.assign_mineral_targets(ctx.world);
		Ok(())
	}

	fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
		let world = ctx.world;
		self.assign_roles(world);
		self.execute_micro(world, ctx.cmds);

		// visualise the mineral target points
		for (mineral_tag, t) in &self.targets {
			if let Some(m) = world.unit(*mineral_tag).map(|m| m.position) {
				let start = t.to3(world.get_z_height(*t) + 0.5);
				let end = m.to3(world.get_z_height(m) + 0.5);

				ctx.debug.draw_line(start, end, Some((255, 255, 60)));
				ctx.debug.draw_sphere(start, 0.5, Some((255, 255, 60)));
			}
		}

		// print out total minerals gathered by the 5 minute mark
		if world.game_loop == 6720 {
			println!(
				"mined {} minerals by {}:{:02}",
				world.minerals,
				world.time as usize / 60,
				world.time as usize % 60
			);
		}

		Ok(())
	}
}

impl LightningMcQueen {
	const MINERAL_RADIUS: f32 = 1.35;

	fn remove_mineral(&mut self, tag: UnitTag) {
		if let Some(workers) = self.assigned.remove(&tag) {
			for w in workers {
				self.harvesters.remove(&w);
				self.free_workers.insert(w);
			}
		}
	}

	fn assign_mineral_targets(&mut self, world: &World) {
		for (&b, &i) in &self.base_indices {
			let Some(base) = world.my().townhalls().get(b) else {
				continue;
			};
			let base = base.position;

			for m in world.minerals_of(&world.expansions[i]) {
				let mineral = m.position;

				// default target point is straight towards the townhall
				let mut target = mineral.towards(base, Self::MINERAL_RADIUS);

				// create an offset vector that pushes the target away from each nearby mineral
				let nearby_minerals = world
					.mineral_fields()
					.closer(Self::MINERAL_RADIUS * 1.5, mineral)
					.filter(|p| p.tag != m.tag);
				let mut offset = Point2::new(0.0, 0.0);
				for patch in &nearby_minerals {
					let patch = patch.position;
					let push = patch.towards(target, 1.0) - patch;
					offset += push / mineral.distance(patch);
				}

				// add our offset, and then normalise the resulting point back onto the radius
				target = mineral.towards(target + offset, Self::MINERAL_RADIUS);

				self.targets.insert(m.tag, target);
			}
		}
	}

	fn assign_roles(&mut self, world: &World) {
		// (mineral tag, nearest base tag), once for each free slot on a mineral
		let mut harvest_targets = vec![];
		for (b, i) in &self.base_indices {
			for m in world.minerals_of(&world.expansions[*i]) {
				let free = 2 - self.assigned.get(&m.tag).map_or(0, |ws| ws.len());
				harvest_targets.extend(std::iter::repeat((m.tag, *b)).take(free));
			}
		}

		let free_workers = self.free_workers.iter().copied().collect::<Vec<_>>();
		for (w, t) in free_workers.into_iter().zip(harvest_targets) {
			self.free_workers.remove(&w);
			self.harvesters.insert(w, t);
			self.assigned.entry(t.0).or_default().insert(w);
		}
	}

	fn execute_micro(&self, world: &World, cmds: &mut Commands) {
		for u in world.my().workers() {
			let Some(&(mineral, base_tag)) = self.harvesters.get(&u.tag) else {
				continue;
			};
			// only need to change orders if we don't already have 2 commands queued
			if u.orders.len() >= 2 {
				continue;
			}
			// we're on our way back from a mineral field
			if u.is_carrying_resource() {
				let Some(base) = world.my().townhalls().get(base_tag) else {
					continue;
				};
				let target = base.position.towards(u.position, base.radius * 1.08);
				let distance = u.position.distance_squared(target);
				// let the built-in unit behaviour handle the first ~half of the trip
				if distance > 0.5625 && distance < 4.0 {
					cmds.move_to(u, target, false);
					cmds.smart(u, base, true);
				}
				// deal with the rare case where collisions cause the worker to just park itself
				else if !u.is_returning() {
					cmds.smart(u, base, false);
				}
			}
			// we're on our way to a mineral field
			else if let Some(&target) = self.targets.get(&mineral) {
				let distance = u.position.distance_squared(target);
				// again we want to mineral walk as much of the way as possible, before using the queue trick
				if distance > 0.5625 && distance < 4.0 {
					cmds.move_to(u, target, false);
					cmds.smart(u, mineral, true);
				}
				// either sc2 accidentally deposited the minerals early, or it switched mineral fields on us
				else if !u.is_gathering() || u.tartag().map_or(false, |t| t != mineral) {
					cmds.gather(u, mineral, false);
				}
			}
		}
	}
}

fn main() -> SC2Result<()> {
	ex_main::main(LightningMcQueen::default())
}
