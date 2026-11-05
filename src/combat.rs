//! Weapons, ranges and damage.

use super::UnitRef;
use crate::{
	consts::{ANTI_ARMOR_BUFF, DAMAGE_BONUS_PER_UPGRADE, MISSED_WEAPONS},
	distance::Distance,
	game_data::{Attribute, TargetType, Weapon},
	geometry::Point2,
	ids::{AbilityId, BuffId, UnitTypeId, UpgradeId},
	unit::Radius,
};

/// Weapon target used in [`calculate_weapon_stats`](UnitRef::calculate_weapon_stats).
#[derive(Clone, Copy)]
pub enum CalcTarget<'a> {
	/// Specific unit.
	Unit(UnitRef<'a>),
	/// Abstract target with given type and attributes.
	Abstract(TargetType, &'a [Attribute]),
}

fn dps(w: &Weapon) -> f32 {
	w.damage as f32 * w.attacks as f32 / w.speed
}

impl<'w> UnitRef<'w> {
	/// Weapons of the unit.
	pub fn weapons(self) -> &'w [Weapon] {
		match self.type_id {
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
				.world()
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
	fn ground_weapon(self) -> Option<&'w Weapon> {
		self.weapons().iter().find(|w| !w.target.is_air())
	}
	fn air_weapon(self) -> Option<&'w Weapon> {
		self.weapons().iter().find(|w| !w.target.is_ground())
	}
	/// First weapon that can hit `target`. A colossus can be hit by anything,
	/// so for one this returns the weapon with the largest value of `key`.
	fn weapon_against(self, target: UnitRef<'_>, key: impl Fn(&Weapon) -> f32) -> f32 {
		let weapons = self.weapons();
		if target.type_id == UnitTypeId::Colossus {
			weapons.iter().map(key).max_by(f32::total_cmp).unwrap_or(0.0)
		} else {
			let not_target = if target.is_flying {
				TargetType::Ground
			} else {
				TargetType::Air
			};
			weapons.iter().find(|w| w.target != not_target).map_or(0.0, key)
		}
	}
	fn range_with_upgrades(self, w: &Weapon) -> f32 {
		let upgrades = self.upgrades();
		let bonus = match self.type_id {
			UnitTypeId::Hydralisk if upgrades.contains(&UpgradeId::EvolveGroovedSpines) => 1.0,
			UnitTypeId::Phoenix if upgrades.contains(&UpgradeId::PhoenixRangeUpgrade) => 2.0,
			UnitTypeId::PlanetaryFortress | UnitTypeId::MissileTurret | UnitTypeId::AutoTurret
				if upgrades.contains(&UpgradeId::HiSecAutoTracking) =>
			{
				1.0
			}
			_ => 0.0,
		};
		w.range + bonus
	}

	/// Targets the unit can attack, if it has a weapon.
	pub fn weapon_target(self) -> Option<TargetType> {
		match (self.can_attack_ground(), self.can_attack_air()) {
			(true, true) => Some(TargetType::Any),
			(true, false) => Some(TargetType::Ground),
			(false, true) => Some(TargetType::Air),
			(false, false) => None,
		}
	}
	/// Checks if unit has any weapons.
	pub fn can_attack(self) -> bool {
		!self.weapons().is_empty()
	}
	/// Checks if unit can attack both air and ground targets.
	pub fn can_attack_both(self) -> bool {
		self.can_attack_ground() && self.can_attack_air()
	}
	/// Checks if unit can attack ground targets.
	pub fn can_attack_ground(self) -> bool {
		self.ground_weapon().is_some()
	}
	/// Checks if unit can attack air targets.
	pub fn can_attack_air(self) -> bool {
		self.air_weapon().is_some()
	}
	/// Checks if unit can attack given target.
	pub fn can_attack_unit(self, target: UnitRef<'_>) -> bool {
		if target.type_id == UnitTypeId::Colossus {
			self.can_attack()
		} else if target.is_flying {
			self.can_attack_air()
		} else {
			self.can_attack_ground()
		}
	}
	/// Max cooldown in frames seen so far for this unit type's weapon.
	pub fn max_cooldown(self) -> Option<f32> {
		self.world().max_cooldowns.get(&self.type_id).copied()
	}
	/// Current cooldown divided by max cooldown.
	pub fn cooldown_percentage(self) -> Option<f32> {
		let current = self.weapon_cooldown?;
		let max = self.max_cooldown()?;
		if max == 0.0 {
			None
		} else {
			Some(current / max)
		}
	}
	/// Ground weapon range, without upgrades.
	pub fn ground_range(self) -> f32 {
		self.ground_weapon().map_or(0.0, |w| w.range)
	}
	/// Air weapon range, without upgrades.
	pub fn air_range(self) -> f32 {
		self.air_weapon().map_or(0.0, |w| w.range)
	}
	/// Weapon range against given target, without upgrades, or `0` if it can't be attacked.
	pub fn range_vs(self, target: UnitRef<'_>) -> f32 {
		self.weapon_against(target, |w| w.range)
	}
	/// Ground weapon range, including upgrades.
	pub fn real_ground_range(self) -> f32 {
		self.ground_weapon().map_or(0.0, |w| self.range_with_upgrades(w))
	}
	/// Air weapon range, including upgrades.
	pub fn real_air_range(self) -> f32 {
		self.air_weapon().map_or(0.0, |w| self.range_with_upgrades(w))
	}
	/// Weapon range against given target, including upgrades, or `0` if it can't be attacked.
	pub fn real_range_vs(self, target: UnitRef<'_>) -> f32 {
		self.weapon_against(target, |w| self.range_with_upgrades(w))
	}
	/// Ground dps, without upgrades.
	pub fn ground_dps(self) -> f32 {
		self.ground_weapon().map_or(0.0, dps)
	}
	/// Air dps, without upgrades.
	pub fn air_dps(self) -> f32 {
		self.air_weapon().map_or(0.0, dps)
	}
	/// Dps against given target, without upgrades, or `0` if it can't be attacked.
	pub fn dps_vs(self, target: UnitRef<'_>) -> f32 {
		self.weapon_against(target, dps)
	}
	/// (dps, range) of the best weapon, including buffs and upgrades.
	pub fn real_weapon(self, attributes: &[Attribute]) -> (f32, f32) {
		self.calculate_weapon_stats(CalcTarget::Abstract(TargetType::Any, attributes))
	}
	/// (dps, range) of the ground weapon, including buffs and upgrades.
	pub fn real_ground_weapon(self, attributes: &[Attribute]) -> (f32, f32) {
		self.calculate_weapon_stats(CalcTarget::Abstract(TargetType::Ground, attributes))
	}
	/// (dps, range) of the air weapon, including buffs and upgrades.
	pub fn real_air_weapon(self, attributes: &[Attribute]) -> (f32, f32) {
		self.calculate_weapon_stats(CalcTarget::Abstract(TargetType::Air, attributes))
	}
	/// (dps, range) against given target, including buffs and upgrades, or `(0, 0)` if it can't be attacked.
	pub fn real_weapon_vs(self, target: UnitRef<'_>) -> (f32, f32) {
		self.calculate_weapon_stats(CalcTarget::Unit(target))
	}
	/// (dps, range) against given abstract target, including buffs and upgrades.
	pub fn calculate_weapon_abstract(self, target: TargetType, attributes: &[Attribute]) -> (f32, f32) {
		self.calculate_weapon_stats(CalcTarget::Abstract(target, attributes))
	}
	/// (dps, range) against given target, including buffs and upgrades, or `(0, 0)` if it can't be attacked.
	pub fn calculate_weapon_stats(self, target: CalcTarget<'_>) -> (f32, f32) {
		let world = self.world();
		let (upgrades, target_upgrades) = if self.is_mine() {
			(&world.upgrades, &world.enemy_upgrades)
		} else {
			(&world.enemy_upgrades, &world.upgrades)
		};

		let (not_target, attributes, target_unit) = match target {
			CalcTarget::Unit(target) => {
				let mut armor = target.armor() + target.armor_upgrade_level;
				let mut shield_armor = target.shield_upgrade_level;
				let mut guardian_shield = false;

				for buff in &target.buffs {
					if *buff == BuffId::GuardianShield {
						guardian_shield = true;
					} else if *buff == ANTI_ARMOR_BUFF {
						armor -= 3;
						shield_armor -= 3;
					}
				}

				if !target_upgrades.is_empty() {
					if target.race().is_terran() {
						if target.is_structure() && target_upgrades.contains(&UpgradeId::TerranBuildingArmor)
						{
							armor += 2;
						}
					} else if matches!(
						target.type_id,
						UnitTypeId::Ultralisk | UnitTypeId::UltraliskBurrowed
					) && target_upgrades.contains(&UpgradeId::ChitinousPlating)
					{
						armor += 2;
					}
				}

				(
					if target.type_id == UnitTypeId::Colossus {
						TargetType::Any
					} else if target.is_flying {
						TargetType::Ground
					} else {
						TargetType::Air
					},
					target.attributes(),
					Some((target, armor, shield_armor, guardian_shield)),
				)
			}
			CalcTarget::Abstract(target, attributes) => (
				match target {
					TargetType::Any => TargetType::Any,
					TargetType::Ground => TargetType::Air,
					TargetType::Air => TargetType::Ground,
				},
				attributes,
				None,
			),
		};

		let weapons = self.weapons();
		if weapons.is_empty() {
			return (0.0, 0.0);
		}

		let mut speed_modifier = 1.0;
		let mut range_modifier = 0.0;

		for buff in &self.buffs {
			match buff {
				BuffId::Stimpack | BuffId::StimpackMarauder => speed_modifier /= 1.5,
				BuffId::TimeWarpProduction => speed_modifier *= 2.0,
				_ => {}
			}
		}

		if !upgrades.is_empty() {
			match self.type_id {
				UnitTypeId::Zergling if upgrades.contains(&UpgradeId::Zerglingattackspeed) => {
					speed_modifier /= 1.4
				}
				UnitTypeId::Adept if upgrades.contains(&UpgradeId::AdeptPiercingAttack) => {
					speed_modifier /= 1.45
				}
				UnitTypeId::Hydralisk if upgrades.contains(&UpgradeId::EvolveGroovedSpines) => {
					range_modifier += 1.0
				}
				UnitTypeId::Phoenix if upgrades.contains(&UpgradeId::PhoenixRangeUpgrade) => {
					range_modifier += 2.0
				}
				UnitTypeId::PlanetaryFortress | UnitTypeId::MissileTurret | UnitTypeId::AutoTurret
					if upgrades.contains(&UpgradeId::HiSecAutoTracking) =>
				{
					range_modifier += 1.0
				}
				_ => {}
			}
		}

		let bonus_per_upgrade = DAMAGE_BONUS_PER_UPGRADE.get(&self.type_id);
		let weapon_stats = |w: &Weapon| {
			let bonus_per_upgrade = bonus_per_upgrade.and_then(|bonus| bonus.get(&w.target));

			let mut damage = w.damage
				+ self.attack_upgrade_level * bonus_per_upgrade.and_then(|bonus| bonus.0).unwrap_or(1);
			let speed = w.speed * speed_modifier;
			let range = w.range + range_modifier;

			// Bonus damage
			let bonus = w
				.damage_bonus
				.iter()
				.filter(|(attribute, _)| attributes.contains(attribute))
				.map(|(attribute, bonus)| {
					let mut per_upgrade = bonus_per_upgrade
						.and_then(|bonus| bonus.1.get(attribute))
						.copied()
						.unwrap_or(0);
					if let Attribute::Light = attribute {
						if upgrades.contains(&UpgradeId::HighCapacityBarrels) {
							match self.type_id {
								UnitTypeId::Hellion => per_upgrade += 5,
								UnitTypeId::HellionTank => per_upgrade += 12,
								_ => {}
							}
						}
					}
					let mut bonus_damage = bonus + self.attack_upgrade_level * per_upgrade;
					if let Attribute::Armored = attribute {
						if self.has_buff(BuffId::VoidRaySwarmDamageBoost) {
							bonus_damage += 6;
						}
					}
					bonus_damage
				})
				.max();
			if let Some(bonus) = bonus {
				damage += bonus;
			}

			// Subtract damage
			match target_unit {
				Some((target, armor, shield_armor, guardian_shield)) => {
					let guardian = if guardian_shield && range >= 2.0 { 2 } else { 0 };
					let mut attacks = w.attacks;
					let mut shield_damage = 0;
					let mut health_damage = 0;

					if let Some(shield) = target.shield.filter(|shield| *shield > 0) {
						let exact_damage = 1.max(damage as i32 - (shield_armor + guardian)) as u32;
						for _ in 0..attacks {
							if shield_damage >= shield {
								health_damage = shield_damage - shield;
								break;
							}
							shield_damage += exact_damage;
							attacks -= 1;
						}
					}

					if let Some(health) = target.health.filter(|health| *health > 0) {
						let exact_damage = 1.max(damage as i32 - (armor + guardian)) as u32;
						for _ in 0..attacks {
							if health_damage >= health {
								break;
							}
							health_damage += exact_damage;
						}
					}

					(shield_damage + health_damage, speed, range)
				}
				None => (damage * w.attacks, speed, range),
			}
		};

		let (damage, speed, range) = weapons
			.iter()
			.filter(|w| not_target.is_any() || w.target != not_target)
			.map(weapon_stats)
			.max_by_key(|stats| stats.0)
			.unwrap_or((0, 0.0, 0.0));
		(if speed == 0.0 { 0.0 } else { damage as f32 / speed }, range)
	}

	fn within(self, target: UnitRef<'_>, range: f32, gap: f32) -> bool {
		if range < f32::EPSILON {
			return false;
		}
		let distance = self.position.distance_squared(target.position);
		// Takes into account that Sieged Tank has a minimum range of 2
		(self.type_id != UnitTypeId::SiegeTankSieged || distance > 4.0)
			&& distance <= (self.radius + target.radius + range + gap).powi(2)
	}
	/// Checks if unit is close enough to attack given target, without considering upgrades.
	pub fn in_range(self, target: UnitRef<'_>, gap: f32) -> bool {
		let range = if target.type_id == UnitTypeId::Colossus {
			self.range_vs(target)
		} else if target.is_flying {
			self.air_range()
		} else {
			self.ground_range()
		};
		self.within(target, range, gap)
	}
	/// Checks if unit is close enough to be attacked by given threat, without considering upgrades.
	pub fn in_range_of(self, threat: UnitRef<'_>, gap: f32) -> bool {
		threat.in_range(self, gap)
	}
	/// Checks if unit is close enough to attack given target, considering upgrades.
	pub fn in_real_range(self, target: UnitRef<'_>, gap: f32) -> bool {
		self.within(target, self.real_range_vs(target), gap)
	}
	/// Checks if unit is close enough to be attacked by given threat, considering upgrades.
	pub fn in_real_range_of(self, threat: UnitRef<'_>, gap: f32) -> bool {
		threat.in_real_range(self, gap)
	}
	/// Checks if unit is close enough to use given ability on target.
	pub fn in_ability_cast_range<A>(self, ability: AbilityId, target: A, gap: f32) -> bool
	where
		A: Into<Point2> + Radius,
	{
		let cast_range = self
			.world()
			.map
			.game_data
			.abilities
			.get(&ability)
			.and_then(|data| data.cast_range);
		match cast_range {
			Some(cast_range) => {
				let reach = cast_range + self.radius + target.radius() + gap;
				reach.powi(2) >= self.position.distance_squared(target)
			}
			None => false,
		}
	}
	/// (attribute, bonus damage) of the first weapon that has one.
	pub fn damage_bonus(self) -> Option<(Attribute, u32)> {
		self.weapons()
			.iter()
			.find_map(|w| w.damage_bonus.first())
			.copied()
	}
}
