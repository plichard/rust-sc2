use super::World;
use super::{UnitRef, UnitTag};
use crate::{
	action::{Action, Target},
	consts::WARPGATE_ABILITIES,
	geometry::{Point2, Point3},
	ids::{AbilityId, UnitTypeId, UpgradeId},
};
use rustc_hash::FxHashMap;

impl From<Point2> for Target {
	#[inline]
	fn from(pos: Point2) -> Self {
		Target::Pos(pos)
	}
}

/// Orders collected during your turn and sent to SC2 when it ends.
///
/// Orders take a [`UnitRef`], so you can only command a unit that exists on this step.
#[derive(Default)]
pub struct Commands {
	commands: Vec<(AbilityId, Target, bool, Vec<u64>)>,
	autocast: FxHashMap<AbilityId, Vec<u64>>,
	actions: Vec<Action>,
	pub(super) sleeping: Vec<(UnitTag, u32)>,
	pub(super) game_step: Option<u32>,
	pub(super) enemy_upgrades: Vec<UpgradeId>,
	pub(super) leave_game: bool,
}

impl Commands {
	/// Orders unit to execute given command.
	/// Identical orders are merged into one action.
	///
	/// `target` can be a [`Target`], a position, a unit or a tag.
	pub fn command(&mut self, unit: UnitRef<'_>, ability: AbilityId, target: impl Into<Target>, queue: bool) {
		let target = target.into();
		for (a, t, q, units) in &mut self.commands {
			if *a == ability && *t == target && *q == queue {
				units.push(unit.tag.get());
				return;
			}
		}
		self.commands.push((ability, target, queue, vec![unit.tag.get()]));
	}
	/// Orders unit to attack given target.
	pub fn attack(&mut self, unit: UnitRef<'_>, target: impl Into<Target>, queue: bool) {
		self.command(unit, AbilityId::Attack, target, queue)
	}
	/// Orders unit to move to given target.
	pub fn move_to(&mut self, unit: UnitRef<'_>, target: impl Into<Target>, queue: bool) {
		self.command(unit, AbilityId::MoveMove, target, queue)
	}
	/// Orders worker to gather given resource.
	pub fn gather(&mut self, unit: UnitRef<'_>, resource: impl Into<UnitTag>, queue: bool) {
		self.command(unit, AbilityId::HarvestGather, resource.into(), queue)
	}
	/// Orders production building to train given unit.
	pub fn train(&mut self, unit: UnitRef<'_>, type_id: UnitTypeId, queue: bool) {
		if let Some(ability) = unit
			.world()
			.map
			.game_data
			.units
			.get(&type_id)
			.and_then(|d| d.ability)
		{
			self.command(unit, ability, Target::None, queue);
		}
	}
	/// Orders worker to build something on given position.
	pub fn build(&mut self, unit: UnitRef<'_>, type_id: UnitTypeId, pos: Point2, queue: bool) {
		if let Some(ability) = unit
			.world()
			.map
			.game_data
			.units
			.get(&type_id)
			.and_then(|d| d.ability)
		{
			self.command(unit, ability, Target::Pos(pos), queue);
		}
	}
	/// Orders unit to use given ability.
	pub fn use_ability(&mut self, unit: UnitRef<'_>, ability: AbilityId, queue: bool) {
		self.command(unit, ability, Target::None, queue)
	}
	/// Orders unit a `Smart` ability (the equivalent of a right click).
	pub fn smart(&mut self, unit: UnitRef<'_>, target: impl Into<Target>, queue: bool) {
		self.command(unit, AbilityId::Smart, target, queue)
	}
	/// Orders unit to hold position.
	pub fn hold_position(&mut self, unit: UnitRef<'_>, queue: bool) {
		self.use_ability(unit, AbilityId::HoldPosition, queue)
	}
	/// Orders worker to return resource to closest base.
	pub fn return_resource(&mut self, unit: UnitRef<'_>, queue: bool) {
		self.use_ability(unit, AbilityId::HarvestReturn, queue)
	}
	/// Orders unit to stop actions.
	pub fn stop(&mut self, unit: UnitRef<'_>, queue: bool) {
		self.use_ability(unit, AbilityId::Stop, queue)
	}
	/// Orders unit to patrol.
	pub fn patrol(&mut self, unit: UnitRef<'_>, target: impl Into<Target>, queue: bool) {
		self.command(unit, AbilityId::Patrol, target, queue)
	}
	/// Orders SCV or MULE to repair given structure or mechanical unit.
	pub fn repair(&mut self, unit: UnitRef<'_>, target: impl Into<UnitTag>, queue: bool) {
		self.command(unit, AbilityId::EffectRepair, target.into(), queue)
	}
	/// Orders building which is in progress to cancel construction.
	pub fn cancel_building(&mut self, unit: UnitRef<'_>, queue: bool) {
		self.use_ability(unit, AbilityId::CancelBuildInProgress, queue)
	}
	/// Orders production building to cancel the last unit in its queue.
	pub fn cancel_queue(&mut self, unit: UnitRef<'_>, queue: bool) {
		let ability = if unit.is_townhall() {
			AbilityId::CancelQueueCancelToSelection
		} else {
			AbilityId::CancelQueue5
		};
		self.use_ability(unit, ability, queue)
	}
	/// Orders worker to build your race's gas building on given geyser.
	pub fn build_gas(&mut self, unit: UnitRef<'_>, geyser: impl Into<UnitTag>, queue: bool) {
		let map = &unit.world().map;
		if let Some(ability) = map
			.game_data
			.units
			.get(&map.race_values.gas)
			.and_then(|d| d.ability)
		{
			self.command(unit, ability, geyser.into(), queue);
		}
	}
	/// Orders building to research given upgrade.
	pub fn research(&mut self, unit: UnitRef<'_>, upgrade: UpgradeId, queue: bool) {
		match upgrade {
			UpgradeId::TerranVehicleAndShipArmorsLevel1
			| UpgradeId::TerranVehicleAndShipArmorsLevel2
			| UpgradeId::TerranVehicleAndShipArmorsLevel3 => {
				self.use_ability(unit, AbilityId::ResearchTerranVehicleAndShipPlating, queue)
			}
			_ => {
				if let Some(data) = unit.world().map.game_data.upgrades.get(&upgrade) {
					self.use_ability(unit, data.ability, queue);
				}
			}
		}
	}
	/// Orders protoss warp gate to warp unit on given position.
	pub fn warp_in(&mut self, unit: UnitRef<'_>, type_id: UnitTypeId, pos: Point2) {
		if let Some(ability) = WARPGATE_ABILITIES.get(&type_id) {
			self.command(unit, *ability, Target::Pos(pos), false);
		}
	}
	/// Orders terran building to lift in the air.
	pub fn lift(&mut self, unit: UnitRef<'_>, queue: bool) {
		self.use_ability(unit, AbilityId::Lift, queue)
	}
	/// Orders flying terran building to land on given position.
	pub fn land(&mut self, unit: UnitRef<'_>, pos: Point2, queue: bool) {
		self.command(unit, AbilityId::Land, Target::Pos(pos), queue)
	}
	/// Toggles autocast on given ability.
	pub fn toggle_autocast(&mut self, unit: UnitRef<'_>, ability: AbilityId) {
		self.autocast.entry(ability).or_default().push(unit.tag.get());
	}
	/// Makes [`is_sleeping`](UnitRef::is_sleeping) return `true` for this unit for given
	/// number of frames, starting from the next step.
	pub fn sleep(&mut self, unit: UnitRef<'_>, frames: u32) {
		self.sleeping.push((unit.tag, frames));
	}
	/// Moves player camera to given position.
	pub fn move_camera(&mut self, pos: Point3) {
		self.actions.push(Action::CameraMove(pos));
	}
	/// Sends message to game chat.
	pub fn chat(&mut self, message: &str) {
		self.actions.push(Action::Chat(message.to_string(), false));
	}
	/// Sends message to allies only.
	pub fn chat_ally(&mut self, message: &str) {
		self.actions.push(Action::Chat(message.to_string(), true));
	}
	/// Sets frames between steps, starting with the step this turn requests.
	pub fn set_game_step(&mut self, frames: u32) {
		self.game_step = Some(frames);
	}
	/// Records an enemy upgrade you inferred. Visible in [`World`] from the next step.
	pub fn note_enemy_upgrade(&mut self, upgrade: UpgradeId) {
		self.enemy_upgrades.push(upgrade);
	}
	/// Leaves the game when this turn ends. Counts as a defeat, and `on_end` isn't called.
	pub fn leave_game(&mut self) {
		self.leave_game = true;
	}

	pub(super) fn take_actions(&mut self) -> Vec<Action> {
		let actions = self.actions.drain(..);
		let commands = self
			.commands
			.drain(..)
			.map(|(ability, target, queue, units)| Action::UnitCommand(ability, target, units, queue));
		let autocast = self
			.autocast
			.drain()
			.map(|(ability, units)| Action::ToggleAutocast(ability, units));
		actions.chain(commands).chain(autocast).collect()
	}
}

/// Resources left to spend during this turn. Reset from [`World`] on every step.
///
/// The world always reports what SC2 said, so what you've already
/// committed to spend this turn is tracked here instead.
#[derive(Default, Clone, Copy, Debug)]
pub struct Budget {
	/// Minerals not spent yet.
	pub minerals: u32,
	/// Vespene not spent yet.
	pub vespene: u32,
	/// Supply used, including what you've committed to.
	pub supply_used: u32,
	/// Supply still free.
	pub supply_left: u32,
}

impl Budget {
	pub(super) fn new(world: &World) -> Self {
		Self {
			minerals: world.minerals,
			vespene: world.vespene,
			supply_used: world.supply_used,
			supply_left: world.supply_left,
		}
	}
	/// Checks if you have enough resources and optionally supply to build given unit type.
	pub fn can_afford(&self, world: &World, unit: UnitTypeId, check_supply: bool) -> bool {
		let cost = world.get_unit_cost(unit);
		self.minerals >= cost.minerals
			&& self.vespene >= cost.vespene
			&& (!check_supply || self.supply_left as f32 >= cost.supply)
	}
	/// Checks if you have enough resources to make given upgrade.
	pub fn can_afford_upgrade(&self, world: &World, upgrade: UpgradeId) -> bool {
		let cost = world.get_upgrade_cost(upgrade);
		self.minerals >= cost.minerals && self.vespene >= cost.vespene
	}
	/// Subtracts the cost of given unit type.
	pub fn subtract_resources(&mut self, world: &World, unit: UnitTypeId, subtract_supply: bool) {
		let cost = world.get_unit_cost(unit);
		self.minerals = self.minerals.saturating_sub(cost.minerals);
		self.vespene = self.vespene.saturating_sub(cost.vespene);
		if subtract_supply {
			let supply = cost.supply as u32;
			self.supply_used += supply;
			self.supply_left = self.supply_left.saturating_sub(supply);
		}
	}
	/// Subtracts the cost of given upgrade.
	pub fn subtract_upgrade_cost(&mut self, world: &World, upgrade: UpgradeId) {
		let cost = world.get_upgrade_cost(upgrade);
		self.minerals = self.minerals.saturating_sub(cost.minerals);
		self.vespene = self.vespene.saturating_sub(cost.vespene);
	}
}
