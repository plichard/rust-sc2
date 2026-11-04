use super::UnitRef;
use crate::{
	action::{Action, Target},
	geometry::Point2,
	ids::{AbilityId, UnitTypeId, UpgradeId},
};
use rustc_hash::FxHashMap;

/// Orders collected during your turn and sent to SC2 when it ends.
///
/// Orders take a [`UnitRef`], so you can only command a unit that exists on this step.
#[derive(Default)]
pub struct Commands {
	commands: Vec<(AbilityId, Target, bool, Vec<u64>)>,
	autocast: FxHashMap<AbilityId, Vec<u64>>,
	chat: Vec<(String, bool)>,
	pub(super) game_step: Option<u32>,
	pub(super) enemy_upgrades: Vec<UpgradeId>,
	pub(super) leave_game: bool,
}

impl Commands {
	/// Orders unit to execute given command.
	/// Identical orders are merged into one action, as in the old API.
	pub fn command(&mut self, unit: UnitRef<'_>, ability: AbilityId, target: Target, queue: bool) {
		for (a, t, q, units) in &mut self.commands {
			if *a == ability && *t == target && *q == queue {
				units.push(unit.tag);
				return;
			}
		}
		self.commands.push((ability, target, queue, vec![unit.tag]));
	}
	/// Orders unit to attack given target.
	pub fn attack(&mut self, unit: UnitRef<'_>, target: Target, queue: bool) {
		self.command(unit, AbilityId::Attack, target, queue)
	}
	/// Orders unit to move to given target.
	pub fn move_to(&mut self, unit: UnitRef<'_>, target: Target, queue: bool) {
		self.command(unit, AbilityId::MoveMove, target, queue)
	}
	/// Orders worker to gather given resource.
	pub fn gather(&mut self, unit: UnitRef<'_>, target: u64, queue: bool) {
		self.command(unit, AbilityId::HarvestGather, Target::Tag(target), queue)
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
	/// Toggles autocast on given ability.
	pub fn toggle_autocast(&mut self, unit: UnitRef<'_>, ability: AbilityId) {
		self.autocast.entry(ability).or_default().push(unit.tag);
	}
	/// Sends message to game chat.
	pub fn chat(&mut self, message: &str, team_only: bool) {
		self.chat.push((message.to_string(), team_only));
	}
	/// Sets frames between steps, starting with the step this turn requests.
	pub fn set_game_step(&mut self, frames: u32) {
		self.game_step = Some(frames);
	}
	/// Records an enemy upgrade you inferred. Visible in [`World`](super::World) from the next step.
	pub fn note_enemy_upgrade(&mut self, upgrade: UpgradeId) {
		self.enemy_upgrades.push(upgrade);
	}
	/// Leaves the game when this turn ends.
	pub fn leave_game(&mut self) {
		self.leave_game = true;
	}

	pub(super) fn take_actions(&mut self) -> Vec<Action> {
		let chat = self.chat.drain(..).map(|(m, team)| Action::Chat(m, team));
		let commands = self
			.commands
			.drain(..)
			.map(|(ability, target, queue, units)| Action::UnitCommand(ability, target, units, queue));
		let autocast = self
			.autocast
			.drain()
			.map(|(ability, units)| Action::ToggleAutocast(ability, units));
		chat.chain(commands).chain(autocast).collect()
	}
}
