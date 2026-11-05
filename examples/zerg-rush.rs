//! Mass zerglings with the speed upgrade, while expanding and injecting larvas.

use rust_sc2::prelude::*;
use std::cmp::Ordering;

mod ex_main;

#[derive(Default)]
struct ZergRushAI {
	last_loop_distributed: u32,
}

impl Player for ZergRushAI {
	fn settings(&self) -> PlayerSettings<'_> {
		PlayerSettings::new(Race::Zerg).with_name("RustyLings")
	}

	fn on_start(&mut self, ctx: &mut Context) -> SC2Result<()> {
		let world = ctx.world;

		// Setting rallypoint for hatchery
		if let Some(townhall) = world.my().townhalls().first() {
			ctx.cmds
				.command(townhall, AbilityId::RallyWorkers, world.map.start_center, false);
		}

		// Splitting workers to closest mineral crystals
		for u in world.my().workers() {
			if let Some(mineral) = world.mineral_fields().closest(u) {
				ctx.cmds.gather(u, mineral, false);
			}
		}

		// Ordering drone on initial 50 minerals
		if let Some(larva) = world.my().larvas().first() {
			ctx.cmds.train(larva, UnitTypeId::Drone, false);
		}
		ctx.budget.subtract_resources(world, UnitTypeId::Drone, true);

		Ok(())
	}

	fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
		self.distribute_workers(ctx);
		self.upgrades(ctx);
		self.build(ctx)?;
		self.order_units(ctx);
		self.execute_micro(ctx);
		Ok(())
	}
}

impl ZergRushAI {
	const DISTRIBUTION_DELAY: u32 = 8;

	fn distribute_workers(&mut self, ctx: &mut Context) {
		let world = ctx.world;
		let workers = world.my().workers();
		if workers.is_empty() {
			return;
		}
		let mut idle_workers = workers.idle().iter().collect::<Vec<_>>();
		let bases = world.my().townhalls().ready();

		// Check distribution delay if there aren't any idle workers
		if idle_workers.is_empty()
			&& self.last_loop_distributed + Self::DISTRIBUTION_DELAY + bases.len() as u32 > world.game_loop
		{
			return;
		}
		self.last_loop_distributed = world.game_loop;

		// Distribute
		let mineral_fields = world.mineral_fields();
		if mineral_fields.is_empty() || bases.is_empty() {
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
		let speed_upgrade = UpgradeId::Zerglingmovementspeed;
		let has_enough_gas = ctx.budget.can_afford_upgrade(world, speed_upgrade)
			|| world.has_upgrade(speed_upgrade)
			|| world.is_ordered_upgrade(speed_upgrade);
		let target_gas_workers = Some(if has_enough_gas { 0 } else { 3 });

		for gas in world.my().gas_buildings().ready() {
			match gas.assigned_harvesters.cmp(&target_gas_workers) {
				Ordering::Less => {
					let mining = workers
						.iter()
						.filter(|u| {
							u.tartag()
								.map_or(false, |target| mineral_fields.contains_tag(target))
						})
						.collect();
					add_idle(&mut idle_workers, mining);
					let missing = gas.ideal_harvesters.unwrap() - gas.assigned_harvesters.unwrap();
					(0..missing).for_each(|_| deficit_geysers.push(gas));
				}
				Ordering::Greater => {
					let closest_base = bases.closest(gas).unwrap();
					let extra = workers
						.iter()
						.filter(|u| {
							u.tartag().map_or(false, |target| {
								target == gas.tag || (u.is_carrying_vespene() && target == closest_base.tag)
							})
						})
						.take((gas.assigned_harvesters.unwrap() - gas.ideal_harvesters.unwrap()) as usize)
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

	fn order_units(&mut self, ctx: &mut Context) {
		let world = ctx.world;

		// Can't order units without resources
		if ctx.budget.minerals < 50 {
			return;
		}

		// Order one queen per each base
		let queen = UnitTypeId::Queen;
		if world.counter().all().count(queen) < world.my().townhalls().len()
			&& ctx.budget.can_afford(world, queen, true)
		{
			if let Some(townhall) = world.my().townhalls().first() {
				ctx.cmds.train(townhall, queen, false);
				ctx.budget.subtract_resources(world, queen, true);
			}
		}

		// Can't order units without larva
		let mut larvas = world.my().larvas().iter().collect::<Vec<_>>();
		if larvas.is_empty() {
			return;
		}

		let over = UnitTypeId::Overlord;
		if ctx.budget.supply_left < 3
			&& world.supply_cap < 200
			&& world.counter().ordered().count(over) == 0
			&& ctx.budget.can_afford(world, over, false)
		{
			if let Some(larva) = larvas.pop() {
				ctx.cmds.train(larva, over, false);
				ctx.budget.subtract_resources(world, over, false);
			}
		}

		let drone = UnitTypeId::Drone;
		if (world.supply_workers as usize) < 96.min(world.counter().all().count(UnitTypeId::Hatchery) * 16)
			&& ctx.budget.can_afford(world, drone, true)
		{
			if let Some(larva) = larvas.pop() {
				ctx.cmds.train(larva, drone, false);
				ctx.budget.subtract_resources(world, drone, true);
			}
		}

		let zergling = UnitTypeId::Zergling;
		if ctx.budget.can_afford(world, zergling, true) {
			if let Some(larva) = larvas.pop() {
				ctx.cmds.train(larva, zergling, false);
				ctx.budget.subtract_resources(world, zergling, true);
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
		let start = world.map.start_location;

		let pool = UnitTypeId::SpawningPool;
		if world.counter().all().count(pool) == 0 && ctx.budget.can_afford(world, pool, false) {
			let place = start.towards(world.map.game_info.map_center, 6.0);
			if let Some(location) = ctx.api.find_placement(world, pool, place, Default::default())? {
				if let Some(builder) = Self::get_builder(world, location) {
					ctx.cmds.build(builder, pool, location, false);
					ctx.budget.subtract_resources(world, pool, false);
				}
			}
		}

		let extractor = UnitTypeId::Extractor;
		if world.counter().all().count(extractor) == 0 && ctx.budget.can_afford(world, extractor, false) {
			if let Some(geyser) = ctx.api.find_gas_placement(world, start)? {
				if let Some(builder) = Self::get_builder(world, geyser.position) {
					ctx.cmds.build_gas(builder, geyser, false);
					ctx.budget.subtract_resources(world, extractor, false);
				}
			}
		}

		let hatchery = UnitTypeId::Hatchery;
		if ctx.budget.can_afford(world, hatchery, false) {
			if let Some(exp) = world.get_expansion() {
				if let Some(builder) = Self::get_builder(world, exp.loc) {
					ctx.cmds.build(builder, hatchery, exp.loc, false);
					ctx.budget.subtract_resources(world, hatchery, false);
				}
			}
		}
		Ok(())
	}

	fn upgrades(&mut self, ctx: &mut Context) {
		let world = ctx.world;
		let speed_upgrade = UpgradeId::Zerglingmovementspeed;
		if !(world.has_upgrade(speed_upgrade) || world.is_ordered_upgrade(speed_upgrade))
			&& ctx.budget.can_afford_upgrade(world, speed_upgrade)
		{
			if let Some(pool) = world.my().structures().of_type(UnitTypeId::SpawningPool).first() {
				ctx.cmds.research(pool, speed_upgrade, false);
				ctx.budget.subtract_upgrade_cost(world, speed_upgrade);
			}
		}
	}

	fn execute_micro(&self, ctx: &mut Context) {
		let world = ctx.world;
		let cmds = &mut *ctx.cmds;

		// Injecting Larva
		let inject = AbilityId::EffectInjectLarva;
		let mut queens = world
			.my()
			.units()
			.of_type(UnitTypeId::Queen)
			.iter()
			.filter(|u| !u.is_using(inject) && u.has_ability(inject))
			.collect::<Vec<_>>();
		for hatchery in world.my().townhalls() {
			let needs_inject = !hatchery.has_buff(BuffId::QueenSpawnLarvaTimer)
				|| hatchery.buff_duration_remain.unwrap() * 20 > hatchery.buff_duration_max.unwrap();
			if needs_inject {
				if let Some(queen) = take_closest(&mut queens, hatchery) {
					cmds.command(queen, inject, hatchery, false);
				}
			}
		}

		let zerglings = world.my().units().of_type(UnitTypeId::Zergling);
		if zerglings.is_empty() {
			return;
		}

		// Check if speed upgrade is >80% ready
		let speed_upgrade = UpgradeId::Zerglingmovementspeed;
		let speed_upgrade_is_almost_ready =
			world.has_upgrade(speed_upgrade) || world.upgrade_progress(speed_upgrade) >= 0.8;

		// Attacking with zerglings or defending our locations
		let targets = if speed_upgrade_is_almost_ready {
			world.enemy().all().ground()
		} else {
			let townhalls = world.my().townhalls();
			world
				.enemy()
				.all()
				.filter(|e| !e.is_flying && townhalls.iter().any(|h| h.is_closer(25.0, e)))
		};
		if !targets.is_empty() {
			for u in &zerglings {
				let target = targets
					.in_range_of(u, 0.0)
					.min(|t| t.hits())
					.or_else(|| targets.closest(u));
				if let Some(target) = target {
					cmds.attack(u, target.position, false);
				}
			}
		} else {
			let map = &world.map;
			let target = if speed_upgrade_is_almost_ready {
				map.enemy_start
			} else {
				map.start_location.towards(map.start_center, -8.0)
			};
			for u in &zerglings {
				cmds.move_to(u, target, false);
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
	ex_main::main(ZergRushAI::default())
}
