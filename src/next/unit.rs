use super::World;
use crate::{
	action::Target,
	consts::{MISSED_WEAPONS, OFF_CREEP_SPEED_UPGRADES, SPEED_BUFFS, SPEED_ON_CREEP, SPEED_UPGRADES},
	distance::Distance,
	game_data::{Attribute, UnitTypeData, Weapon},
	game_state::Alliance,
	geometry::{Point2, Point3},
	ids::{AbilityId, BuffId, UnitTypeId, UpgradeId},
	pixel_map::VisibilityMap,
	player::Race,
	unit::{DisplayType, PassengerUnit, RallyTarget, UnitOrder},
	FromProto,
};
use num_traits::FromPrimitive;
use rustc_hash::FxHashSet;
use sc2_proto::raw::{unit_order::Target as ProtoTarget, CloakState as ProtoCloakState, Unit as ProtoUnit};
use std::ops::Deref;

/// What SC2 reported about a unit on one step: plain data with public fields.
///
/// `Clone` it if you want to keep a unit's state between steps. It's a value,
/// so nothing about it can change or go half-stale behind your back.
#[derive(Clone)]
#[allow(missing_docs)]
pub struct UnitData {
	pub tag: u64,
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
	pub addon_tag: Option<u64>,
	pub passengers: Vec<PassengerUnit>,
	pub cargo_space_taken: Option<u32>,
	pub cargo_space_max: Option<u32>,
	pub assigned_harvesters: Option<u32>,
	pub ideal_harvesters: Option<u32>,
	pub weapon_cooldown: Option<f32>,
	pub engaged_target_tag: Option<u64>,
	pub buff_duration_remain: Option<u32>,
	pub buff_duration_max: Option<u32>,
	pub rally_targets: Vec<RallyTarget>,
}

impl UnitData {
	pub(super) fn from_proto(visibility: &VisibilityMap, u: &ProtoUnit) -> Self {
		let pos = u.pos.get_or_default();
		let position = Point2::from_proto(pos);
		let type_id = {
			let id = u.unit_type();
			UnitTypeId::from_u32(id).unwrap_or_else(|| panic!("There's no `UnitTypeId` with value {}", id))
		};
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
		Self {
			tag: u.tag(),
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
				.map(|b| {
					BuffId::from_u32(*b).unwrap_or_else(|| panic!("There's no `BuffId` with value {}", b))
				})
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
					ability: {
						let id = order.ability_id();
						AbilityId::from_u32(id)
							.unwrap_or_else(|| panic!("There's no `AbilityId` with value {}", id))
					},
					target: match &order.target {
						Some(ProtoTarget::TargetWorldSpacePos(pos)) => Target::Pos(Point2::from_proto(pos)),
						Some(ProtoTarget::TargetUnitTag(tag)) => Target::Tag(*tag),
						Some(_) => panic!("Unhandled target"),
						None => Target::None,
					},
					progress: order.progress(),
				})
				.collect(),
			addon_tag: u.add_on_tag,
			passengers: u
				.passengers
				.iter()
				.map(|p| PassengerUnit {
					tag: p.tag(),
					health: p.health(),
					health_max: p.health_max(),
					shield: p.shield(),
					shield_max: p.shield_max(),
					energy: p.energy(),
					energy_max: p.energy_max(),
					type_id: {
						let id = p.unit_type();
						UnitTypeId::from_u32(id)
							.unwrap_or_else(|| panic!("There's no `UnitTypeId` with value {}", id))
					},
				})
				.collect(),
			cargo_space_taken: u.cargo_space_taken.map(|x| x as u32),
			cargo_space_max: u.cargo_space_max.map(|x| x as u32),
			assigned_harvesters: u.assigned_harvesters.map(|x| x as u32),
			ideal_harvesters: u.ideal_harvesters.map(|x| x as u32),
			weapon_cooldown: u.weapon_cooldown,
			engaged_target_tag: u.engaged_target_tag,
			buff_duration_remain: u.buff_duration_remain.map(|x| x as u32),
			buff_duration_max: u.buff_duration_max.map(|x| x as u32),
			rally_targets: u
				.rally_targets
				.iter()
				.map(|t| RallyTarget {
					point: Point2::from_proto(t.point.get_or_default()),
					tag: t.tag,
				})
				.collect(),
		}
	}

	/// Summed health and shield. Not populated for snapshots.
	pub fn hits(&self) -> Option<u32> {
		match (self.health, self.shield) {
			(Some(health), Some(shield)) => Some(health + shield),
			(Some(health), None) => Some(health),
			(None, Some(shield)) => Some(shield),
			(None, None) => None,
		}
	}
	/// Checks if unit is yours.
	pub fn is_mine(&self) -> bool {
		self.alliance.is_mine()
	}
	/// Checks if unit is an enemy's.
	pub fn is_enemy(&self) -> bool {
		self.alliance.is_enemy()
	}
	/// Building construction is complete.
	pub fn is_ready(&self) -> bool {
		(self.build_progress - 1.0).abs() < f32::EPSILON
	}
	/// Building construction is more than 95% complete.
	pub fn is_almost_ready(&self) -> bool {
		self.build_progress >= 0.95
	}
	/// Unit is fully visible.
	pub fn is_visible(&self) -> bool {
		self.display_type.is_visible()
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
}

/// A unit on the current step, together with the [`World`] it belongs to.
///
/// It's two references, so it's `Copy` and free to pass around. Plain data is
/// available directly through `Deref` (`u.tag`, `u.position`), while methods that
/// need game rules read them from the world. The lifetime ties it to its step.
#[derive(Clone, Copy)]
pub struct UnitRef<'w> {
	data: &'w UnitData,
	world: &'w World,
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

impl<'w> UnitRef<'w> {
	pub(super) fn new(data: &'w UnitData, world: &'w World) -> Self {
		Self { data, world }
	}
	/// The world this unit belongs to.
	pub fn world(self) -> &'w World {
		self.world
	}
	/// The plain data, for example to clone and keep it.
	pub fn data(self) -> &'w UnitData {
		self.data
	}

	fn type_data(self) -> Option<&'w UnitTypeData> {
		self.world.map.game_data.units.get(&self.data.type_id)
	}
	fn upgrades(self) -> &'w FxHashSet<UpgradeId> {
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
	/// Checks if unit has given attribute.
	pub fn has_attribute(self, attribute: Attribute) -> bool {
		self.type_data()
			.map_or(false, |d| d.attributes.contains(&attribute))
	}
	/// Checks if unit is a structure.
	pub fn is_structure(self) -> bool {
		self.has_attribute(Attribute::Structure)
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
	/// A unit that merely lost its health reading, such as one that became a snapshot,
	/// doesn't count. The old `Unit::is_attacked` reports `true` in that case.
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
	/// Checks if ability is available for the unit.
	pub fn has_ability(self, ability: AbilityId) -> bool {
		self.world
			.abilities
			.get(&self.data.tag)
			.map_or(false, |a| a.contains(&ability))
	}
	/// Max cooldown in frames seen so far for this unit type's weapon.
	pub fn max_cooldown(self) -> Option<f32> {
		self.world.max_cooldowns.get(&self.data.type_id).copied()
	}
	/// Weapons of the unit.
	pub fn weapons(self) -> &'w [Weapon] {
		match self.data.type_id {
			UnitTypeId::Changeling
			| UnitTypeId::ChangelingZealot
			| UnitTypeId::ChangelingMarineShield
			| UnitTypeId::ChangelingMarine
			| UnitTypeId::ChangelingZerglingWings
			| UnitTypeId::ChangelingZergling => &[],
			UnitTypeId::Baneling | UnitTypeId::BanelingBurrowed | UnitTypeId::BanelingCocoon => {
				&MISSED_WEAPONS[&UnitTypeId::Baneling]
			}
			UnitTypeId::RavagerCocoon => self
				.world
				.map
				.game_data
				.units
				.get(&UnitTypeId::Ravager)
				.map_or(&[], |d| d.weapons.as_slice()),
			unit_type => self
				.type_data()
				.map(|d| d.weapons.as_slice())
				.filter(|w| !w.is_empty())
				.or_else(|| MISSED_WEAPONS.get(&unit_type).map(|w| w.as_slice()))
				.unwrap_or_default(),
		}
	}
	/// Checks if unit can attack ground targets.
	pub fn can_attack_ground(self) -> bool {
		self.weapons().iter().any(|w| !w.target.is_air())
	}
	/// Checks if unit can attack air targets.
	pub fn can_attack_air(self) -> bool {
		self.weapons().iter().any(|w| !w.target.is_ground())
	}
	/// Ground weapon range, without upgrades.
	pub fn ground_range(self) -> f32 {
		self.weapons()
			.iter()
			.find(|w| !w.target.is_air())
			.map_or(0.0, |w| w.range)
	}
	/// Air weapon range, without upgrades.
	pub fn air_range(self) -> f32 {
		self.weapons()
			.iter()
			.find(|w| !w.target.is_ground())
			.map_or(0.0, |w| w.range)
	}
	/// Checks if unit is close enough to attack given target, without considering upgrades.
	pub fn in_range(self, target: UnitRef<'_>, gap: f32) -> bool {
		let range = if target.type_id == UnitTypeId::Colossus {
			match self.weapons().iter().map(|w| w.range).max_by(f32::total_cmp) {
				Some(max_range) => max_range,
				None => return false,
			}
		} else {
			let range = if target.is_flying {
				self.air_range()
			} else {
				self.ground_range()
			};
			if range < f32::EPSILON {
				return false;
			}
			range
		};
		let distance = self.data.position.distance_squared(target.position);

		// Takes into account that Sieged Tank has a minimum range of 2
		(self.data.type_id != UnitTypeId::SiegeTankSieged || distance > 4.0)
			&& distance <= (self.data.radius + target.radius + range + gap).powi(2)
	}
	/// Checks if unit is close enough to be attacked by given threat.
	pub fn in_range_of(self, threat: UnitRef<'_>, gap: f32) -> bool {
		threat.in_range(self, gap)
	}
	/// Movement speed from unit data, without buffs and upgrades.
	pub fn speed(self) -> f32 {
		self.type_data().map_or(0.0, |d| d.movement_speed)
	}
	/// Actual speed of the unit, including buffs, upgrades and creep.
	///
	/// Not cached yet. The old API caches this per unit, which is what puts a
	/// lazy cell in every unit; a cache here should be measured first.
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
		if self.world.creep[self.data.position].is_set() {
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
}
