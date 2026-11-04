use super::{units::PlayerGroups, PlayerUnits, UnitData, UnitRef, Units};
use crate::{
	action::Target,
	bot::Expansion,
	consts::{RaceValues, FRAMES_PER_SECOND, INHIBITOR_IDS},
	distance::Distance,
	game_data::{Attribute, GameData},
	game_info::GameInfo,
	game_state::{Alliance, Effect},
	geometry::Point2,
	ids::{AbilityId, EffectId, UnitTypeId, UpgradeId},
	pixel_map::{PixelMap, VisibilityMap},
	player::Race,
	ramp::Ramps,
	Event, FromProto,
};
use num_traits::FromPrimitive;
use rustc_hash::{FxHashMap, FxHashSet};
use sc2_proto::sc2api::ResponseObservation;
use std::sync::Arc;

/// Data that doesn't change during a game. Shared by every [`World`].
pub struct MapData {
	/// Your in-game id.
	pub player_id: u32,
	/// Opponent's in-game id.
	pub enemy_player_id: u32,
	/// Actual race of your bot.
	pub race: Race,
	/// Values which depend on your race.
	pub race_values: RaceValues,
	/// Information about the map.
	pub game_info: GameInfo,
	/// Constant information about abilities, unit types, upgrades, buffs and effects.
	pub game_data: GameData,
	/// Your starting location.
	pub start_location: Point2,
	/// Opponent's starting location.
	pub enemy_start: Point2,
	/// Center of the resources at your start location.
	pub start_center: Point2,
	/// Center of the resources at the opponent's start location.
	pub enemy_start_center: Point2,
	/// Ramps on the map.
	pub ramps: Ramps,
	/// Obstacles that block vision of ground units, but are pathable.
	pub vision_blockers: Vec<Point2>,
}

#[derive(Default, Clone)]
struct Groups {
	all: Vec<u32>,
	my: PlayerGroups,
	enemy: PlayerGroups,
	mineral_fields: Vec<u32>,
	vespene_geysers: Vec<u32>,
	resources: Vec<u32>,
	destructables: Vec<u32>,
	watchtowers: Vec<u32>,
	inhibitor_zones: Vec<u32>,
}

/// The game state of one step.
///
/// [`build_world`] creates a new one from the previous one and the new observation.
/// Nothing changes it while your bot runs, so it has no locks and no interior mutability.
pub struct World {
	/// Data that doesn't change during the game.
	pub map: Arc<MapData>,
	/// Current game tick.
	pub game_loop: u32,
	/// Frames between steps.
	pub game_step: u32,
	/// In-game time in seconds.
	pub time: f32,
	/// Amount of minerals you have.
	pub minerals: u32,
	/// Amount of gas you have.
	pub vespene: u32,
	/// Supply used by your army.
	pub supply_army: u32,
	/// Supply used by your workers.
	pub supply_workers: u32,
	/// Your supply limit.
	pub supply_cap: u32,
	/// Your total used supply.
	pub supply_used: u32,
	/// Your free supply.
	pub supply_left: u32,
	/// Requested or detected race of your opponent.
	pub enemy_race: Race,
	/// Your ready upgrades.
	pub upgrades: FxHashSet<UpgradeId>,
	/// Enemy upgrades you noted with [`Commands`](super::Commands).
	pub enemy_upgrades: FxHashSet<UpgradeId>,
	/// Creep on the map.
	pub creep: PixelMap,
	/// Your visibility map.
	pub visibility: VisibilityMap,
	/// Current effects on the map.
	pub effects: Vec<Effect>,
	/// Tags of units which died since the previous step.
	pub dead_units: Vec<u64>,
	/// All expansions, closest to your start location first.
	pub expansions: Vec<Expansion>,
	/// Orders of your units, counted by ability.
	pub orders: FxHashMap<AbilityId, usize>,
	/// Your ready units, counted by type.
	pub current_units: FxHashMap<UnitTypeId, usize>,
	/// In-progress enemy units, counted by type.
	pub enemies_ordered: FxHashMap<UnitTypeId, usize>,
	/// Ready enemy units, counted by type.
	pub enemies_current: FxHashMap<UnitTypeId, usize>,

	units: Vec<UnitData>,
	index: FxHashMap<u64, u32>,
	groups: Groups,
	pub(super) abilities: FxHashMap<u64, FxHashSet<AbilityId>>,
	pub(super) techlab_tags: FxHashSet<u64>,
	pub(super) reactor_tags: FxHashSet<u64>,
	pub(super) last_health: FxHashMap<u64, u32>,
	pub(super) max_cooldowns: FxHashMap<UnitTypeId, f32>,
	// Remembered from step to step
	hallucinations: FxHashSet<u64>,
	owned_tags: FxHashSet<u64>,
	under_construction: FxHashSet<u64>,
}

impl World {
	/// The state before the first observation.
	pub(super) fn initial(map: Arc<MapData>, enemy_race: Race, expansions: Vec<Expansion>) -> Self {
		Self {
			map,
			game_loop: 0,
			game_step: 1,
			time: 0.0,
			minerals: 0,
			vespene: 0,
			supply_army: 0,
			supply_workers: 0,
			supply_cap: 0,
			supply_used: 0,
			supply_left: 0,
			enemy_race,
			upgrades: Default::default(),
			enemy_upgrades: Default::default(),
			creep: Default::default(),
			visibility: Default::default(),
			effects: Default::default(),
			dead_units: Default::default(),
			expansions,
			orders: Default::default(),
			current_units: Default::default(),
			enemies_ordered: Default::default(),
			enemies_current: Default::default(),
			units: Default::default(),
			index: Default::default(),
			groups: Default::default(),
			abilities: Default::default(),
			techlab_tags: Default::default(),
			reactor_tags: Default::default(),
			last_health: Default::default(),
			max_cooldowns: Default::default(),
			hallucinations: Default::default(),
			owned_tags: Default::default(),
			under_construction: Default::default(),
		}
	}

	#[inline]
	pub(super) fn unit_at(&self, id: u32) -> UnitRef<'_> {
		UnitRef::new(&self.units[id as usize], self)
	}
	#[inline]
	pub(super) fn index_of(&self, tag: u64) -> Option<u32> {
		self.index.get(&tag).copied()
	}
	/// Finds any unit by tag. `None` means it's not in this step's observation.
	pub fn unit(&self, tag: u64) -> Option<UnitRef<'_>> {
		self.index_of(tag).map(|id| self.unit_at(id))
	}
	/// All units, including your own, enemy and neutral.
	pub fn all(&self) -> Units<'_> {
		Units::borrowed(self, &self.groups.all)
	}
	/// Your units.
	pub fn my(&self) -> PlayerUnits<'_> {
		PlayerUnits {
			world: self,
			groups: &self.groups.my,
		}
	}
	/// Opponent's units.
	pub fn enemy(&self) -> PlayerUnits<'_> {
		PlayerUnits {
			world: self,
			groups: &self.groups.enemy,
		}
	}
	/// All mineral fields.
	pub fn mineral_fields(&self) -> Units<'_> {
		Units::borrowed(self, &self.groups.mineral_fields)
	}
	/// All vespene geysers.
	pub fn vespene_geysers(&self) -> Units<'_> {
		Units::borrowed(self, &self.groups.vespene_geysers)
	}
	/// All mineral fields and vespene geysers.
	pub fn resources(&self) -> Units<'_> {
		Units::borrowed(self, &self.groups.resources)
	}
	/// Destructable rocks and other neutral obstacles.
	pub fn destructables(&self) -> Units<'_> {
		Units::borrowed(self, &self.groups.destructables)
	}
	/// Xel'Naga towers.
	pub fn watchtowers(&self) -> Units<'_> {
		Units::borrowed(self, &self.groups.watchtowers)
	}
	/// Inhibitor zones.
	pub fn inhibitor_zones(&self) -> Units<'_> {
		Units::borrowed(self, &self.groups.inhibitor_zones)
	}
	/// Actual terrain height on given position in 3D space.
	pub fn get_z_height<P: Into<(usize, usize)>>(&self, pos: P) -> f32 {
		self.map
			.game_info
			.terrain_height
			.get(pos.into())
			.map_or(0.0, |h| *h as f32 * 32.0 / 255.0 - 16.0)
	}
	/// Checks if given position is visible now.
	pub fn is_visible<P: Into<(usize, usize)>>(&self, pos: P) -> bool {
		self.visibility.get(pos.into()).map_or(false, |p| p.is_visible())
	}
	/// Checks if given position hasn't been explored yet.
	pub fn is_hidden<P: Into<(usize, usize)>>(&self, pos: P) -> bool {
		self.visibility.get(pos.into()).map_or(true, |p| p.is_hidden())
	}
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

/// Builds the next [`World`]. A pure function: it talks to nothing but its arguments,
/// so it can be tested without running SC2.
pub(super) fn build_world(
	prev: &World,
	response: &ResponseObservation,
	abilities: FxHashMap<u64, FxHashSet<AbilityId>>,
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
	let mut enemy_race = prev.enemy_race;

	// Dead units
	let dead_units = raw.event.dead_units.clone();
	for u in &dead_units {
		let alliance = if owned_tags.remove(u) {
			under_construction.remove(u);
			Some(Alliance::Own)
		} else if hallucinations.remove(u) {
			Some(Alliance::Enemy)
		} else if expansions.iter_mut().any(|exp| exp.minerals.remove(u)) {
			Some(Alliance::Neutral)
		} else {
			None
		};
		events.push(Event::UnitDestroyed(*u, alliance));
	}

	let effects = raw
		.effects
		.iter()
		.map(|e| Effect {
			id: {
				let id = e.effect_id();
				EffectId::from_u32(id).unwrap_or_else(|| panic!("There's no `EffectId` with value {}", id))
			},
			positions: e.pos.iter().map(Point2::from_proto).collect(),
			alliance: Alliance::from_proto(e.alliance()),
			owner: e.owner() as u32,
			radius: e.radius(),
		})
		.collect::<Vec<_>>();

	// Units. They're plain values here, so anything derived from
	// other units is simply written before the world is put together.
	let visibility = VisibilityMap::from_proto(map_state.visibility.get_or_default());
	let mut units = raw
		.units
		.iter()
		.map(|u| UnitData::from_proto(&visibility, u))
		.collect::<Vec<_>>();

	for u in &mut units {
		if u.alliance.is_enemy() {
			// Once flagged, a unit stays a hallucination, as in the old API
			if u.is_hallucination {
				hallucinations.insert(u.tag);
			} else if hallucinations.contains(&u.tag) {
				u.is_hallucination = true;
			}
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
		let is_structure = has_attribute(game_data, u.type_id, Attribute::Structure);

		match u.alliance {
			Alliance::Neutral => match u.type_id {
				UnitTypeId::XelNagaTower => groups.watchtowers.push(id),
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
				| UnitTypeId::MineralFieldOpaque900 => {
					groups.resources.push(id);
					groups.mineral_fields.push(id);
				}
				UnitTypeId::VespeneGeyser
				| UnitTypeId::SpacePlatformGeyser
				| UnitTypeId::RichVespeneGeyser
				| UnitTypeId::ProtossVespeneGeyser
				| UnitTypeId::PurifierVespeneGeyser
				| UnitTypeId::ShakurasVespeneGeyser => {
					groups.resources.push(id);
					groups.vespene_geysers.push(id);
				}
				type_id if INHIBITOR_IDS.contains(&type_id) => groups.inhibitor_zones.push(id),
				_ => groups.destructables.push(id),
			},
			Alliance::Own | Alliance::Enemy => {
				let is_mine = u.alliance.is_mine();
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
						| UnitTypeId::Nexus => {
							bases.insert(u.position, (u.alliance, Some(u.tag)));
							group.townhalls.push(id);
						}
						UnitTypeId::CommandCenterFlying | UnitTypeId::OrbitalCommandFlying => {
							group.townhalls.push(id)
						}
						UnitTypeId::Refinery
						| UnitTypeId::RefineryRich
						| UnitTypeId::Assimilator
						| UnitTypeId::AssimilatorRich
						| UnitTypeId::Extractor
						| UnitTypeId::ExtractorRich => group.gas_buildings.push(id),
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
			_ => {}
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
		if has_attribute(game_data, u.type_id, Attribute::Structure) {
			let counter = if u.is_ready() {
				&mut enemies_current
			} else {
				&mut enemies_ordered
			};
			*counter.entry(u.type_id).or_default() += 1;
		} else if !u.is_hallucination {
			*enemies_current.entry(u.type_id).or_default() += 1;
		}
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
		enemy_race,
		upgrades: raw
			.player
			.upgrade_ids
			.iter()
			.map(|u| {
				UpgradeId::from_u32(*u).unwrap_or_else(|| panic!("There's no `UpgradeId` with value {}", u))
			})
			.collect(),
		enemy_upgrades: prev.enemy_upgrades.clone(),
		creep: PixelMap::from_proto(&map_state.creep.get_or_default()),
		visibility,
		effects,
		dead_units,
		expansions,
		orders,
		current_units,
		enemies_ordered,
		enemies_current,
		last_health: prev
			.units
			.iter()
			.filter_map(|u| Some((u.tag, u.hits()?)))
			.collect(),
		units,
		index,
		groups,
		abilities,
		techlab_tags,
		reactor_tags,
		max_cooldowns,
		hallucinations,
		owned_tags,
		under_construction,
		map,
	};
	(world, events)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
