//! Types describing what's happening in the game, available through [`World`](crate::World).

use crate::{geometry::Point2, ids::*, unit::UnitTag, FromProto};
use sc2_proto::{
	raw::{Alliance as ProtoAlliance, PowerSource as ProtoPowerSource},
	sc2api::Alert as ProtoAlert,
};

/// Messege in game chat.
#[derive(Clone)]
pub struct ChatMessage {
	/// Id of player who sent that message.
	pub player_id: u32,
	/// Actual message.
	pub message: String,
}

/// Power matrix from the pylon or warp prism, used to give power to buildings and warp units on it.
#[derive(Clone)]
pub struct PsionicMatrix {
	/// Position of psionic matrix source.
	pub pos: Point2,
	/// Radius of the matrix: shold be `6.5` for pylon and `3.75` for warp prism in phsing mode.
	pub radius: f32,
	/// Tag of unit that is source of power matrix.
	pub tag: UnitTag,
}
impl FromProto<&ProtoPowerSource> for PsionicMatrix {
	fn from_proto(ps: &ProtoPowerSource) -> Self {
		Self {
			pos: Point2::from_proto(ps.pos.get_or_default()),
			radius: ps.radius(),
			tag: UnitTag(ps.tag()),
		}
	}
}

/// There are different effects in SC2, some of them can harm your units,
/// so take them into account when microing.
///
/// All effects are in [`World::effects`](crate::World::effects).
#[derive(Clone)]
pub struct Effect {
	/// Type of the effect.
	pub id: EffectId,
	/// Positions covered by this effect.
	pub positions: Vec<Point2>,
	/// Is this effect yours or opponent's.
	pub alliance: Alliance,
	/// Player id of effect's owner.
	pub owner: u32,
	/// Additional radius covered by effect around every it's position.
	pub radius: f32,
}

/// The alliance of unit or effect to your bot.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Alliance {
	/// Your own objects.
	Own,
	/// Owned by your allias (in 2v2, 3v3 an 4v4 games).
	Ally,
	/// Just neutral object.
	Neutral,
	/// Owned by your opponent.
	Enemy,
}
impl Alliance {
	pub fn is_mine(self) -> bool {
		matches!(self, Alliance::Own)
	}
	pub fn is_enemy(self) -> bool {
		matches!(self, Alliance::Enemy)
	}
	pub fn is_neutral(self) -> bool {
		matches!(self, Alliance::Neutral)
	}
	pub fn is_ally(self) -> bool {
		matches!(self, Alliance::Ally)
	}
}
impl FromProto<ProtoAlliance> for Alliance {
	fn from_proto(alliance: ProtoAlliance) -> Self {
		match alliance {
			ProtoAlliance::Self_ => Alliance::Own,
			ProtoAlliance::Ally => Alliance::Ally,
			ProtoAlliance::Neutral => Alliance::Neutral,
			ProtoAlliance::Enemy => Alliance::Enemy,
		}
	}
}

/// Radar point on the map.
#[derive(Clone)]
pub struct Radar {
	/// Position where radar is.
	pub pos: Point2,
	/// Radius covered by radar (Pretty sure it's `12`).
	pub radius: f32,
}

/// Common information of player.
#[derive(Default, Clone)]
pub struct Common {
	/// In-game player id.
	pub player_id: u32,
	/// Amount of minerals bot currently has.
	pub minerals: u32,
	/// Amount of vespene gas bot currently has.
	pub vespene: u32,
	/// Supply capacity.
	pub food_cap: u32,
	/// Supply used in total.
	pub food_used: u32,
	/// Supply used by combat units.
	pub food_army: u32,
	/// Supply used by workers (workers count).
	pub food_workers: u32,
	/// The count of your idle workers.
	pub idle_worker_count: u32,
	/// The count of your combat units.
	pub army_count: u32,
	/// The count of your free warp gates.
	pub warp_gate_count: u32,
	/// The count of your larva.
	pub larva_count: u32,
}

/// Different kinds of alert that can happen.
/// All alerts are in [`World::alerts`](crate::World::alerts).
#[allow(missing_docs)]
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone)]
pub enum Alert {
	AlertError,
	AddOnComplete,
	BuildingComplete,
	BuildingUnderAttack,
	LarvaHatched,
	MergeComplete,
	MineralsExhausted,
	MorphComplete,
	MothershipComplete,
	MULEExpired,
	NuclearLaunchDetected,
	NukeComplete,
	NydusWormDetected,
	ResearchComplete,
	TrainError,
	TrainUnitComplete,
	TrainWorkerComplete,
	TransformationComplete,
	UnitUnderAttack,
	UpgradeComplete,
	VespeneExhausted,
	WarpInComplete,
}
impl FromProto<ProtoAlert> for Alert {
	fn from_proto(alert: ProtoAlert) -> Self {
		match alert {
			ProtoAlert::AlertError => Alert::AlertError,
			ProtoAlert::AddOnComplete => Alert::AddOnComplete,
			ProtoAlert::BuildingComplete => Alert::BuildingComplete,
			ProtoAlert::BuildingUnderAttack => Alert::BuildingUnderAttack,
			ProtoAlert::LarvaHatched => Alert::LarvaHatched,
			ProtoAlert::MergeComplete => Alert::MergeComplete,
			ProtoAlert::MineralsExhausted => Alert::MineralsExhausted,
			ProtoAlert::MorphComplete => Alert::MorphComplete,
			ProtoAlert::MothershipComplete => Alert::MothershipComplete,
			ProtoAlert::MULEExpired => Alert::MULEExpired,
			ProtoAlert::NuclearLaunchDetected => Alert::NuclearLaunchDetected,
			ProtoAlert::NukeComplete => Alert::NukeComplete,
			ProtoAlert::NydusWormDetected => Alert::NydusWormDetected,
			ProtoAlert::ResearchComplete => Alert::ResearchComplete,
			ProtoAlert::TrainError => Alert::TrainError,
			ProtoAlert::TrainUnitComplete => Alert::TrainUnitComplete,
			ProtoAlert::TrainWorkerComplete => Alert::TrainWorkerComplete,
			ProtoAlert::TransformationComplete => Alert::TransformationComplete,
			ProtoAlert::UnitUnderAttack => Alert::UnitUnderAttack,
			ProtoAlert::UpgradeComplete => Alert::UpgradeComplete,
			ProtoAlert::VespeneExhausted => Alert::VespeneExhausted,
			ProtoAlert::WarpInComplete => Alert::WarpInComplete,
		}
	}
}

#[derive(Clone)]
pub struct AvailableAbility {
	pub id: AbilityId,
	pub requires_point: bool,
}
