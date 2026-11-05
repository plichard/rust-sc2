//! Turns an observation and the previous world into the next world.

use super::{unit::known, units::PlayerGroups, world::Groups, Event, UnitData, UnitTag, World};
use crate::{
	action::{Action, ActionError, Target},
	consts::{BURROWED_IDS, FRAMES_PER_SECOND, INHIBITOR_IDS},
	distance::Distance,
	game_data::{Attribute, GameData},
	game_state::{Alert, Alliance, AvailableAbility, ChatMessage, Common, Effect, PsionicMatrix, Radar},
	geometry::Point2,
	ids::{AbilityId, EffectId, UnitTypeId, UpgradeId},
	pixel_map::{PixelMap, VisibilityMap},
	score::Score,
	unit::DisplayType,
	FromProto,
};
use indexmap::IndexMap;
use num_traits::FromPrimitive;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use sc2_proto::{
	raw::action_raw::Action as ProtoRawAction,
	sc2api::{Action as ProtoAction, ResponseObservation},
};
use std::{hash::BuildHasherDefault, sync::Arc};

type FxIndexMap<K, V> = IndexMap<K, V, BuildHasherDefault<FxHasher>>;

pub(super) fn is_mineral_type(type_id: UnitTypeId) -> bool {
	matches!(
		type_id,
		UnitTypeId::RichMineralField
			| UnitTypeId::RichMineralField750
			| UnitTypeId::MineralField
			| UnitTypeId::MineralField450
			| UnitTypeId::MineralField750
			| UnitTypeId::LabMineralField
			| UnitTypeId::LabMineralField750
			| UnitTypeId::PurifierRichMineralField
			| UnitTypeId::PurifierRichMineralField750
			| UnitTypeId::PurifierMineralField
			| UnitTypeId::PurifierMineralField750
			| UnitTypeId::BattleStationMineralField
			| UnitTypeId::BattleStationMineralField750
			| UnitTypeId::MineralFieldOpaque
			| UnitTypeId::MineralFieldOpaque900
	)
}

pub(super) fn is_geyser_type(type_id: UnitTypeId) -> bool {
	matches!(
		type_id,
		UnitTypeId::VespeneGeyser
			| UnitTypeId::SpacePlatformGeyser
			| UnitTypeId::RichVespeneGeyser
			| UnitTypeId::ProtossVespeneGeyser
			| UnitTypeId::PurifierVespeneGeyser
			| UnitTypeId::ShakurasVespeneGeyser
	)
}

fn has_attribute(game_data: &GameData, type_id: UnitTypeId, attribute: Attribute) -> bool {
	game_data
		.units
		.get(&type_id)
		.map_or(false, |d| d.attributes.contains(&attribute))
}

/// `false` when a detector or scan within `gap` of the unit can see it.
fn is_invisible(u: &UnitData, detectors: &[&UnitData], scans: &[&Effect], gap: f32) -> bool {
	let additional = u.radius + gap;
	!detectors.iter().any(|d| {
		u.position
			.is_closer(additional + d.radius + d.detect_range, d.position)
	}) && !scans.iter().any(|scan| {
		scan.positions
			.iter()
			.any(|p| u.position.is_closer(additional + scan.radius, *p))
	})
}

/// Adds a player's unit to the groups it belongs in.
fn add_to_groups(group: &mut PlayerGroups, id: u32, u: &UnitData, is_structure: bool, is_mine: bool) {
	group.all.push(id);
	if !is_structure {
		group.units.push(id);
		match u.type_id {
			UnitTypeId::SCV | UnitTypeId::Probe | UnitTypeId::Drone => group.workers.push(id),
			UnitTypeId::Larva => group.larvas.push(id),
			_ => {}
		}
	} else if is_mine && u.display_type.is_placeholder() {
		group.placeholders.push(id);
	} else {
		group.structures.push(id);
		match u.type_id {
			UnitTypeId::CommandCenter
			| UnitTypeId::OrbitalCommand
			| UnitTypeId::PlanetaryFortress
			| UnitTypeId::Hatchery
			| UnitTypeId::Lair
			| UnitTypeId::Hive
			| UnitTypeId::Nexus
			| UnitTypeId::CommandCenterFlying
			| UnitTypeId::OrbitalCommandFlying => group.townhalls.push(id),
			UnitTypeId::Refinery
			| UnitTypeId::RefineryRich
			| UnitTypeId::Assimilator
			| UnitTypeId::AssimilatorRich
			| UnitTypeId::Extractor
			| UnitTypeId::ExtractorRich => group.gas_buildings.push(id),
			_ => {}
		}
	}
}

fn count_enemy(
	u: &UnitData,
	is_structure: bool,
	current: &mut FxHashMap<UnitTypeId, usize>,
	ordered: &mut FxHashMap<UnitTypeId, usize>,
) {
	if is_structure {
		let counter = if u.is_ready() { current } else { ordered };
		*counter.entry(u.type_id).or_default() += 1;
	} else if !u.is_hallucination {
		*current.entry(u.type_id).or_default() += 1;
	}
}

/// `Action::from_proto` panics on anything else, so those are filtered out first.
fn action_is_known(action: &ProtoAction) -> bool {
	let ability = |id: i32| known("ability", id as i64, AbilityId::from_i32(id)).is_some();
	match action.action_raw.action.as_ref() {
		Some(ProtoRawAction::UnitCommand(command)) => ability(command.ability_id()),
		Some(ProtoRawAction::ToggleAutocast(toggle)) => ability(toggle.ability_id()),
		Some(ProtoRawAction::CameraMove(_)) | None => true,
		Some(_) => false,
	}
}

/// Builds the next [`World`]. A pure function: it talks to nothing but its arguments,
/// so it can be tested without running SC2.
pub(super) fn build_world(
	prev: &World,
	response: &ResponseObservation,
	abilities: FxHashMap<UnitTag, FxHashSet<AbilityId>>,
) -> (World, Vec<Event>) {
	let map = Arc::clone(&prev.map);
	let game_data = &map.game_data;
	let obs = &response.observation;
	let raw = &obs.raw_data;
	let map_state = &raw.map_state;

	let mut events = vec![];
	let mut owned_tags = prev.owned_tags.clone();
	let mut under_construction = prev.under_construction.clone();
	let mut hallucinations = prev.hallucinations.clone();
	let mut expansions = prev.expansions.clone();
	let mut max_cooldowns = prev.max_cooldowns.clone();
	let mut available_frames = prev.available_frames.clone();
	let mut enemy_race = prev.enemy_race;
	let mut memory = prev
		.enemy_memory()
		.all()
		.iter()
		.map(|u| (u.tag, u.data().clone()))
		.collect::<FxIndexMap<UnitTag, UnitData>>();

	// Dead units
	let dead_units = raw
		.event
		.dead_units
		.iter()
		.copied()
		.map(UnitTag)
		.collect::<Vec<_>>();
	for tag in &dead_units {
		let last_seen = prev.unit(*tag).map(|u| u.alliance);
		let was_hallucination = hallucinations.remove(tag);
		let was_remembered = memory.shift_remove(tag).is_some();

		let alliance = if owned_tags.remove(tag) {
			available_frames.remove(tag);
			under_construction.remove(tag);
			Some(Alliance::Own)
		} else if was_hallucination || was_remembered {
			Some(Alliance::Enemy)
		} else {
			last_seen
		};
		events.push(Event::UnitDestroyed(*tag, alliance));
	}

	let effects = raw
		.effects
		.iter()
		.filter_map(|e| {
			Some(Effect {
				id: known("effect", e.effect_id() as i64, EffectId::from_u32(e.effect_id()))?,
				positions: e.pos.iter().map(Point2::from_proto).collect(),
				alliance: Alliance::from_proto(e.alliance()),
				owner: e.owner() as u32,
				radius: e.radius(),
			})
		})
		.collect::<Vec<_>>();

	// Units. They're plain values here, so anything derived from
	// other units is simply written before the world is put together.
	let visibility = VisibilityMap::from_proto(map_state.visibility.get_or_default());
	let mut units = raw
		.units
		.iter()
		.filter_map(|u| UnitData::from_proto(&visibility, u))
		.collect::<Vec<_>>();

	for u in units.iter_mut().filter(|u| u.alliance.is_enemy()) {
		// Once flagged, a unit stays a hallucination
		if u.is_hallucination {
			hallucinations.insert(u.tag);
		} else if hallucinations.contains(&u.tag) {
			u.is_hallucination = true;
		}
	}

	let revealed = {
		let detectors = units
			.iter()
			.filter(|u| u.alliance.is_enemy() && u.is_detector())
			.collect::<Vec<_>>();
		let scans = effects
			.iter()
			.filter(|e| e.id == EffectId::ScannerSweep && e.alliance.is_enemy())
			.collect::<Vec<_>>();
		if detectors.is_empty() && scans.is_empty() {
			vec![]
		} else {
			units
				.iter()
				.enumerate()
				.filter(|(_, u)| {
					u.alliance.is_mine() && !u.is_revealed && !is_invisible(u, &detectors, &scans, 1.0)
				})
				.map(|(i, _)| i)
				.collect()
		}
	};
	for i in revealed {
		units[i].is_revealed = true;
	}

	// Sorting units into groups
	let mut groups = Groups::default();
	let mut index = FxHashMap::default();
	let mut techlab_tags = FxHashSet::default();
	let mut reactor_tags = FxHashSet::default();
	let mut bases = FxHashMap::default();
	let enemy_start_hidden = visibility
		.get(<(usize, usize)>::from(map.enemy_start))
		.map_or(true, |p| p.is_hidden());
	if enemy_start_hidden {
		bases.insert(map.enemy_start, (Alliance::Enemy, None));
	}

	for (i, u) in units.iter().enumerate() {
		let id = i as u32;
		index.insert(u.tag, id);
		groups.all.push(id);

		match u.alliance {
			Alliance::Neutral => match u.type_id {
				UnitTypeId::XelNagaTower => groups.watchtowers.push(id),
				type_id if is_mineral_type(type_id) => {
					groups.resources.push(id);
					groups.mineral_fields.push(id);
				}
				type_id if is_geyser_type(type_id) => {
					groups.resources.push(id);
					groups.vespene_geysers.push(id);
				}
				type_id if INHIBITOR_IDS.contains(&type_id) => groups.inhibitor_zones.push(id),
				_ => groups.destructables.push(id),
			},
			Alliance::Own | Alliance::Enemy => {
				let is_mine = u.alliance.is_mine();
				let is_structure = has_attribute(game_data, u.type_id, Attribute::Structure);

				if is_mine {
					if let Some(cooldown) = u.weapon_cooldown {
						max_cooldowns
							.entry(u.type_id)
							.and_modify(|max| *max = cooldown.max(*max))
							.or_insert(cooldown);
					}
				}

				let group = if is_mine {
					&mut groups.my
				} else {
					&mut groups.enemy
				};
				add_to_groups(group, id, u, is_structure, is_mine);

				if is_structure && !(is_mine && u.display_type.is_placeholder()) {
					match u.type_id {
						UnitTypeId::CommandCenter
						| UnitTypeId::OrbitalCommand
						| UnitTypeId::PlanetaryFortress
						| UnitTypeId::Hatchery
						| UnitTypeId::Lair
						| UnitTypeId::Hive
						| UnitTypeId::Nexus => {
							bases.insert(u.position, (u.alliance, Some(u.tag)));
						}
						UnitTypeId::TechLab
						| UnitTypeId::BarracksTechLab
						| UnitTypeId::FactoryTechLab
						| UnitTypeId::StarportTechLab
							if is_mine =>
						{
							techlab_tags.insert(u.tag);
						}
						UnitTypeId::Reactor
						| UnitTypeId::BarracksReactor
						| UnitTypeId::FactoryReactor
						| UnitTypeId::StarportReactor
							if is_mine =>
						{
							reactor_tags.insert(u.tag);
						}
						_ => {}
					}
				}
			}
			Alliance::Ally => {}
		}
	}

	for exp in &mut expansions {
		let (alliance, base) = bases.remove(&exp.loc).unwrap_or((Alliance::Neutral, None));
		exp.alliance = alliance;
		exp.base = base;
	}

	// Counters
	let mut orders = FxHashMap::default();
	let mut current_units = FxHashMap::default();
	let mut constructed = FxHashMap::default();
	for &id in &groups.my.all {
		let u = &units[id as usize];
		if u.is_hallucination {
			continue;
		}
		for order in &u.orders {
			if order.ability.is_constructing() {
				if let Target::Pos(pos) = order.target {
					constructed.insert((pos, order.ability), false);
				}
			}
			*orders.entry(order.ability).or_default() += 1;
		}
		if u.is_ready() {
			*current_units.entry(u.type_id).or_default() += 1;
		} else if let Some(ability) = game_data.units.get(&u.type_id).and_then(|d| d.ability) {
			constructed.entry((u.position, ability)).or_insert(true);
		}
	}
	for ((_, ability), standalone) in constructed {
		if standalone {
			*orders.entry(ability).or_default() += 1;
		}
	}

	let mut enemies_ordered = FxHashMap::default();
	let mut enemies_current = FxHashMap::default();
	for &id in &groups.enemy.all {
		let u = &units[id as usize];
		let is_structure = has_attribute(game_data, u.type_id, Attribute::Structure);
		count_enemy(u, is_structure, &mut enemies_current, &mut enemies_ordered);
	}

	// Events
	for &id in &groups.my.all {
		let u = &units[id as usize];
		let is_structure = has_attribute(game_data, u.type_id, Attribute::Structure);
		if owned_tags.insert(u.tag) {
			if !is_structure {
				events.push(Event::UnitCreated(u.tag));
			} else if !(u.display_type.is_placeholder() || u.type_id == UnitTypeId::KD8Charge) {
				if u.is_ready() {
					events.push(Event::ConstructionComplete(u.tag));
				} else {
					events.push(Event::ConstructionStarted(u.tag));
					under_construction.insert(u.tag);
				}
			}
		} else if u.is_ready() && under_construction.remove(&u.tag) {
			events.push(Event::ConstructionComplete(u.tag));
		}
	}
	if enemy_race.is_random() {
		let detected = groups
			.enemy
			.all
			.iter()
			.filter_map(|&id| game_data.units.get(&units[id as usize].type_id))
			.map(|d| d.race)
			.find(|r| !r.is_random());
		if let Some(race) = detected {
			events.push(Event::RandomRaceDetected(race));
			enemy_race = race;
		}
	}

	// Enemy memory
	let changed = update_memory(
		&mut memory,
		&units,
		&groups,
		&visibility,
		&effects,
		game_data,
		enemy_race.is_zerg(),
	);
	let current_units_count = units.len();
	let mut remembered_current = FxHashMap::default();
	let mut remembered_ordered = FxHashMap::default();
	for u in memory.values() {
		let is_structure = has_attribute(game_data, u.type_id, Attribute::Structure);
		count_enemy(u, is_structure, &mut remembered_current, &mut remembered_ordered);

		// A unit seen on this step and left as it is can share the current unit's entry
		let id = match index.get(&u.tag) {
			Some(id) if !changed.contains(&u.tag) => *id,
			_ => {
				units.push(u.clone());
				(units.len() - 1) as u32
			}
		};
		add_to_groups(&mut groups.memory, id, u, is_structure, false);
		// Burrowed drones are still workers. Larvas hatch or die rather than wander off,
		// so only the ones seen right now count.
		if u.type_id == UnitTypeId::DroneBurrowed {
			groups.memory.workers.push(id);
		}
		if u.type_id == UnitTypeId::Larva && id as usize >= current_units_count {
			groups.memory.larvas.pop();
		}
	}

	let common = &obs.player_common;
	let game_loop = obs.game_loop();
	let world = World {
		game_loop,
		game_step: prev.game_step,
		time: game_loop as f32 / FRAMES_PER_SECOND,
		minerals: common.minerals(),
		vespene: common.vespene(),
		supply_army: common.food_army(),
		supply_workers: common.food_workers(),
		supply_cap: common.food_cap(),
		supply_used: common.food_used(),
		supply_left: common.food_cap().saturating_sub(common.food_used()),
		common: Common {
			player_id: common.player_id(),
			minerals: common.minerals(),
			vespene: common.vespene(),
			food_cap: common.food_cap(),
			food_used: common.food_used(),
			food_army: common.food_army(),
			food_workers: common.food_workers(),
			idle_worker_count: common.idle_worker_count(),
			army_count: common.army_count(),
			warp_gate_count: common.warp_gate_count(),
			larva_count: common.larva_count(),
		},
		enemy_race,
		upgrades: raw
			.player
			.upgrade_ids
			.iter()
			.filter_map(|u| known("upgrade", *u as i64, UpgradeId::from_u32(*u)))
			.collect(),
		enemy_upgrades: prev.enemy_upgrades.clone(),
		creep: PixelMap::from_proto(&map_state.creep.get_or_default()),
		visibility,
		effects,
		radars: raw
			.radar
			.iter()
			.map(|r| Radar {
				pos: Point2::from_proto(r.pos.get_or_default()),
				radius: r.radius(),
			})
			.collect(),
		psionic_matrix: raw
			.player
			.power_sources
			.iter()
			.map(PsionicMatrix::from_proto)
			.collect(),
		camera: Point2::from_proto(raw.player.camera.get_or_default()),
		alerts: obs
			.alerts
			.iter()
			.map(|a| Alert::from_proto(a.enum_value_or_default()))
			.collect(),
		available_abilities: obs
			.abilities
			.iter()
			.filter_map(|a| {
				Some(AvailableAbility {
					id: known(
						"ability",
						a.ability_id() as i64,
						AbilityId::from_i32(a.ability_id()),
					)?,
					requires_point: a.requires_point(),
				})
			})
			.collect(),
		score: Score::from_proto(&obs.score),
		chat: response
			.chat
			.iter()
			.map(|m| ChatMessage {
				player_id: m.player_id(),
				message: m.message().to_string(),
			})
			.collect(),
		actions: response
			.actions
			.iter()
			.filter(|a| action_is_known(a))
			.filter_map(Option::<Action>::from_proto)
			.collect(),
		action_errors: response
			.action_errors
			.iter()
			.filter(|e| {
				known(
					"ability",
					e.ability_id() as i64,
					AbilityId::from_u64(e.ability_id()),
				)
				.is_some()
			})
			.map(ActionError::from_proto)
			.collect(),
		dead_units,
		expansions,
		orders,
		current_units,
		enemies_ordered,
		enemies_current,
		remembered_ordered,
		remembered_current,
		last_health: prev.units[..prev.current_units_count]
			.iter()
			.filter_map(|u| Some((u.tag, u.hits()?)))
			.collect(),
		units,
		current_units_count,
		index,
		groups,
		abilities,
		techlab_tags,
		reactor_tags,
		max_cooldowns,
		available_frames,
		hallucinations,
		owned_tags,
		under_construction,
		map,
	};
	(world, events)
}

/// Works out what to believe about enemies that aren't in this observation.
/// Returns the tags of units seen on this step whose remembered entry differs from SC2's report.
fn update_memory(
	memory: &mut FxIndexMap<UnitTag, UnitData>,
	units: &[UnitData],
	groups: &Groups,
	visibility: &VisibilityMap,
	effects: &[Effect],
	game_data: &GameData,
	enemy_is_zerg: bool,
) -> FxHashSet<UnitTag> {
	let is_structure = |u: &UnitData| has_attribute(game_data, u.type_id, Attribute::Structure);
	let unit = |id: &u32| &units[*id as usize];

	for u in groups.enemy.all.iter().map(unit) {
		memory.insert(u.tag, u.clone());
	}

	let detectors = groups
		.my
		.all
		.iter()
		.map(unit)
		.filter(|u| u.is_detector())
		.collect::<Vec<_>>();
	let scans = effects
		.iter()
		.filter(|e| e.id == EffectId::ScannerSweep && e.alliance.is_mine())
		.collect::<Vec<_>>();
	let visible = |pos: Point2| {
		visibility
			.get(<(usize, usize)>::from(pos))
			.map_or(false, |p| p.is_visible())
	};
	let seen_now = groups
		.enemy
		.all
		.iter()
		.map(|id| unit(id).tag)
		.collect::<FxHashSet<_>>();

	let mut remove = vec![];
	let mut cloaked = FxHashSet::default();
	let mut burrowed = vec![];
	let mut hidden = vec![];

	for u in memory.values() {
		let invisible = || is_invisible(u, &detectors, &scans, 0.0);

		if seen_now.contains(&u.tag) {
			// Undetected burrowed units can't be attacked
			if u.is_burrowed && u.is_revealed && invisible() {
				cloaked.insert(u.tag);
			}
		} else if u.is_flying || !is_structure(u) {
			if !visible(u.position) {
				// Out of vision, so it may well still be there
				hidden.push(u.tag);
			} else if u.is_visible() {
				// It was visible and it's gone. A zerg ground unit has probably burrowed.
				let drone_morphed = u.type_id == UnitTypeId::Drone
					&& groups.enemy.structures.iter().map(unit).any(|s| {
						(s.build_progress < 0.1 || (s.type_id == UnitTypeId::Extractor && s.is_ready()))
							&& u.position.is_closer(u.radius + s.radius + 1.0, s.position)
					});
				let cant_burrow = u.is_flying
					|| matches!(
						u.type_id,
						UnitTypeId::Changeling
							| UnitTypeId::ChangelingZealot
							| UnitTypeId::ChangelingMarineShield
							| UnitTypeId::ChangelingMarine
							| UnitTypeId::ChangelingZerglingWings
							| UnitTypeId::ChangelingZergling
							| UnitTypeId::Broodling
							| UnitTypeId::Larva | UnitTypeId::Egg
					);
				if enemy_is_zerg && !cant_burrow && !drone_morphed && invisible() {
					burrowed.push(u.tag);
				} else {
					remove.push(u.tag);
				}
			} else if !(u.is_burrowed && invisible()) {
				// It wasn't visible, or was burrowed and detected, so it probably moved
				remove.push(u.tag);
			}
		} else {
			// A structure that's no longer reported was destroyed
			remove.push(u.tag);
		}
	}

	for tag in remove {
		memory.shift_remove(&tag);
	}
	for tag in &cloaked {
		if let Some(u) = memory.get_mut(tag) {
			u.display_type = DisplayType::Hidden;
			u.is_cloaked = true;
			u.is_revealed = false;
		}
	}
	for tag in burrowed {
		if let Some(u) = memory.get_mut(&tag) {
			if let Some(burrowed_id) = BURROWED_IDS.get(&u.type_id) {
				u.display_type = DisplayType::Hidden;
				u.type_id = *burrowed_id;
				u.is_burrowed = true;
				u.is_cloaked = true;
				u.is_revealed = false;
			}
		}
	}
	for tag in hidden {
		if let Some(u) = memory.get_mut(&tag) {
			u.display_type = DisplayType::Hidden;
		}
	}
	cloaked
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
