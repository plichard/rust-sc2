//! Builds barracks and attacks with reapers, using their grenades and retreating when hurt.

use rust_sc2::prelude::*;
use std::{cmp::Ordering, collections::HashSet};

mod ex_main;

#[derive(Default)]
struct ReaperRushAI {
	reapers_retreat: HashSet<UnitTag>,
	last_loop_distributed: u32,
}

impl Player for ReaperRushAI {
	fn settings(&self) -> PlayerSettings<'_> {
		PlayerSettings::new(Race::Terran).with_name("RustyReapers")
	}

	fn on_start(&mut self, ctx: &mut Context) -> SC2Result<()> {
		let world = ctx.world;
		if let Some(townhall) = world.my().townhalls().first() {
			// Setting rallypoint for command center
			ctx.cmds.smart(townhall, world.map.start_center, false);

			// Ordering scv on initial 50 minerals
			ctx.cmds.train(townhall, UnitTypeId::SCV, false);
			ctx.budget.subtract_resources(world, UnitTypeId::SCV, true);
		}

		// Splitting workers to closest mineral crystals
		for u in world.my().workers() {
			if let Some(mineral) = world.mineral_fields().closest(u) {
				ctx.cmds.gather(u, mineral, false);
			}
		}

		Ok(())
	}

	fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
		self.distribute_workers(ctx);
		self.build(ctx)?;
		self.train(ctx);
		self.execute_micro(ctx);
		Ok(())
	}
}

impl ReaperRushAI {
	const DISTRIBUTION_DELAY: u32 = 8;

	fn distribute_workers(&mut self, ctx: &mut Context) {
		let world = ctx.world;
		let workers = world.my().workers();
		if workers.is_empty() {
			return;
		}
		let mut idle_workers = workers.idle().iter().collect::<Vec<_>>();

		// Check distribution delay if there aren't any idle workers
		if idle_workers.is_empty() && self.last_loop_distributed + Self::DISTRIBUTION_DELAY > world.game_loop
		{
			return;
		}
		self.last_loop_distributed = world.game_loop;

		// Distribute
		let mineral_fields = world.mineral_fields();
		if mineral_fields.is_empty() {
			return;
		}
		let bases = world.my().townhalls().ready();
		if bases.is_empty() {
			return;
		}

		let mut deficit_minings = vec![];
		let mut deficit_geysers = vec![];

		// Distributing mineral workers
		for base in &bases {
			match base.assigned_harvesters.cmp(&base.ideal_harvesters) {
				Ordering::Less => {
					let missing = base.ideal_harvesters.unwrap() - base.assigned_harvesters.unwrap();
					(0..missing).for_each(|_| deficit_minings.push(base));
				}
				Ordering::Greater => {
					let local_minerals = mineral_fields.closer(11.0, base);
					let surplus = base.assigned_harvesters.unwrap() - base.ideal_harvesters.unwrap();
					let extra = workers
						.iter()
						.filter(|u| {
							u.tartag().map_or(false, |target| {
								local_minerals.contains_tag(target)
									|| (u.is_carrying_minerals() && target == base.tag)
							})
						})
						.take(surplus as usize)
						.collect();
					add_idle(&mut idle_workers, extra);
				}
				_ => {}
			}
		}

		// Distributing gas workers
		let gas_buildings = world
			.my()
			.gas_buildings()
			.ready()
			.filter(|g| g.vespene_contents.map_or(false, |vespene| vespene > 0));
		for gas in &gas_buildings {
			match gas.assigned_harvesters.cmp(&gas.ideal_harvesters) {
				Ordering::Less => {
					let missing = gas.ideal_harvesters.unwrap() - gas.assigned_harvesters.unwrap();
					(0..missing).for_each(|_| deficit_geysers.push(gas));
				}
				Ordering::Greater => {
					let surplus = gas.assigned_harvesters.unwrap() - gas.ideal_harvesters.unwrap();
					let closest_base = bases.closest(gas).unwrap();
					let extra = workers
						.iter()
						.filter(|u| {
							u.tartag().map_or(false, |target| {
								target == gas.tag || (u.is_carrying_vespene() && target == closest_base.tag)
							})
						})
						.take(surplus as usize)
						.collect();
					add_idle(&mut idle_workers, extra);
				}
				_ => {}
			}
		}

		// Distributing idle workers
		let minerals_near_base = if idle_workers.len() > deficit_minings.len() + deficit_geysers.len() {
			Some(mineral_fields.filter(|m| bases.iter().any(|base| base.is_closer(11.0, m))))
				.filter(|minerals| !minerals.is_empty())
		} else {
			None
		};

		for u in idle_workers {
			if let Some(closest) = take_closest(&mut deficit_geysers, u) {
				ctx.cmds.gather(u, closest, false);
			} else if let Some(closest) = take_closest(&mut deficit_minings, u) {
				let richest = mineral_fields
					.closer(11.0, closest)
					.max(|m| m.mineral_contents.unwrap_or(0))
					.unwrap();
				ctx.cmds.gather(u, richest, false);
			} else if u.is_idle() {
				if let Some(mineral) = minerals_near_base.as_ref().and_then(|ms| ms.closest(u)) {
					ctx.cmds.gather(u, mineral, false);
				}
			}
		}
	}

	fn get_builder<'w>(world: &'w World, pos: Point2) -> Option<UnitRef<'w>> {
		let minerals = world.mineral_fields();
		world
			.my()
			.workers()
			.filter(|u| {
				!(u.is_constructing()
					|| u.is_returning()
					|| u.is_carrying_resource()
					|| (u.is_gathering() && u.tartag().map_or(true, |tag| !minerals.contains_tag(tag))))
			})
			.closest(pos)
	}

	fn build(&mut self, ctx: &mut Context) -> SC2Result<()> {
		let world = ctx.world;
		if ctx.budget.minerals < 75 {
			return Ok(());
		}

		let map = &world.map;
		let main_base = map.start_location.towards(map.game_info.map_center, 8.0);

		if world.counter().count(UnitTypeId::Refinery) < 2
			&& world.counter().ordered().count(UnitTypeId::Refinery) == 0
			&& ctx.budget.can_afford(world, UnitTypeId::Refinery, false)
		{
			if let Some(geyser) = ctx.api.find_gas_placement(world, map.start_location)? {
				if let Some(builder) = Self::get_builder(world, geyser.position) {
					ctx.cmds.build_gas(builder, geyser, false);
					ctx.budget.subtract_resources(world, UnitTypeId::Refinery, false);
				}
			}
		}

		if ctx.budget.supply_left < 3
			&& world.supply_cap < 200
			&& world.counter().ordered().count(UnitTypeId::SupplyDepot) == 0
			&& ctx.budget.can_afford(world, UnitTypeId::SupplyDepot, false)
		{
			let location =
				ctx.api
					.find_placement(world, UnitTypeId::SupplyDepot, main_base, Default::default())?;
			if let Some(location) = location {
				if let Some(builder) = Self::get_builder(world, location) {
					ctx.cmds.build(builder, UnitTypeId::SupplyDepot, location, false);
					ctx.budget
						.subtract_resources(world, UnitTypeId::SupplyDepot, false);
					return Ok(());
				}
			}
		}

		if world.counter().all().count(UnitTypeId::Barracks) < 4
			&& ctx.budget.can_afford(world, UnitTypeId::Barracks, false)
		{
			let options = PlacementOptions {
				step: 4,
				..Default::default()
			};
			if let Some(location) = ctx
				.api
				.find_placement(world, UnitTypeId::Barracks, main_base, options)?
			{
				if let Some(builder) = Self::get_builder(world, location) {
					ctx.cmds.build(builder, UnitTypeId::Barracks, location, false);
					ctx.budget.subtract_resources(world, UnitTypeId::Barracks, false);
				}
			}
		}
		Ok(())
	}

	fn train(&mut self, ctx: &mut Context) {
		let world = ctx.world;
		if ctx.budget.minerals < 50 || ctx.budget.supply_left == 0 {
			return;
		}

		if world.supply_workers < 22 && ctx.budget.can_afford(world, UnitTypeId::SCV, true) {
			if let Some(cc) = world
				.my()
				.townhalls()
				.iter()
				.find(|u| u.is_ready() && u.is_almost_idle())
			{
				ctx.cmds.train(cc, UnitTypeId::SCV, false);
				ctx.budget.subtract_resources(world, UnitTypeId::SCV, true);
			}
		}

		if ctx.budget.can_afford(world, UnitTypeId::Reaper, true) {
			let barracks = world.my().structures().of_type(UnitTypeId::Barracks);
			if let Some(barracks) = barracks.iter().find(|u| u.is_ready() && u.is_almost_idle()) {
				ctx.cmds.train(barracks, UnitTypeId::Reaper, false);
				ctx.budget.subtract_resources(world, UnitTypeId::Reaper, true);
			}
		}
	}

	fn throw_mine(reaper: UnitRef, target: UnitRef, cmds: &mut Commands) -> bool {
		let grenade = AbilityId::KD8ChargeKD8Charge;
		if reaper.has_ability(grenade) && reaper.in_ability_cast_range(grenade, target, 0.0) {
			cmds.command(reaper, grenade, target.position, false);
			true
		} else {
			false
		}
	}

	fn execute_micro(&mut self, ctx: &mut Context) {
		let world = ctx.world;
		let cmds = &mut *ctx.cmds;

		// Lower ready depots
		for depot in world.my().structures().of_type(UnitTypeId::SupplyDepot).ready() {
			cmds.use_ability(depot, AbilityId::MorphSupplyDepotLower, false);
		}

		// Reapers micro
		let reapers = world.my().units().of_type(UnitTypeId::Reaper);
		if reapers.is_empty() {
			return;
		}

		let targets = {
			let ground_targets = world.enemy().all().ground();
			let ground_attackers = ground_targets.filter(|e| e.can_attack_ground());
			if ground_attackers.is_empty() {
				ground_targets
			} else {
				ground_attackers
			}
		};

		for u in &reapers {
			let is_retreating = self.reapers_retreat.contains(&u.tag);
			if is_retreating {
				if u.health_percentage().unwrap() > 0.75 {
					self.reapers_retreat.remove(&u.tag);
				}
			} else if u.health_percentage().unwrap() < 0.5 {
				self.reapers_retreat.insert(u.tag);
			}

			let Some(closest) = targets.closest(u) else {
				let pos = if is_retreating {
					u.position
				} else {
					world.map.enemy_start
				};
				cmds.move_to(u, pos, false);
				continue;
			};
			if Self::throw_mine(u, closest, cmds) {
				return;
			}
			if is_retreating || u.on_cooldown() {
				let attackers =
					targets.filter(|t| t.in_range(u, t.speed() + if is_retreating { 2.0 } else { 0.5 }));
				match attackers.closest(u) {
					Some(closest_attacker) => {
						let flee_position = {
							let pos = u.position.towards(closest_attacker.position, -u.speed());
							if world.is_pathable(pos) {
								pos
							} else {
								*u.position
									.neighbors8()
									.iter()
									.filter(|p| world.is_pathable(**p))
									.furthest(closest_attacker)
									.unwrap_or(&world.map.start_location)
							}
						};
						cmds.move_to(u, flee_position, false);
					}
					None => {
						if !(is_retreating || u.in_range(closest, 0.0)) {
							cmds.move_to(u, closest.position, false);
						}
					}
				}
			} else {
				match targets.in_range_of(u, 0.0).min(|t| t.hits()) {
					Some(target) => cmds.attack(u, target, false),
					None => cmds.move_to(u, closest.position, false),
				}
			}
		}
	}
}

/// Adds workers to the idle ones, unless they're already there.
fn add_idle<'w>(idle: &mut Vec<UnitRef<'w>>, extra: Vec<UnitRef<'w>>) {
	for u in extra {
		if !idle.iter().any(|w| w.tag == u.tag) {
			idle.push(u);
		}
	}
}

/// Removes and returns the unit closest to `to`.
fn take_closest<'w>(units: &mut Vec<UnitRef<'w>>, to: UnitRef) -> Option<UnitRef<'w>> {
	let closest = units.iter().copied().closest(to)?;
	let index = units.iter().position(|u| u.tag == closest.tag)?;
	Some(units.swap_remove(index))
}

fn main() -> SC2Result<()> {
	ex_main::main(ReaperRushAI::default())
}
