//! These run without SC2: observations are built by hand and fed to `build_world`.

use super::*;
use crate::{
	bot::Expansion,
	consts::RACE_VALUES,
	game_data::UnitTypeData,
	next::{Budget, CalcTarget, Commands, MapData, UnitRef, Units},
	player::Race,
};
use indexmap::IndexSet;
use num_traits::ToPrimitive;
use sc2_proto::{
	common::ImageData,
	raw::{Alliance as ProtoAlliance, Unit as ProtoUnit},
};
use std::sync::Arc;

const MAP_SIZE: i32 = 64;

fn unit_type(id: UnitTypeId, attributes: &[Attribute]) -> UnitTypeData {
	UnitTypeData {
		id,
		name: format!("{:?}", id),
		available: true,
		cargo_size: 0,
		mineral_cost: 0,
		vespene_cost: 0,
		food_required: 0.0,
		food_provided: 0.0,
		ability: None,
		race: Race::Random,
		build_time: 0.0,
		has_vespene: false,
		has_minerals: false,
		sight_range: 0.0,
		tech_alias: vec![],
		unit_alias: None,
		tech_requirement: None,
		require_attached: false,
		attributes: attributes.to_vec(),
		movement_speed: 3.0,
		armor: 0,
		weapons: vec![],
	}
}

fn empty_world() -> World {
	let mut game_data = GameData::default();
	for id in [UnitTypeId::Probe, UnitTypeId::Marine, UnitTypeId::MineralField] {
		game_data.units.insert(id, unit_type(id, &[]));
	}
	for id in [UnitTypeId::Nexus, UnitTypeId::Pylon] {
		game_data.units.insert(id, unit_type(id, &[Attribute::Structure]));
	}
	let map = MapData {
		player_id: 1,
		enemy_player_id: 2,
		opponent_id: String::new(),
		race: Race::Protoss,
		race_values: RACE_VALUES[&Race::Protoss].clone(),
		game_info: Default::default(),
		game_data,
		start_location: Point2::new(10.0, 10.0),
		enemy_start: Point2::new(50.0, 50.0),
		start_center: Point2::new(10.0, 10.0),
		enemy_start_center: Point2::new(50.0, 50.0),
		ramps: Default::default(),
		vision_blockers: vec![],
	};
	let mut minerals = IndexSet::default();
	minerals.insert(100);
	let expansion = Expansion {
		loc: Point2::new(30.0, 30.0),
		center: Point2::new(30.0, 30.0),
		minerals,
		geysers: Default::default(),
		alliance: Alliance::Neutral,
		base: None,
	};
	World::initial(Arc::new(map), Race::Terran, vec![expansion])
}

fn image(data: Vec<u8>) -> ImageData {
	let mut image = ImageData::new();
	image.set_bits_per_pixel(8);
	let size = image.size.mut_or_insert_default();
	size.set_x(MAP_SIZE);
	size.set_y(MAP_SIZE);
	image.set_data(data);
	image
}

fn observation(units: Vec<ProtoUnit>, dead: &[u64]) -> ResponseObservation {
	let mut res = ResponseObservation::new();
	let raw = res
		.observation
		.mut_or_insert_default()
		.raw_data
		.mut_or_insert_default();
	let map = raw.map_state.mut_or_insert_default();
	let tiles = (MAP_SIZE * MAP_SIZE) as usize;
	map.visibility = Some(image(vec![2; tiles])).into(); // everything is visible
	map.creep = Some(image(vec![0; tiles / 8])).into();
	raw.units = units;
	raw.event.mut_or_insert_default().dead_units = dead.to_vec();
	res
}

fn unit(tag: u64, type_id: UnitTypeId, alliance: ProtoAlliance, pos: (f32, f32)) -> ProtoUnit {
	let mut u = ProtoUnit::new();
	u.set_tag(tag);
	u.set_unit_type(type_id.to_u32().unwrap());
	u.set_alliance(alliance);
	u.set_build_progress(1.0);
	let p = u.pos.mut_or_insert_default();
	p.set_x(pos.0);
	p.set_y(pos.1);
	u
}

fn step(prev: &World, units: Vec<ProtoUnit>, dead: &[u64]) -> (World, Vec<Event>) {
	build_world(prev, &observation(units, dead), Default::default())
}

#[test]
fn world_is_thread_safe_without_the_rayon_feature() {
	fn assert_send_sync<T: Send + Sync>() {}
	assert_send_sync::<World>();
	assert_send_sync::<UnitRef<'_>>();
	assert_send_sync::<Units<'_>>();

	let (world, _) = step(
		&empty_world(),
		vec![
			unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0)),
			unit(2, UnitTypeId::Probe, ProtoAlliance::Self_, (2.0, 2.0)),
		],
		&[],
	);
	let total = std::thread::scope(|s| {
		let a = s.spawn(|| world.my().workers().iter().map(|u| u.tag).sum::<u64>());
		let b = s.spawn(|| world.my().workers().iter().map(|u| u.tag).sum::<u64>());
		a.join().unwrap() + b.join().unwrap()
	});
	assert_eq!(total, 6);
}

#[test]
fn units_are_sorted_into_groups() {
	let (world, _) = step(
		&empty_world(),
		vec![
			unit(100, UnitTypeId::MineralField, ProtoAlliance::Neutral, (5.0, 5.0)),
			unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0)),
			unit(2, UnitTypeId::Nexus, ProtoAlliance::Self_, (30.0, 30.0)),
			unit(3, UnitTypeId::Marine, ProtoAlliance::Enemy, (9.0, 9.0)),
		],
		&[],
	);
	assert_eq!(world.all().len(), 4);
	assert_eq!(world.my().all().len(), 2);
	assert_eq!(world.my().workers().first().unwrap().tag, 1);
	assert_eq!(world.my().townhalls().first().unwrap().tag, 2);
	assert_eq!(world.enemy().units().first().unwrap().tag, 3);
	assert_eq!(world.mineral_fields().first().unwrap().tag, 100);
	assert_eq!(world.expansions[0].alliance, Alliance::Own);
	assert_eq!(world.expansions[0].base, Some(2));
}

#[test]
fn group_lookup_only_finds_members() {
	let (world, _) = step(
		&empty_world(),
		vec![
			unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0)),
			unit(2, UnitTypeId::Nexus, ProtoAlliance::Self_, (2.0, 2.0)),
			unit(3, UnitTypeId::Probe, ProtoAlliance::Self_, (9.0, 9.0)),
		],
		&[],
	);
	let workers = world.my().workers();
	assert!(workers.get(3).is_some());
	assert!(workers.get(2).is_none());
	assert!(workers.get(99).is_none());
	assert_eq!(world.unit(2).unwrap().type_id, UnitTypeId::Nexus);

	let far = workers.filter(|u| u.position.x > 5.0);
	assert!(far.get(3).is_some());
	assert!(far.get(1).is_none());
	assert_eq!(workers.closest(Point2::new(8.0, 8.0)).unwrap().tag, 3);
}

#[test]
fn losing_health_counts_as_attacked() {
	let healthy = |hp: f32| {
		let mut u = unit(1, UnitTypeId::Marine, ProtoAlliance::Enemy, (1.0, 1.0));
		u.set_health(hp);
		u
	};
	let (first, _) = step(&empty_world(), vec![healthy(45.0)], &[]);
	assert!(!first.unit(1).unwrap().is_attacked());

	let (second, _) = step(&first, vec![healthy(30.0)], &[]);
	assert!(second.unit(1).unwrap().is_attacked());
	assert_eq!(second.unit(1).unwrap().damage_taken(), 15);
}

/// `Unit::is_attacked` in the old API returns `true` here, because `None < Some(_)`.
#[test]
fn losing_the_health_reading_is_not_being_attacked() {
	let mut visible = unit(1, UnitTypeId::Marine, ProtoAlliance::Enemy, (1.0, 1.0));
	visible.set_health(45.0);
	let (first, _) = step(&empty_world(), vec![visible], &[]);

	let snapshot = unit(1, UnitTypeId::Marine, ProtoAlliance::Enemy, (1.0, 1.0));
	let (second, _) = step(&first, vec![snapshot], &[]);
	assert_eq!(second.unit(1).unwrap().health, None);
	assert!(!second.unit(1).unwrap().is_attacked());
}

#[test]
fn hallucinations_stay_known_after_the_flag_disappears() {
	let mut flagged = unit(1, UnitTypeId::Marine, ProtoAlliance::Enemy, (1.0, 1.0));
	flagged.set_is_hallucination(true);
	let (first, _) = step(&empty_world(), vec![flagged], &[]);

	let unflagged = unit(1, UnitTypeId::Marine, ProtoAlliance::Enemy, (1.0, 1.0));
	let (second, _) = step(&first, vec![unflagged], &[]);
	assert!(second.unit(1).unwrap().is_hallucination);
	assert_eq!(second.enemies_current.get(&UnitTypeId::Marine), None);
}

#[test]
fn detection_by_enemy_detector_reveals_your_units() {
	let mine = |pos| {
		let mut u = unit(1, UnitTypeId::Marine, ProtoAlliance::Self_, pos);
		u.set_radius(0.5);
		u
	};
	let mut raven = unit(2, UnitTypeId::Raven, ProtoAlliance::Enemy, (10.0, 10.0));
	raven.set_detect_range(11.0);

	let (near, _) = step(&empty_world(), vec![mine((12.0, 10.0)), raven.clone()], &[]);
	assert!(near.unit(1).unwrap().is_revealed);

	let (far, _) = step(&empty_world(), vec![mine((60.0, 60.0)), raven], &[]);
	assert!(!far.unit(1).unwrap().is_revealed);
}

#[test]
fn events_report_new_finished_and_dead_units() {
	let mut building = unit(2, UnitTypeId::Pylon, ProtoAlliance::Self_, (2.0, 2.0));
	building.set_build_progress(0.5);
	let probe = unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0));

	let (first, events) = step(&empty_world(), vec![probe.clone(), building.clone()], &[]);
	assert!(events.iter().any(|e| matches!(e, Event::UnitCreated(1))));
	assert!(events.iter().any(|e| matches!(e, Event::ConstructionStarted(2))));

	let (second, events) = step(&first, vec![probe.clone(), building.clone()], &[]);
	assert!(events.is_empty());

	building.set_build_progress(1.0);
	let (third, events) = step(&second, vec![probe, building.clone()], &[]);
	assert!(events.iter().any(|e| matches!(e, Event::ConstructionComplete(2))));

	let (fourth, events) = step(&third, vec![building], &[1]);
	assert!(events
		.iter()
		.any(|e| matches!(e, Event::UnitDestroyed(1, Some(Alliance::Own)))));
	assert!(fourth.unit(1).is_none());
}

#[test]
fn mined_out_minerals_leave_their_expansion() {
	let (world, events) = step(&empty_world(), vec![], &[100]);
	assert!(events
		.iter()
		.any(|e| matches!(e, Event::UnitDestroyed(100, Some(Alliance::Neutral)))));
	assert!(world.expansions[0].minerals.is_empty());
}

/// Building the next world doesn't change the previous one.
#[test]
fn previous_world_stays_intact() {
	let (first, _) = step(
		&empty_world(),
		vec![unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0))],
		&[100],
	);
	let tag_before = first.unit(1).unwrap().tag;
	let (second, _) = step(&first, vec![], &[1]);
	assert!(second.unit(1).is_none());
	assert_eq!(first.unit(1).unwrap().tag, tag_before);
}

#[test]
fn identical_commands_are_merged() {
	let (world, _) = step(
		&empty_world(),
		vec![
			unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0)),
			unit(2, UnitTypeId::Probe, ProtoAlliance::Self_, (2.0, 2.0)),
		],
		&[],
	);
	let mut cmds = Commands::default();
	for w in world.my().workers() {
		cmds.gather(w, 100, false);
	}
	let actions = cmds.take_actions();
	assert_eq!(actions.len(), 1);
	assert!(matches!(&actions[0], Action::UnitCommand(_, _, units, _) if units == &[1, 2]));
	assert!(cmds.take_actions().is_empty());
}

fn with_creep(res: &mut ResponseObservation, value: u8) {
	let raw = res
		.observation
		.mut_or_insert_default()
		.raw_data
		.mut_or_insert_default();
	raw.map_state.mut_or_insert_default().creep =
		Some(image(vec![value; (MAP_SIZE * MAP_SIZE / 8) as usize])).into();
}

/// A set bit in SC2's creep map means creep. The old `Unit::real_speed` has this backwards.
#[test]
fn speed_bonus_applies_on_creep_only() {
	let mut prev = empty_world();
	let mut data = GameData::clone(&prev.map.game_data);
	data.units
		.insert(UnitTypeId::Queen, unit_type(UnitTypeId::Queen, &[]));
	let map = MapData {
		game_data: data,
		player_id: 1,
		enemy_player_id: 2,
		opponent_id: String::new(),
		race: Race::Zerg,
		race_values: RACE_VALUES[&Race::Zerg].clone(),
		game_info: Default::default(),
		start_location: Point2::default(),
		enemy_start: Point2::default(),
		start_center: Point2::default(),
		enemy_start_center: Point2::default(),
		ramps: Default::default(),
		vision_blockers: vec![],
	};
	prev.map = Arc::new(map);

	let speed = |creep: u8| {
		let mut res = observation(
			vec![unit(1, UnitTypeId::Queen, ProtoAlliance::Self_, (5.0, 5.0))],
			&[],
		);
		with_creep(&mut res, creep);
		let (world, _) = build_world(&prev, &res, Default::default());
		assert_eq!(world.has_creep(Point2::new(5.0, 5.0)), creep != 0);
		world.unit(1).unwrap().real_speed()
	};
	assert_eq!(speed(0), 3.0);
	assert!((speed(0xFF) - 3.0 * 2.67).abs() < 0.001);
}

/// The old code only labels enemy deaths when the unit was a hallucination or cached.
#[test]
fn enemy_deaths_are_labelled_as_enemy() {
	let marine = unit(1, UnitTypeId::Marine, ProtoAlliance::Enemy, (1.0, 1.0));
	let (first, _) = step(&empty_world(), vec![marine], &[]);
	let (_, events) = step(&first, vec![], &[1]);
	assert!(events
		.iter()
		.any(|e| matches!(e, Event::UnitDestroyed(1, Some(Alliance::Enemy)))));
}

#[test]
fn death_of_a_neutral_unit_or_a_stranger_is_labelled_correctly() {
	let rock = unit(5, UnitTypeId::Marine, ProtoAlliance::Neutral, (1.0, 1.0));
	let (first, _) = step(&empty_world(), vec![rock], &[]);
	let (_, events) = step(&first, vec![], &[5, 77]);
	assert!(events
		.iter()
		.any(|e| matches!(e, Event::UnitDestroyed(5, Some(Alliance::Neutral)))));
	assert!(events.iter().any(|e| matches!(e, Event::UnitDestroyed(77, None))));
}

fn observation_hidden_at(units: Vec<ProtoUnit>, hidden: bool) -> ResponseObservation {
	let mut res = observation(units, &[]);
	if hidden {
		let tiles = (MAP_SIZE * MAP_SIZE) as usize;
		let raw = res
			.observation
			.mut_or_insert_default()
			.raw_data
			.mut_or_insert_default();
		raw.map_state.mut_or_insert_default().visibility = Some(image(vec![1; tiles])).into();
	}
	res
}

#[test]
fn enemy_memory_keeps_units_that_left_vision() {
	let marine = unit(1, UnitTypeId::Marine, ProtoAlliance::Enemy, (9.0, 9.0));
	let (first, _) = step(&empty_world(), vec![marine], &[]);
	assert_eq!(first.enemy_memory().units().len(), 1);

	// Its last position is in fog now, so it's believed to still be there
	let (second, _) = build_world(&first, &observation_hidden_at(vec![], true), Default::default());
	assert!(second.unit(1).is_none());
	assert_eq!(second.enemy().all().len(), 0);
	let remembered = second.enemy_memory().all().get(1).unwrap();
	assert!(remembered.is_hidden());
	assert_eq!(remembered.position, Point2::new(9.0, 9.0));
	assert_eq!(second.remembered_counter().count(UnitTypeId::Marine), 1);

	// The position is visible and it's not there, so it left
	let (third, _) = step(&second, vec![], &[]);
	assert_eq!(third.enemy_memory().all().len(), 0);
}

#[test]
fn memory_only_units_do_not_leak_into_other_views() {
	let marine = unit(1, UnitTypeId::Marine, ProtoAlliance::Enemy, (9.0, 9.0));
	let (first, _) = step(&empty_world(), vec![marine], &[]);
	let (second, _) = build_world(
		&first,
		&observation_hidden_at(
			vec![unit(2, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0))],
			true,
		),
		Default::default(),
	);
	assert_eq!(second.all().len(), 1);
	assert_eq!(second.enemy_counter().count(UnitTypeId::Marine), 0);
	// Only current units count for damage on the next step
	let (third, _) = step(&second, vec![], &[]);
	assert!(third.last_health.is_empty());
}

#[test]
fn enemy_memory_forgets_dead_units_and_labels_them() {
	let marine = unit(1, UnitTypeId::Marine, ProtoAlliance::Enemy, (9.0, 9.0));
	let (first, _) = step(&empty_world(), vec![marine], &[]);
	let (second, _) = build_world(&first, &observation_hidden_at(vec![], true), Default::default());
	let mut res = observation_hidden_at(vec![], true);
	let raw = res
		.observation
		.mut_or_insert_default()
		.raw_data
		.mut_or_insert_default();
	raw.event.mut_or_insert_default().dead_units = vec![1];
	let (third, events) = build_world(&second, &res, Default::default());
	assert_eq!(third.enemy_memory().all().len(), 0);
	assert!(events
		.iter()
		.any(|e| matches!(e, Event::UnitDestroyed(1, Some(Alliance::Enemy)))));
}

#[test]
fn sorted_and_found_groups_can_still_look_up_tags() {
	let (world, _) = step(
		&empty_world(),
		vec![
			unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (9.0, 9.0)),
			unit(2, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0)),
			unit(3, UnitTypeId::Probe, ProtoAlliance::Self_, (5.0, 5.0)),
		],
		&[],
	);
	let workers = world.my().workers();
	let sorted = workers.sorted(|u| u.position.x);
	assert_eq!(sorted.tags().collect::<Vec<_>>(), [2, 3, 1]);
	assert_eq!(sorted.get(1).unwrap().tag, 1);
	assert!(sorted.get(99).is_none());

	let found = workers.find_tags(&[3, 1, 99]);
	assert_eq!(found.tags().collect::<Vec<_>>(), [3, 1]);

	let collected = world.units_from(workers.iter().filter(|u| u.tag != 2));
	assert_eq!(collected.tags().collect::<Vec<_>>(), [1, 3]);
	assert_eq!(workers.furthest(Point2::new(0.0, 0.0)).unwrap().tag, 1);
	assert_eq!(workers.sum(|u| u.tag), 6);
	assert_eq!(workers.max_value(|u| u.tag), Some(3));
}

#[test]
fn counters_and_budget() {
	let mut first_world = empty_world();
	let mut data = GameData::clone(&first_world.map.game_data);
	data.units.get_mut(&UnitTypeId::Probe).unwrap().mineral_cost = 50;
	data.units.get_mut(&UnitTypeId::Probe).unwrap().food_required = 1.0;
	first_world.map = Arc::new(MapData {
		game_data: data,
		player_id: 1,
		enemy_player_id: 2,
		opponent_id: String::new(),
		race: Race::Protoss,
		race_values: RACE_VALUES[&Race::Protoss].clone(),
		game_info: Default::default(),
		start_location: Point2::default(),
		enemy_start: Point2::default(),
		start_center: Point2::default(),
		enemy_start_center: Point2::default(),
		ramps: Default::default(),
		vision_blockers: vec![],
	});
	let mut res = observation(
		vec![unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0))],
		&[],
	);
	let common = res
		.observation
		.mut_or_insert_default()
		.player_common
		.mut_or_insert_default();
	common.set_minerals(70);
	common.set_food_cap(15);
	common.set_food_used(14);
	let (world, _) = build_world(&first_world, &res, Default::default());
	assert_eq!(world.counter().count(UnitTypeId::Probe), 1);

	let mut budget = Budget::new(&world);
	assert!(budget.can_afford(&world, UnitTypeId::Probe, true));
	budget.subtract_resources(&world, UnitTypeId::Probe, true);
	assert_eq!(
		(budget.minerals, budget.supply_left, budget.supply_used),
		(20, 0, 15)
	);
	assert!(!budget.can_afford(&world, UnitTypeId::Probe, false));
	// The world still reports what SC2 said
	assert_eq!(world.minerals, 70);
}

#[test]
fn weapon_stats_account_for_armor_and_attributes() {
	use crate::game_data::{TargetType, Weapon};
	let mut prev = empty_world();
	let mut data = GameData::clone(&prev.map.game_data);
	data.units.get_mut(&UnitTypeId::Marine).unwrap().weapons = vec![Weapon {
		target: TargetType::Any,
		damage: 6,
		damage_bonus: vec![(Attribute::Light, 4)],
		attacks: 1,
		range: 5.0,
		speed: 0.5,
	}];
	data.units.get_mut(&UnitTypeId::Probe).unwrap().attributes = vec![Attribute::Light];
	data.units.get_mut(&UnitTypeId::Probe).unwrap().armor = 1;
	prev.map = Arc::new(MapData {
		game_data: data,
		player_id: 1,
		enemy_player_id: 2,
		opponent_id: String::new(),
		race: Race::Terran,
		race_values: RACE_VALUES[&Race::Terran].clone(),
		game_info: Default::default(),
		start_location: Point2::default(),
		enemy_start: Point2::default(),
		start_center: Point2::default(),
		enemy_start_center: Point2::default(),
		ramps: Default::default(),
		vision_blockers: vec![],
	});
	let mut probe = unit(2, UnitTypeId::Probe, ProtoAlliance::Enemy, (1.0, 4.0));
	probe.set_health(40.0);
	let (world, _) = step(
		&prev,
		vec![
			unit(1, UnitTypeId::Marine, ProtoAlliance::Self_, (1.0, 1.0)),
			probe,
		],
		&[],
	);
	let marine = world.unit(1).unwrap();
	let target = world.unit(2).unwrap();
	// (6 + 4 light bonus - 1 armor) / 0.5 speed
	assert_eq!(marine.real_weapon_vs(target), (18.0, 5.0));
	assert_eq!(
		marine.calculate_weapon_stats(CalcTarget::Unit(target)),
		(18.0, 5.0)
	);
	assert_eq!(marine.real_ground_weapon(&[]), (12.0, 5.0));
	assert!(marine.in_real_range(target, 0.0));
	assert!(marine.can_attack_unit(target));
	assert_eq!(target.real_ground_range(), 0.0);
}

#[test]
fn sleeping_lasts_until_its_frame() {
	let (mut world, _) = step(
		&empty_world(),
		vec![unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0))],
		&[],
	);
	world.available_frames.insert(1, 10);
	assert!(world.unit(1).unwrap().is_sleeping());
	world.game_loop = 10;
	assert!(!world.unit(1).unwrap().is_sleeping());
}

/// A client newer than the ID tables reports IDs they don't have. The old parser panics on these.
#[test]
fn unknown_ids_do_not_end_the_game() {
	let mut worker = unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0));
	let mut order = sc2_proto::raw::UnitOrder::new();
	order.set_ability_id(999_999);
	worker.orders.push(order);
	worker.buff_ids.push(999_999);
	let mystery = {
		let mut u = unit(2, UnitTypeId::Probe, ProtoAlliance::Enemy, (2.0, 2.0));
		u.set_unit_type(999_999);
		u
	};
	let (world, _) = step(&empty_world(), vec![worker, mystery], &[]);

	assert!(world.unit(2).is_none());
	let worker = world.unit(1).unwrap();
	assert!(worker.buffs.is_empty());
	// Busy doing something unknown is still busy, so a bot won't re-task it
	assert!(!worker.is_idle());
	assert_eq!(worker.ordered_ability(), Some(AbilityId::NullNull));
	assert_eq!(world.my().workers().idle().len(), 0);
}

#[cfg(feature = "rayon")]
#[test]
fn parallel_helpers_match_the_sequential_ones() {
	use rayon::iter::ParallelIterator;
	let (world, _) = step(
		&empty_world(),
		vec![
			unit(1, UnitTypeId::Probe, ProtoAlliance::Self_, (9.0, 9.0)),
			unit(2, UnitTypeId::Probe, ProtoAlliance::Self_, (1.0, 1.0)),
			unit(3, UnitTypeId::Probe, ProtoAlliance::Self_, (5.0, 5.0)),
		],
		&[],
	);
	let workers = world.my().workers();
	assert_eq!(workers.par_sorted(|u| u.position.x).tags().collect::<Vec<_>>(), [2, 3, 1]);
	assert_eq!(workers.par_filter(|u| u.tag != 2).tags().collect::<Vec<_>>(), [1, 3]);
	assert_eq!(workers.par_tags().sum::<u64>(), 6);
	assert_eq!(workers.par_iter().count(), 3);
}
