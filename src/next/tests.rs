//! These run without SC2: observations are built by hand and fed to `build_world`.

use super::*;
use crate::{action::Action, next::Commands};
use crate::{consts::RACE_VALUES, game_data::UnitTypeData, geometry::Point2};
use indexmap::IndexSet;
use num_traits::ToPrimitive;
use sc2_proto::{
	common::ImageData,
	raw::{Alliance as ProtoAlliance, Unit as ProtoUnit},
	sc2api::ResponseObservation,
};

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
