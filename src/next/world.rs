use super::{units::PlayerGroups, Expansion, MapData, PlayerUnits, UnitData, UnitRef, UnitTag, Units};
use crate::{
	action::{Action, ActionError},
	bot::{Completion, UnitAlias},
	consts::{TECH_ALIAS, UNIT_ALIAS},
	distance::Distance,
	game_data::Cost,
	game_state::{Alert, AvailableAbility, ChatMessage, Common, Effect, PsionicMatrix, Radar},
	geometry::Point2,
	ids::{AbilityId, UnitTypeId, UpgradeId},
	pixel_map::{PixelMap, VisibilityMap},
	player::Race,
	score::Score,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

#[derive(Default, Clone)]
pub(super) struct Groups {
	pub all: Vec<u32>,
	pub my: PlayerGroups,
	pub enemy: PlayerGroups,
	pub memory: PlayerGroups,
	pub mineral_fields: Vec<u32>,
	pub vespene_geysers: Vec<u32>,
	pub resources: Vec<u32>,
	pub destructables: Vec<u32>,
	pub watchtowers: Vec<u32>,
	pub inhibitor_zones: Vec<u32>,
}

/// The game state of one step.
///
/// A new one is built from the previous one and the new observation.
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
	/// Everything SC2 reported about your economy and army.
	pub common: Common,
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
	/// Terran radars on the map.
	pub radars: Vec<Radar>,
	/// Protoss power from pylons and warp prisms.
	pub psionic_matrix: Vec<PsionicMatrix>,
	/// Current camera position.
	pub camera: Point2,
	/// Alerts raised since the previous step.
	pub alerts: Vec<Alert>,
	/// Abilities available through the UI.
	pub available_abilities: Vec<AvailableAbility>,
	/// SC2 score data.
	pub score: Score,
	/// Messages in game chat since the previous step.
	pub chat: Vec<ChatMessage>,
	/// Actions executed on the previous step.
	pub actions: Vec<Action>,
	/// Errors of actions from the previous step.
	pub action_errors: Vec<ActionError>,
	/// Tags of units which died since the previous step.
	pub dead_units: Vec<UnitTag>,
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
	/// In-progress enemy units in [`enemy_memory`](Self::enemy_memory), counted by type.
	pub remembered_ordered: FxHashMap<UnitTypeId, usize>,
	/// Ready enemy units in [`enemy_memory`](Self::enemy_memory), counted by type.
	pub remembered_current: FxHashMap<UnitTypeId, usize>,

	// This step's units come first, followed by what only enemy memory holds.
	pub(super) units: Vec<UnitData>,
	pub(super) current_units_count: usize,
	pub(super) index: FxHashMap<UnitTag, u32>,
	pub(super) groups: Groups,
	pub(super) abilities: FxHashMap<UnitTag, FxHashSet<AbilityId>>,
	pub(super) techlab_tags: FxHashSet<UnitTag>,
	pub(super) reactor_tags: FxHashSet<UnitTag>,
	pub(super) last_health: FxHashMap<UnitTag, u32>,
	pub(super) max_cooldowns: FxHashMap<UnitTypeId, f32>,
	pub(super) available_frames: FxHashMap<UnitTag, u32>,
	// Remembered from step to step
	pub(super) hallucinations: FxHashSet<UnitTag>,
	pub(super) owned_tags: FxHashSet<UnitTag>,
	pub(super) under_construction: FxHashSet<UnitTag>,
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
			common: Default::default(),
			enemy_race,
			upgrades: Default::default(),
			enemy_upgrades: Default::default(),
			creep: Default::default(),
			visibility: Default::default(),
			effects: Default::default(),
			radars: Default::default(),
			psionic_matrix: Default::default(),
			camera: Default::default(),
			alerts: Default::default(),
			available_abilities: Default::default(),
			score: Default::default(),
			chat: Default::default(),
			actions: Default::default(),
			action_errors: Default::default(),
			dead_units: Default::default(),
			expansions,
			orders: Default::default(),
			current_units: Default::default(),
			enemies_ordered: Default::default(),
			enemies_current: Default::default(),
			remembered_ordered: Default::default(),
			remembered_current: Default::default(),
			units: Default::default(),
			current_units_count: 0,
			index: Default::default(),
			groups: Default::default(),
			abilities: Default::default(),
			techlab_tags: Default::default(),
			reactor_tags: Default::default(),
			last_health: Default::default(),
			max_cooldowns: Default::default(),
			available_frames: Default::default(),
			hallucinations: Default::default(),
			owned_tags: Default::default(),
			under_construction: Default::default(),
		}
	}

	#[inline]
	pub(super) fn unit_at(&self, id: u32) -> UnitRef<'_> {
		UnitRef::new(&self.units[id as usize], self, id)
	}
	#[inline]
	pub(super) fn index_of(&self, tag: UnitTag) -> Option<u32> {
		self.index.get(&tag).copied()
	}
	/// Finds any unit by tag. `None` means it's not in this step's observation.
	pub fn unit(&self, tag: UnitTag) -> Option<UnitRef<'_>> {
		self.index_of(tag).map(|id| self.unit_at(id))
	}
	/// A group made of given units, for example the result of your own iterator chain.
	pub fn units_from<'w>(&'w self, units: impl IntoIterator<Item = UnitRef<'w>>) -> Units<'w> {
		let ids = units.into_iter().map(UnitRef::id).collect::<Vec<_>>();
		Units::unordered(self, ids.into())
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
			searchable: true,
		}
	}
	/// Opponent's units.
	pub fn enemy(&self) -> PlayerUnits<'_> {
		PlayerUnits {
			world: self,
			groups: &self.groups.enemy,
			searchable: true,
		}
	}
	/// Opponent's units, including ones seen on earlier steps that are probably still around:
	/// out of vision, or thought to be burrowed. Their data is from when they were last seen.
	///
	/// Replaces the old `enemies_cache` feature, and is always available. Unlike that feature,
	/// [`enemy`](Self::enemy) always reports exactly what SC2 said on this step.
	pub fn enemy_memory(&self) -> PlayerUnits<'_> {
		PlayerUnits {
			world: self,
			groups: &self.groups.memory,
			searchable: false,
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
	/// Terrain height on given position.
	pub fn get_height<P: Into<(usize, usize)>>(&self, pos: P) -> u8 {
		self.map
			.game_info
			.terrain_height
			.get(pos.into())
			.copied()
			.unwrap_or(0)
	}
	/// Checks if it's possible to build on given position.
	pub fn is_placeable<P: Into<(usize, usize)>>(&self, pos: P) -> bool {
		self.map
			.game_info
			.placement_grid
			.get(pos.into())
			.map_or(false, |p| p.is_empty())
	}
	/// Checks if ground units can walk through given position.
	pub fn is_pathable<P: Into<(usize, usize)>>(&self, pos: P) -> bool {
		self.map
			.game_info
			.pathing_grid
			.get(pos.into())
			.map_or(false, |p| p.is_empty())
	}
	/// Checks if given position hasn't been explored yet.
	pub fn is_hidden<P: Into<(usize, usize)>>(&self, pos: P) -> bool {
		self.visibility.get(pos.into()).map_or(true, |p| p.is_hidden())
	}
	/// Checks if given position is in fog of war.
	pub fn is_fogged<P: Into<(usize, usize)>>(&self, pos: P) -> bool {
		self.visibility.get(pos.into()).map_or(true, |p| p.is_fogged())
	}
	/// Checks if given position is visible now.
	pub fn is_visible<P: Into<(usize, usize)>>(&self, pos: P) -> bool {
		self.visibility.get(pos.into()).map_or(false, |p| p.is_visible())
	}
	/// Checks if given position is fully hidden (only in campaign and custom maps).
	pub fn is_full_hidden<P: Into<(usize, usize)>>(&self, pos: P) -> bool {
		self.visibility
			.get(pos.into())
			.map_or(true, |p| p.is_full_hidden())
	}
	/// Checks if given position was explored before.
	pub fn is_explored<P: Into<(usize, usize)>>(&self, pos: P) -> bool {
		self.visibility.get(pos.into()).map_or(false, |p| p.is_explored())
	}
	/// Checks if given position has creep. In SC2's creep map a set bit means creep, and
	/// the map parser turns set bits into `Pixel::Empty`, so `Empty` is what means creep here.
	pub fn has_creep<P: Into<(usize, usize)>>(&self, pos: P) -> bool {
		self.creep.get(pos.into()).map_or(false, |p| p.is_empty())
	}

	/// Full cost of given unit type, without any corrections.
	pub fn get_unit_api_cost(&self, unit: UnitTypeId) -> Cost {
		self.map
			.game_data
			.units
			.get(&unit)
			.map_or_else(Cost::default, |d| d.cost())
	}
	/// Correct cost of building given unit type.
	pub fn get_unit_cost(&self, unit: UnitTypeId) -> Cost {
		let mut cost = self.get_unit_api_cost(unit);
		match unit {
			UnitTypeId::OverlordTransport => {
				cost.minerals = 25;
				cost.vespene = 25;
			}
			UnitTypeId::Zergling | UnitTypeId::ZerglingBurrowed => {
				cost.minerals *= 2;
				cost.supply *= 2.0;
			}
			_ => {
				let pred = self.get_unit_api_cost(match unit {
					UnitTypeId::Baneling | UnitTypeId::BanelingBurrowed => UnitTypeId::Zergling,
					UnitTypeId::Ravager | UnitTypeId::RavagerBurrowed => UnitTypeId::Roach,
					UnitTypeId::LurkerMP | UnitTypeId::LurkerMPBurrowed => UnitTypeId::Hydralisk,
					UnitTypeId::Overseer | UnitTypeId::OverseerSiegeMode => UnitTypeId::Overlord,
					UnitTypeId::BroodLord => UnitTypeId::Corruptor,
					UnitTypeId::OrbitalCommand
					| UnitTypeId::OrbitalCommandFlying
					| UnitTypeId::PlanetaryFortress => UnitTypeId::CommandCenter,
					UnitTypeId::Lair => UnitTypeId::Hatchery,
					UnitTypeId::Hive => UnitTypeId::Lair,
					UnitTypeId::GreaterSpire => UnitTypeId::Spire,
					UnitTypeId::Hatchery
					| UnitTypeId::SpineCrawler
					| UnitTypeId::SporeCrawler
					| UnitTypeId::Extractor
					| UnitTypeId::SpawningPool
					| UnitTypeId::EvolutionChamber
					| UnitTypeId::RoachWarren
					| UnitTypeId::BanelingNest
					| UnitTypeId::HydraliskDen
					| UnitTypeId::LurkerDenMP
					| UnitTypeId::InfestationPit
					| UnitTypeId::Spire
					| UnitTypeId::NydusNetwork
					| UnitTypeId::UltraliskCavern => UnitTypeId::Drone,
					_ => return cost,
				});
				cost.minerals -= pred.minerals;
				cost.vespene -= pred.vespene;
				cost.supply = (cost.supply - pred.supply).max(0.0);
			}
		}
		cost
	}
	/// Cost of given upgrade.
	pub fn get_upgrade_cost(&self, upgrade: UpgradeId) -> Cost {
		self.map
			.game_data
			.upgrades
			.get(&upgrade)
			.map_or_else(Default::default, |d| d.cost())
	}
	/// Checks if given upgrade is complete.
	pub fn has_upgrade(&self, upgrade: UpgradeId) -> bool {
		self.upgrades.contains(&upgrade)
	}
	/// Checks if an enemy upgrade was noted with [`Commands`](super::Commands).
	pub fn enemy_has_upgrade(&self, upgrade: UpgradeId) -> bool {
		self.enemy_upgrades.contains(&upgrade)
	}
	/// Checks if given upgrade is in progress.
	pub fn is_ordered_upgrade(&self, upgrade: UpgradeId) -> bool {
		let ability = self.map.game_data.upgrades.get(&upgrade).map(|d| d.ability);
		ability.map_or(false, |a| self.orders.get(&a).map_or(false, |count| *count > 0))
	}
	/// Progress of given upgrade: `1` when complete, `0` when not ordered.
	pub fn upgrade_progress(&self, upgrade: UpgradeId) -> f32 {
		if self.has_upgrade(upgrade) {
			return 1.0;
		}
		if !self.is_ordered_upgrade(upgrade) {
			return 0.0;
		}
		let ability = self.map.game_data.upgrades[&upgrade].ability;
		self.my()
			.structures()
			.iter()
			.filter(|s| s.is_ready())
			.find_map(|s| s.orders.iter().find(|o| o.ability == ability).map(|o| o.progress))
			.unwrap_or(0.0)
	}
	/// Mineral fields of given expansion on this step, closest to the townhall first.
	pub fn minerals_of(&self, expansion: &Expansion) -> Units<'_> {
		self.mineral_fields()
			.filter(|m| expansion.minerals.contains(&m.position))
			.sorted(|m| m.position.distance_squared(expansion.loc))
	}
	/// Vespene geysers of given expansion on this step.
	pub fn geysers_of(&self, expansion: &Expansion) -> Units<'_> {
		self.vespene_geysers()
			.filter(|g| expansion.geysers.contains(&g.position))
	}
	/// Next free expansion closest to your start location.
	pub fn get_expansion(&self) -> Option<&Expansion> {
		self.free_expansions().next()
	}
	/// Expansions taken by you.
	pub fn owned_expansions(&self) -> impl Iterator<Item = &Expansion> {
		self.expansions.iter().filter(|exp| exp.alliance.is_mine())
	}
	/// Expansions taken by the opponent.
	pub fn enemy_expansions(&self) -> impl Iterator<Item = &Expansion> {
		self.expansions.iter().filter(|exp| exp.alliance.is_enemy())
	}
	/// Free expansions.
	pub fn free_expansions(&self) -> impl Iterator<Item = &Expansion> {
		self.expansions.iter().filter(|exp| exp.alliance.is_neutral())
	}
	/// Counts your units. Ready ones of exactly the given type by default.
	pub fn counter(&self) -> CountOptions<'_> {
		CountOptions::new(self, CountSource::Mine)
	}
	/// Counts enemy units seen on this step.
	pub fn enemy_counter(&self) -> CountOptions<'_> {
		CountOptions::new(self, CountSource::Enemy)
	}
	/// Counts enemy units in [`enemy_memory`](Self::enemy_memory).
	pub fn remembered_counter(&self) -> CountOptions<'_> {
		CountOptions::new(self, CountSource::Remembered)
	}
}

#[derive(Clone, Copy)]
enum CountSource {
	Mine,
	Enemy,
	Remembered,
}

/// Which units to count. Made with [`World::counter`] and its siblings.
#[derive(Clone, Copy)]
pub struct CountOptions<'w> {
	world: &'w World,
	source: CountSource,
	completion: Completion,
	alias: UnitAlias,
}

impl<'w> CountOptions<'w> {
	fn new(world: &'w World, source: CountSource) -> Self {
		Self {
			world,
			source,
			completion: Completion::Complete,
			alias: UnitAlias::None,
		}
	}
	/// Count only units in progress.
	pub fn ordered(mut self) -> Self {
		self.completion = Completion::Ordered;
		self
	}
	/// Count both complete units and units in progress.
	pub fn all(mut self) -> Self {
		self.completion = Completion::All;
		self
	}
	/// Also count the other form of units that have 2 forms.
	pub fn alias(mut self) -> Self {
		self.alias = UnitAlias::Unit;
		self
	}
	/// Also count other forms of units that have more than 2, usually structures.
	pub fn tech(mut self) -> Self {
		self.alias = UnitAlias::Tech;
		self
	}
	fn ready(self, id: UnitTypeId) -> usize {
		let counts = match self.source {
			CountSource::Mine => &self.world.current_units,
			CountSource::Enemy => &self.world.enemies_current,
			CountSource::Remembered => &self.world.remembered_current,
		};
		counts.get(&id).copied().unwrap_or(0)
	}
	fn in_progress(self, id: UnitTypeId) -> usize {
		let world = self.world;
		match self.source {
			CountSource::Mine => world
				.map
				.game_data
				.units
				.get(&id)
				.and_then(|d| d.ability)
				.and_then(|ability| world.orders.get(&ability).copied())
				.unwrap_or(0),
			CountSource::Enemy => world.enemies_ordered.get(&id).copied().unwrap_or(0),
			CountSource::Remembered => world.remembered_ordered.get(&id).copied().unwrap_or(0),
		}
	}
	fn count_one(self, id: UnitTypeId) -> usize {
		match self.completion {
			Completion::Complete => self.ready(id),
			Completion::Ordered => self.in_progress(id),
			Completion::All => self.ready(id) + self.in_progress(id),
		}
	}
	/// Counts units of given type.
	pub fn count(self, id: UnitTypeId) -> usize {
		self.count_one(id)
			+ match self.alias {
				UnitAlias::None => 0,
				UnitAlias::Unit => UNIT_ALIAS.get(&id).map_or(0, |alias| self.count_one(*alias)),
				UnitAlias::Tech => TECH_ALIAS
					.get(&id)
					.map_or(0, |alias| alias.iter().map(|a| self.count_one(*a)).sum()),
			}
	}
}
