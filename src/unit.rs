use crate::World;
use crate::{
	action::Target,
	consts::{FRAMES_PER_SECOND, OFF_CREEP_SPEED_UPGRADES, SPEED_BUFFS, SPEED_ON_CREEP, SPEED_UPGRADES},
	game_data::{Attribute, Cost, UnitTypeData},
	game_state::Alliance,
	geometry::{Point2, Point3},
	ids::{AbilityId, BuffId, UnitTypeId, UpgradeId},
	pixel_map::VisibilityMap,
	player::Race,
	units::Container,
	FromProto,
};
use num_traits::FromPrimitive;
use once_cell::sync::Lazy;
use rustc_hash::FxHashSet;
use sc2_proto::raw::{
	unit_order::Target as ProtoTarget, CloakState as ProtoCloakState, DisplayType as ProtoDisplayType,
	Unit as ProtoUnit,
};
use std::{ops::Deref, sync::Mutex};

/// Identifies a unit. Only comes from SC2, so it can't be confused with another number.
///
/// Keep these to remember a unit between steps. SC2 gives units in the fog of war other tags
/// than visible ones, so the tag of a mineral field or geyser changes as it goes in and out
/// of vision. Use positions for those, which is what [`Expansion`](crate::Expansion) does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UnitTag(pub(crate) u64);

impl UnitTag {
	/// The value SC2 uses, for sending your own [`Request`](crate::Request)s.
	pub fn get(self) -> u64 {
		self.0
	}
	/// Makes a tag from a value SC2 sent, for reading responses to your own [`Request`](crate::Request)s.
	pub fn from_raw(tag: u64) -> Self {
		Self(tag)
	}
}
impl From<UnitRef<'_>> for UnitTag {
	#[inline]
	fn from(u: UnitRef<'_>) -> Self {
		u.data.tag
	}
}
impl From<UnitTag> for Target {
	#[inline]
	fn from(tag: UnitTag) -> Self {
		Target::Tag(tag)
	}
}
impl From<UnitRef<'_>> for Target {
	#[inline]
	fn from(u: UnitRef<'_>) -> Self {
		Target::Tag(u.data.tag)
	}
}

/// Returns `parsed`, warning once per id when SC2 sent something this library doesn't know.
///
/// IDs are generated from one game version, so a newer client can report ones missing from
/// the tables. That shouldn't end a game, so unknown ones are skipped. Run `generate_ids.py`
/// against your client to bring the tables up to date.
pub(super) fn known<T>(kind: &'static str, id: i64, parsed: Option<T>) -> Option<T> {
	static WARNED: Lazy<Mutex<FxHashSet<(&'static str, i64)>>> = Lazy::new(Default::default);
	if parsed.is_none() {
		let first_time = WARNED
			.lock()
			.map_or(false, |mut warned| warned.insert((kind, id)));
		if first_time {
			warn!("SC2 reported an unknown {} with id {}, ignoring it", kind, id);
		}
	}
	parsed
}

/// What SC2 reported about a unit on one step: plain data with public fields.
///
/// `Clone` it if you want to keep a unit's state between steps. It's a value,
/// so nothing about it can change or go half-stale behind your back.
#[derive(Clone)]
#[allow(missing_docs)]
pub struct UnitData {
	pub tag: UnitTag,
	pub type_id: UnitTypeId,
	pub alliance: Alliance,
	pub display_type: DisplayType,
	pub owner: u32,
	pub position: Point2,
	pub position3d: Point3,
	pub facing: f32,
	pub radius: f32,
	pub build_progress: f32,
	pub is_cloaked: bool,
	pub is_revealed: bool,
	pub buffs: FxHashSet<BuffId>,
	pub detect_range: f32,
	pub radar_range: f32,
	pub is_selected: bool,
	pub is_on_screen: bool,
	pub is_blip: bool,
	pub is_powered: bool,
	pub is_active: bool,
	pub attack_upgrade_level: u32,
	pub armor_upgrade_level: i32,
	pub shield_upgrade_level: i32,
	// Not populated for snapshots
	pub health: Option<u32>,
	pub health_max: Option<u32>,
	pub shield: Option<u32>,
	pub shield_max: Option<u32>,
	pub energy: Option<u32>,
	pub energy_max: Option<u32>,
	pub mineral_contents: Option<u32>,
	pub vespene_contents: Option<u32>,
	pub is_flying: bool,
	pub is_burrowed: bool,
	pub is_hallucination: bool,
	// Not populated for enemies
	pub orders: Vec<UnitOrder>,
	pub addon_tag: Option<UnitTag>,
	pub passengers: Vec<PassengerUnit>,
	pub cargo_space_taken: Option<u32>,
	pub cargo_space_max: Option<u32>,
	pub assigned_harvesters: Option<u32>,
	pub ideal_harvesters: Option<u32>,
	pub weapon_cooldown: Option<f32>,
	pub engaged_target_tag: Option<UnitTag>,
	pub buff_duration_remain: Option<u32>,
	pub buff_duration_max: Option<u32>,
	pub rally_targets: Vec<RallyTarget>,
}

impl UnitData {
	/// `None` if SC2 reports a unit type this version of the library doesn't know.
	pub(super) fn from_proto(visibility: &VisibilityMap, u: &ProtoUnit) -> Option<Self> {
		let pos = u.pos.get_or_default();
		let position = Point2::from_proto(pos);
		let type_id = known(
			"unit type",
			u.unit_type() as i64,
			UnitTypeId::from_u32(u.unit_type()),
		)?;
		let is_burrowed = u.is_burrowed();
		let (is_cloaked, is_revealed) = if is_burrowed {
			(true, false)
		} else {
			match u.cloak() {
				ProtoCloakState::CloakedUnknown | ProtoCloakState::NotCloaked => (false, false),
				ProtoCloakState::Cloaked | ProtoCloakState::CloakedAllied => (true, false),
				ProtoCloakState::CloakedDetected => (true, true),
			}
		};
		Some(Self {
			tag: UnitTag(u.tag()),
			type_id,
			alliance: Alliance::from_proto(u.alliance()),
			display_type: match DisplayType::from_proto(u.display_type()) {
				DisplayType::Visible => {
					let visible = visibility
						.get(<(usize, usize)>::from(position))
						.map_or(false, |p| p.is_visible());
					if visible {
						DisplayType::Visible
					} else {
						DisplayType::Snapshot
					}
				}
				other => other,
			},
			owner: u.owner() as u32,
			position,
			position3d: Point3::from_proto(&pos),
			facing: u.facing(),
			radius: u.radius(),
			build_progress: u.build_progress(),
			is_cloaked,
			is_revealed,
			buffs: u
				.buff_ids
				.iter()
				.filter_map(|b| known("buff", *b as i64, BuffId::from_u32(*b)))
				.collect(),
			detect_range: match type_id {
				UnitTypeId::Observer => 11.0,
				UnitTypeId::ObserverSiegeMode => 13.75,
				_ => u.detect_range(),
			},
			radar_range: u.radar_range(),
			is_selected: u.is_selected(),
			is_on_screen: u.is_on_screen(),
			is_blip: u.is_blip(),
			is_powered: u.is_powered(),
			is_active: u.is_active(),
			attack_upgrade_level: u.attack_upgrade_level() as u32,
			armor_upgrade_level: u.armor_upgrade_level(),
			shield_upgrade_level: u.shield_upgrade_level(),
			health: u.health.map(|x| x as u32),
			health_max: u.health_max.map(|x| x as u32),
			shield: u.shield.map(|x| x as u32),
			shield_max: u.shield_max.map(|x| x as u32),
			energy: u.energy.map(|x| x as u32),
			energy_max: u.energy_max.map(|x| x as u32),
			mineral_contents: u.mineral_contents.map(|x| x as u32),
			vespene_contents: u.vespene_contents.map(|x| x as u32),
			is_flying: u.is_flying(),
			is_burrowed,
			is_hallucination: u.is_hallucination(),
			orders: u
				.orders
				.iter()
				.map(|order| UnitOrder {
					// Dropping an unknown order would make a busy unit look idle
					ability: known(
						"ability",
						order.ability_id() as i64,
						AbilityId::from_u32(order.ability_id()),
					)
					.unwrap_or(AbilityId::NullNull),
					target: match &order.target {
						Some(ProtoTarget::TargetWorldSpacePos(pos)) => Target::Pos(Point2::from_proto(pos)),
						Some(ProtoTarget::TargetUnitTag(tag)) => Target::Tag(UnitTag(*tag)),
						_ => Target::None,
					},
					progress: order.progress(),
				})
				.collect(),
			addon_tag: u.add_on_tag.map(UnitTag),
			passengers: u
				.passengers
				.iter()
				.filter_map(|p| {
					Some(PassengerUnit {
						tag: UnitTag(p.tag()),
						health: p.health(),
						health_max: p.health_max(),
						shield: p.shield(),
						shield_max: p.shield_max(),
						energy: p.energy(),
						energy_max: p.energy_max(),
						type_id: known(
							"unit type",
							p.unit_type() as i64,
							UnitTypeId::from_u32(p.unit_type()),
						)?,
					})
				})
				.collect(),
			cargo_space_taken: u.cargo_space_taken.map(|x| x as u32),
			cargo_space_max: u.cargo_space_max.map(|x| x as u32),
			assigned_harvesters: u.assigned_harvesters.map(|x| x as u32),
			ideal_harvesters: u.ideal_harvesters.map(|x| x as u32),
			weapon_cooldown: u.weapon_cooldown,
			engaged_target_tag: u.engaged_target_tag.map(UnitTag),
			buff_duration_remain: u.buff_duration_remain.map(|x| x as u32),
			buff_duration_max: u.buff_duration_max.map(|x| x as u32),
			rally_targets: u
				.rally_targets
				.iter()
				.map(|t| RallyTarget {
					point: Point2::from_proto(t.point.get_or_default()),
					tag: t.tag.map(UnitTag),
				})
				.collect(),
		})
	}

	/// Summed health and shield. Not populated for snapshots.
	pub fn hits(&self) -> Option<u32> {
		sum_options(self.health, self.shield)
	}
	/// Summed maximum health and shield. Not populated for snapshots.
	pub fn hits_max(&self) -> Option<u32> {
		sum_options(self.health_max, self.shield_max)
	}
	/// Current health divided by maximum health.
	pub fn health_percentage(&self) -> Option<f32> {
		percentage(self.health, self.health_max)
	}
	/// Current shield divided by maximum shield.
	pub fn shield_percentage(&self) -> Option<f32> {
		percentage(self.shield, self.shield_max)
	}
	/// Current energy divided by maximum energy.
	pub fn energy_percentage(&self) -> Option<f32> {
		percentage(self.energy, self.energy_max)
	}
	/// Current hits divided by maximum hits.
	pub fn hits_percentage(&self) -> Option<f32> {
		percentage(self.hits(), self.hits_max())
	}
	/// Checks if unit is yours.
	pub fn is_mine(&self) -> bool {
		self.alliance.is_mine()
	}
	/// Checks if unit is an enemy's.
	pub fn is_enemy(&self) -> bool {
		self.alliance.is_enemy()
	}
	/// Checks if unit is neutral.
	pub fn is_neutral(&self) -> bool {
		self.alliance.is_neutral()
	}
	/// Checks if unit is an ally's.
	pub fn is_ally(&self) -> bool {
		self.alliance.is_ally()
	}
	/// Checks if unit is a worker.
	pub fn is_worker(&self) -> bool {
		self.type_id.is_worker()
	}
	/// Checks if unit is a townhall.
	pub fn is_townhall(&self) -> bool {
		self.type_id.is_townhall()
	}
	/// Checks if unit is an addon.
	pub fn is_addon(&self) -> bool {
		self.type_id.is_addon()
	}
	/// Checks if unit is a melee attacker.
	pub fn is_melee(&self) -> bool {
		self.type_id.is_melee()
	}
	/// Building construction is complete.
	pub fn is_ready(&self) -> bool {
		(self.build_progress - 1.0).abs() < f32::EPSILON
	}
	/// Building construction is more than 95% complete.
	pub fn is_almost_ready(&self) -> bool {
		self.build_progress >= 0.95
	}
	/// Terran building has an addon.
	pub fn has_addon(&self) -> bool {
		self.addon_tag.is_some()
	}
	/// Some units are inside this transport or bunker.
	pub fn has_cargo(&self) -> bool {
		self.cargo_space_taken.map_or(false, |taken| taken > 0)
	}
	/// Free space in this transport or bunker.
	pub fn cargo_left(&self) -> Option<u32> {
		Some(self.cargo_space_max? - self.cargo_space_taken?)
	}
	/// Point at given distance in the direction unit is facing.
	pub fn towards_facing(&self, offset: f32) -> Point2 {
		self.position
			.offset(offset * self.facing.cos(), offset * self.facing.sin())
	}
	/// Unit is fully visible.
	pub fn is_visible(&self) -> bool {
		self.display_type.is_visible()
	}
	/// Unit is a snapshot (hidden in fog of war or on high ground).
	pub fn is_snapshot(&self) -> bool {
		self.display_type.is_snapshot()
	}
	/// Unit is fully hidden.
	pub fn is_hidden(&self) -> bool {
		self.display_type.is_hidden()
	}
	/// Unit is a building placeholder.
	pub fn is_placeholder(&self) -> bool {
		self.display_type.is_placeholder()
	}
	/// Unit is detected or not even cloaked.
	pub fn can_be_attacked(&self) -> bool {
		self.is_revealed || !self.is_cloaked
	}
	/// Unit is cloaked or burrowed and not detected, so it must be detected to be attacked.
	pub fn is_invisible(&self) -> bool {
		self.is_cloaked && !self.is_revealed
	}
	/// Checks if unit has given buff.
	pub fn has_buff(&self, buff: BuffId) -> bool {
		self.buffs.contains(&buff)
	}
	/// Checks if unit has any of given buffs.
	pub fn has_any_buff<'a, B: IntoIterator<Item = &'a BuffId>>(&self, buffs: B) -> bool {
		buffs.into_iter().any(|b| self.buffs.contains(b))
	}
	/// Worker is carrying minerals.
	pub fn is_carrying_minerals(&self) -> bool {
		self.has_any_buff(&[
			BuffId::CarryMineralFieldMinerals,
			BuffId::CarryHighYieldMineralFieldMinerals,
		])
	}
	/// Worker is carrying gas. SC2 doesn't report gas from rich geysers.
	pub fn is_carrying_vespene(&self) -> bool {
		self.has_any_buff(&[
			BuffId::CarryHarvestableVespeneGeyserGas,
			BuffId::CarryHarvestableVespeneGeyserGasProtoss,
			BuffId::CarryHarvestableVespeneGeyserGasZerg,
		])
	}
	/// Worker is carrying any resource.
	pub fn is_carrying_resource(&self) -> bool {
		self.is_carrying_minerals() || self.is_carrying_vespene()
	}
	/// Checks if unit's weapon is on cooldown.
	pub fn on_cooldown(&self) -> bool {
		self.weapon_cooldown.map_or(false, |cool| cool > f32::EPSILON)
	}
	/// Checks if unit is a detector.
	pub fn is_detector(&self) -> bool {
		matches!(
			self.type_id,
			UnitTypeId::Observer
				| UnitTypeId::ObserverSiegeMode
				| UnitTypeId::Raven
				| UnitTypeId::Overseer
				| UnitTypeId::OverseerSiegeMode
		) || (self.is_almost_ready()
			&& (matches!(self.type_id, UnitTypeId::MissileTurret | UnitTypeId::SporeCrawler)
				|| (self.type_id == UnitTypeId::PhotonCannon && self.is_powered)))
	}

	// Orders. These don't work with enemies, as SC2 doesn't report their orders.

	/// Ability, target and progress of the current order.
	pub fn order(&self) -> Option<(AbilityId, Target, f32)> {
		self.orders.first().map(|o| (o.ability, o.target, o.progress))
	}
	/// Target of the current order.
	pub fn target(&self) -> Target {
		self.orders.first().map_or(Target::None, |o| o.target)
	}
	/// Target position of the current order.
	pub fn tarpos(&self) -> Option<Point2> {
		match self.target() {
			Target::Pos(pos) => Some(pos),
			_ => None,
		}
	}
	/// Target tag of the current order.
	pub fn tartag(&self) -> Option<UnitTag> {
		match self.target() {
			Target::Tag(tag) => Some(tag),
			_ => None,
		}
	}
	/// Ability of the current order.
	pub fn ordered_ability(&self) -> Option<AbilityId> {
		self.orders.first().map(|o| o.ability)
	}
	/// Unit has no orders.
	pub fn is_idle(&self) -> bool {
		self.orders.is_empty()
	}
	/// Unit has no orders, or its only order is more than 95% complete.
	pub fn is_almost_idle(&self) -> bool {
		self.is_idle() || (self.orders.len() == 1 && self.orders[0].progress >= 0.95)
	}
	/// Unit is using given ability.
	pub fn is_using(&self, ability: AbilityId) -> bool {
		self.ordered_ability() == Some(ability)
	}
	/// Unit is using any of given abilities.
	pub fn is_using_any<A: Container<AbilityId>>(&self, abilities: &A) -> bool {
		self.ordered_ability().map_or(false, |a| abilities.contains(&a))
	}
	/// Unit is attacking.
	pub fn is_attacking(&self) -> bool {
		matches!(
			self.ordered_ability(),
			Some(
				AbilityId::Attack
					| AbilityId::AttackAttack
					| AbilityId::AttackAttackTowards
					| AbilityId::AttackAttackBarrage
					| AbilityId::ScanMove
			)
		)
	}
	/// Unit is moving.
	pub fn is_moving(&self) -> bool {
		self.is_using(AbilityId::MoveMove)
	}
	/// Unit is patrolling.
	pub fn is_patrolling(&self) -> bool {
		self.is_using(AbilityId::Patrol)
	}
	/// SCV or MULE is repairing.
	pub fn is_repairing(&self) -> bool {
		matches!(
			self.ordered_ability(),
			Some(AbilityId::EffectRepairSCV | AbilityId::EffectRepairMule)
		)
	}
	/// Worker is gathering a resource.
	pub fn is_gathering(&self) -> bool {
		matches!(
			self.ordered_ability(),
			Some(
				AbilityId::HarvestGatherSCV
					| AbilityId::HarvestGatherMule
					| AbilityId::HarvestGatherDrone
					| AbilityId::HarvestGatherProbe
			)
		)
	}
	/// Worker is returning a resource to the closest base.
	pub fn is_returning(&self) -> bool {
		matches!(
			self.ordered_ability(),
			Some(
				AbilityId::HarvestReturnSCV
					| AbilityId::HarvestReturnMule
					| AbilityId::HarvestReturnDrone
					| AbilityId::HarvestReturnProbe
			)
		)
	}
	/// Worker is gathering or returning resources.
	pub fn is_collecting(&self) -> bool {
		self.ordered_ability()
			.map_or(false, |ability| match self.type_id {
				UnitTypeId::SCV => {
					matches!(ability, AbilityId::HarvestGatherSCV | AbilityId::HarvestReturnSCV)
				}
				UnitTypeId::MULE => matches!(
					ability,
					AbilityId::HarvestGatherMule | AbilityId::HarvestReturnMule
				),
				UnitTypeId::Drone => matches!(
					ability,
					AbilityId::HarvestGatherDrone | AbilityId::HarvestReturnDrone
				),
				UnitTypeId::Probe => matches!(
					ability,
					AbilityId::HarvestGatherProbe | AbilityId::HarvestReturnProbe
				),
				_ => false,
			})
	}
	/// Worker is constructing a building.
	pub fn is_constructing(&self) -> bool {
		self.ordered_ability()
			.map_or(false, |ability| match self.type_id {
				UnitTypeId::SCV => ability.is_constructing_scv(),
				UnitTypeId::Drone => ability.is_constructing_drone(),
				UnitTypeId::Probe => ability.is_constructing_probe(),
				_ => false,
			})
	}
	/// Terran building is making an addon.
	pub fn is_making_addon(&self) -> bool {
		self.ordered_ability()
			.map_or(false, |ability| match self.type_id {
				UnitTypeId::Barracks => matches!(
					ability,
					AbilityId::BuildTechLabBarracks | AbilityId::BuildReactorBarracks
				),
				UnitTypeId::Factory => matches!(
					ability,
					AbilityId::BuildTechLabFactory | AbilityId::BuildReactorFactory
				),
				UnitTypeId::Starport => matches!(
					ability,
					AbilityId::BuildTechLabStarport | AbilityId::BuildReactorStarport
				),
				_ => false,
			})
	}
	/// Terran building is making a techlab.
	pub fn is_making_techlab(&self) -> bool {
		matches!(
			self.ordered_ability(),
			Some(
				AbilityId::BuildTechLabBarracks
					| AbilityId::BuildTechLabFactory
					| AbilityId::BuildTechLabStarport
			)
		)
	}
	/// Terran building is making a reactor.
	pub fn is_making_reactor(&self) -> bool {
		matches!(
			self.ordered_ability(),
			Some(
				AbilityId::BuildReactorBarracks
					| AbilityId::BuildReactorFactory
					| AbilityId::BuildReactorStarport
			)
		)
	}
}

fn sum_options(a: Option<u32>, b: Option<u32>) -> Option<u32> {
	match (a, b) {
		(Some(a), Some(b)) => Some(a + b),
		(a, None) => a,
		(None, b) => b,
	}
}

fn percentage(current: Option<u32>, max: Option<u32>) -> Option<f32> {
	let (current, max) = (current?, max?);
	if max == 0 {
		None
	} else {
		Some(current as f32 / max as f32)
	}
}

/// A unit on the current step, together with the [`World`] it belongs to.
///
/// It's two references and an index, so it's `Copy` and free to pass around. Plain data is
/// available directly through `Deref` (`u.tag`, `u.position`), while methods that
/// need game rules read them from the world. The lifetime ties it to its step.
#[derive(Clone, Copy)]
pub struct UnitRef<'w> {
	data: &'w UnitData,
	world: &'w World,
	id: u32,
}

impl Deref for UnitRef<'_> {
	type Target = UnitData;

	#[inline]
	fn deref(&self) -> &UnitData {
		self.data
	}
}
impl From<UnitRef<'_>> for Point2 {
	#[inline]
	fn from(u: UnitRef<'_>) -> Self {
		u.data.position
	}
}

impl Radius for UnitRef<'_> {
	fn radius(&self) -> f32 {
		self.data.radius
	}
}

impl<'w> UnitRef<'w> {
	pub(super) fn new(data: &'w UnitData, world: &'w World, id: u32) -> Self {
		Self { data, world, id }
	}
	pub(super) fn id(self) -> u32 {
		self.id
	}
	/// The world this unit belongs to.
	pub fn world(self) -> &'w World {
		self.world
	}
	/// The plain data, for example to clone and keep it.
	pub fn data(self) -> &'w UnitData {
		self.data
	}

	pub(super) fn type_data(self) -> Option<&'w UnitTypeData> {
		self.world.map.game_data.units.get(&self.data.type_id)
	}
	pub(super) fn upgrades(self) -> &'w FxHashSet<UpgradeId> {
		if self.data.is_mine() {
			&self.world.upgrades
		} else {
			&self.world.enemy_upgrades
		}
	}
	/// Name of the unit.
	pub fn name(self) -> &'w str {
		self.type_data().map_or("", |d| &d.name)
	}
	/// Race of the unit, dependent on its type.
	pub fn race(self) -> Race {
		self.type_data().map_or(Race::Random, |d| d.race)
	}
	/// Checks if it's a mineral field.
	pub fn is_mineral(self) -> bool {
		self.type_data().map_or(false, |d| d.has_minerals)
	}
	/// Checks if it's a vespene geyser.
	pub fn is_geyser(self) -> bool {
		self.type_data().map_or(false, |d| d.has_vespene)
	}
	/// Attributes of the unit, dependent on its type.
	pub fn attributes(self) -> &'w [Attribute] {
		self.type_data().map_or(&[], |d| d.attributes.as_slice())
	}
	/// Checks if unit has given attribute.
	pub fn has_attribute(self, attribute: Attribute) -> bool {
		self.attributes().contains(&attribute)
	}
	/// Checks if unit is a structure.
	pub fn is_structure(self) -> bool {
		self.has_attribute(Attribute::Structure)
	}
	/// Checks if unit has the `Light` attribute.
	pub fn is_light(self) -> bool {
		self.has_attribute(Attribute::Light)
	}
	/// Checks if unit has the `Armored` attribute.
	pub fn is_armored(self) -> bool {
		self.has_attribute(Attribute::Armored)
	}
	/// Checks if unit has the `Biological` attribute.
	pub fn is_biological(self) -> bool {
		self.has_attribute(Attribute::Biological)
	}
	/// Checks if unit has the `Mechanical` attribute.
	pub fn is_mechanical(self) -> bool {
		self.has_attribute(Attribute::Mechanical)
	}
	/// Checks if unit has the `Robotic` attribute.
	pub fn is_robotic(self) -> bool {
		self.has_attribute(Attribute::Robotic)
	}
	/// Checks if unit has the `Psionic` attribute.
	pub fn is_psionic(self) -> bool {
		self.has_attribute(Attribute::Psionic)
	}
	/// Checks if unit has the `Massive` attribute.
	pub fn is_massive(self) -> bool {
		self.has_attribute(Attribute::Massive)
	}
	/// Checks if unit has the `Hover` attribute.
	pub fn is_hover(self) -> bool {
		self.has_attribute(Attribute::Hover)
	}
	/// Checks if unit has the `Heroic` attribute.
	pub fn is_heroic(self) -> bool {
		self.has_attribute(Attribute::Heroic)
	}
	/// Checks if unit has the `Summoned` attribute.
	pub fn is_summoned(self) -> bool {
		self.has_attribute(Attribute::Summoned)
	}
	/// Half of the building's size, but `2.5` for addons.
	pub fn footprint_radius(self) -> Option<f32> {
		let ability = self.type_data()?.ability?;
		self.world.map.game_data.abilities.get(&ability)?.footprint_radius
	}
	/// Building size in tiles (e.g. `2` for supply and addons, `3` for barracks).
	pub fn building_size(self) -> Option<usize> {
		if self.data.is_addon() {
			Some(2)
		} else {
			self.footprint_radius().map(|radius| (radius * 2.0) as usize)
		}
	}
	/// How long the unit takes to build.
	pub fn build_time(self) -> f32 {
		self.type_data().map_or(0.0, |d| d.build_time)
	}
	/// Space the unit takes in transports and bunkers.
	pub fn cargo_size(self) -> u32 {
		self.type_data().map_or(0, |d| d.cargo_size)
	}
	/// How far the unit can see.
	pub fn sight_range(self) -> f32 {
		self.type_data().map_or(0.0, |d| d.sight_range)
	}
	/// Initial armor, without upgrades and buffs.
	pub fn armor(self) -> i32 {
		self.type_data().map_or(0, |d| d.armor)
	}
	/// How much supply this unit uses.
	pub fn supply_cost(self) -> f32 {
		self.type_data().map_or(0.0, |d| d.food_required)
	}
	/// Cost of the unit.
	pub fn cost(self) -> Cost {
		self.type_data().map_or_else(Cost::default, |d| d.cost())
	}
	/// Terran building's addon is a techlab.
	pub fn has_techlab(self) -> bool {
		self.data
			.addon_tag
			.map_or(false, |t| self.world.techlab_tags.contains(&t))
	}
	/// Terran building's addon is a reactor.
	pub fn has_reactor(self) -> bool {
		self.data
			.addon_tag
			.map_or(false, |t| self.world.reactor_tags.contains(&t))
	}
	/// Unit lost health or shield since the previous step.
	///
	/// A unit that merely lost its health reading, such as one that became a snapshot, doesn't count.
	pub fn is_attacked(self) -> bool {
		self.damage_taken() > 0
	}
	/// Health and shield lost since the previous step.
	pub fn damage_taken(self) -> u32 {
		match (self.data.hits(), self.world.last_health.get(&self.data.tag)) {
			(Some(hits), Some(last)) => last.saturating_sub(hits),
			_ => 0,
		}
	}
	/// Abilities available to the unit. Empty when it's on
	/// cooldown, out of energy or you can't afford them.
	pub fn abilities(self) -> Option<&'w FxHashSet<AbilityId>> {
		self.world.abilities.get(&self.data.tag)
	}
	/// Checks if ability is available for the unit.
	pub fn has_ability(self, ability: AbilityId) -> bool {
		self.abilities().map_or(false, |a| a.contains(&ability))
	}
	/// Production building with a reactor has a free slot, otherwise the unit is idle.
	pub fn is_unused(self) -> bool {
		if self.has_reactor() {
			self.data.orders.len() < 2
		} else {
			self.data.is_idle()
		}
	}
	/// As [`is_unused`](Self::is_unused), counting orders more than 95% complete as finished.
	pub fn is_almost_unused(self) -> bool {
		if self.has_reactor() {
			self.data.orders.len() < 2
				|| (self.data.orders.len() == 2 && self.data.orders.iter().any(|o| o.progress >= 0.95))
		} else {
			self.data.is_almost_idle()
		}
	}
	/// Unit was put to sleep with [`Commands::sleep`](crate::Commands::sleep).
	pub fn is_sleeping(self) -> bool {
		self.world
			.available_frames
			.get(&self.data.tag)
			.map_or(false, |frame| self.world.game_loop < *frame)
	}
	/// Movement speed from unit data, without buffs and upgrades.
	pub fn speed(self) -> f32 {
		self.type_data().map_or(0.0, |d| d.movement_speed)
	}
	/// Actual speed of the unit, including buffs, upgrades and creep.
	pub fn real_speed(self) -> f32 {
		let mut speed = self.speed();
		let unit_type = self.data.type_id;

		// Ultralisk has passive ability "Frenzied" which makes it immune to speed altering buffs
		if unit_type != UnitTypeId::Ultralisk {
			for buff in &self.data.buffs {
				match buff {
					BuffId::MedivacSpeedBoost => return speed * 1.7,
					BuffId::VoidRaySwarmDamageBoost => return speed * 0.75,
					_ => {
						if let Some(increase) = SPEED_BUFFS.get(buff) {
							speed *= increase;
						}
					}
				}
			}
		}

		let upgrades = self.upgrades();
		if let Some((upgrade_id, increase)) = SPEED_UPGRADES.get(&unit_type) {
			if upgrades.contains(upgrade_id) {
				speed *= increase;
			}
		}
		if self.world.has_creep(self.data.position) {
			if let Some(increase) = SPEED_ON_CREEP.get(&unit_type) {
				speed *= increase;
			}
		}
		if !upgrades.is_empty() {
			if let Some((upgrade_id, increase)) = OFF_CREEP_SPEED_UPGRADES.get(&unit_type) {
				if upgrades.contains(upgrade_id) {
					speed *= increase;
				}
			}
		}
		speed
	}
	/// Distance the unit can travel per step.
	pub fn distance_per_step(self) -> f32 {
		self.real_speed() / FRAMES_PER_SECOND * self.world.game_step as f32
	}
	/// Distance the unit can travel until its weapons are ready to fire.
	pub fn distance_to_weapon_ready(self) -> f32 {
		self.real_speed() / FRAMES_PER_SECOND * self.data.weapon_cooldown.unwrap_or(0.0)
	}
}

/// How a unit is displayed, in [`UnitData::display_type`].
#[variant_checkers]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DisplayType {
	/// Fully visible.
	Visible,
	/// Dimmed version of unit left behind after entering fog of war.
	Snapshot,
	/// Fully hidden.
	Hidden,
	/// Building that hasn't started construction.
	Placeholder,
}

impl FromProto<ProtoDisplayType> for DisplayType {
	fn from_proto(display_type: ProtoDisplayType) -> Self {
		match display_type {
			ProtoDisplayType::Visible => DisplayType::Visible,
			ProtoDisplayType::Snapshot => DisplayType::Snapshot,
			ProtoDisplayType::Hidden => DisplayType::Hidden,
			ProtoDisplayType::Placeholder => DisplayType::Placeholder,
		}
	}
}

/// Order given to a unit, in [`UnitData::orders`].
#[derive(Clone)]
pub struct UnitOrder {
	/// Ability unit is using.
	pub ability: AbilityId,
	/// Target of unit's ability.
	pub target: Target,
	/// Progress of train abilities. Value in range from `0` to `1`.
	pub progress: f32,
}

/// Unit inside a transport or bunker, in [`UnitData::passengers`].
#[derive(Clone)]
pub struct PassengerUnit {
	pub tag: UnitTag,
	pub health: f32,
	pub health_max: f32,
	pub shield: f32,
	pub shield_max: f32,
	pub energy: f32,
	pub energy_max: f32,
	pub type_id: UnitTypeId,
}

/// Rally point of a production building, in [`UnitData::rally_targets`].
#[derive(Clone)]
pub struct RallyTarget {
	/// Rally point. Position building rallied on.
	pub point: Point2,
	/// Filled if building is rallied on unit.
	pub tag: Option<UnitTag>,
}

/// Trait for radius
pub trait Radius {
	/// Radius for struct
	fn radius(&self) -> f32 {
		0.0
	}
}
