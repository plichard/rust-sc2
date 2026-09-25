//! SC2 Score interface.

use crate::{FromProto, IntoSC2};
use sc2_proto::score::{
	score::ScoreType as Score_ScoreType, CategoryScoreDetails, Score as ProtoScore, VitalScoreDetails,
};

#[variant_checkers]
#[derive(Clone, Default)]
pub enum ScoreType {
	#[default]
	Curriculum,
	Melee,
}
impl FromProto<Score_ScoreType> for ScoreType {
	fn from_proto(score_type: Score_ScoreType) -> Self {
		match score_type {
			Score_ScoreType::Curriculum => ScoreType::Curriculum,
			Score_ScoreType::Melee => ScoreType::Melee,
		}
	}
}

#[derive(Default, Clone)]
pub struct Category {
	pub none: f32,
	pub army: f32,
	pub economy: f32,
	pub technology: f32,
	pub upgrade: f32,
}
impl FromProto<&CategoryScoreDetails> for Category {
	fn from_proto(category: &CategoryScoreDetails) -> Self {
		Self {
			none: category.none(),
			army: category.army(),
			economy: category.economy(),
			technology: category.technology(),
			upgrade: category.upgrade(),
		}
	}
}

#[derive(Default, Clone)]
pub struct Vital {
	pub life: f32,
	pub shields: f32,
	pub energy: f32,
}
impl FromProto<&VitalScoreDetails> for Vital {
	fn from_proto(vital: &VitalScoreDetails) -> Self {
		Self {
			life: vital.life(),
			shields: vital.shields(),
			energy: vital.energy(),
		}
	}
}

/// All kinds of scores stored here.
///
/// Can be accessed through [state.observation.score](crate::game_state::Observation::score).
#[derive(Default, Clone)]
pub struct Score {
	pub score_type: ScoreType,
	pub total_score: i32,
	// score details
	pub idle_production_time: f32,
	pub idle_worker_time: f32,
	pub total_value_units: f32,
	pub total_value_structures: f32,
	pub killed_value_units: f32,
	pub killed_value_structures: f32,
	pub collected_minerals: f32,
	pub collected_vespene: f32,
	pub collection_rate_minerals: f32,
	pub collection_rate_vespene: f32,
	pub spent_minerals: f32,
	pub spent_vespene: f32,
	pub food_used: Category,
	pub killed_minerals: Category,
	pub killed_vespene: Category,
	pub lost_minerals: Category,
	pub lost_vespene: Category,
	pub friendly_fire_minerals: Category,
	pub friendly_fire_vespene: Category,
	pub used_minerals: Category,
	pub used_vespene: Category,
	pub total_used_minerals: Category,
	pub total_used_vespene: Category,
	pub total_damage_dealt: Vital,
	pub total_damage_taken: Vital,
	pub total_healed: Vital,
	pub current_apm: f32,
	pub current_effective_apm: f32,
}
impl FromProto<&ProtoScore> for Score {
	fn from_proto(score: &ProtoScore) -> Self {
		let details = &score.score_details;
		Self {
			score_type: score.score_type().into_sc2(),
			total_score: score.score(),
			idle_production_time: details.idle_production_time(),
			idle_worker_time: details.idle_worker_time(),
			total_value_units: details.total_value_units(),
			total_value_structures: details.total_value_structures(),
			killed_value_units: details.killed_value_units(),
			killed_value_structures: details.killed_value_structures(),
			collected_minerals: details.collected_minerals(),
			collected_vespene: details.collected_vespene(),
			collection_rate_minerals: details.collection_rate_minerals(),
			collection_rate_vespene: details.collection_rate_vespene(),
			spent_minerals: details.spent_minerals(),
			spent_vespene: details.spent_vespene(),
			food_used: details.food_used.get_or_default().into_sc2(),
			killed_minerals: details.killed_minerals.get_or_default().into_sc2(),
			killed_vespene: details.killed_vespene.get_or_default().into_sc2(),
			lost_minerals: details.lost_minerals.get_or_default().into_sc2(),
			lost_vespene: details.lost_vespene.get_or_default().into_sc2(),
			friendly_fire_minerals: details.friendly_fire_minerals.get_or_default().into_sc2(),
			friendly_fire_vespene: details.friendly_fire_vespene.get_or_default().into_sc2(),
			used_minerals: details.used_minerals.get_or_default().into_sc2(),
			used_vespene: details.used_vespene.get_or_default().into_sc2(),
			total_used_minerals: details.total_used_minerals.get_or_default().into_sc2(),
			total_used_vespene: details.total_used_vespene.get_or_default().into_sc2(),
			total_damage_dealt: details.total_damage_dealt.get_or_default().into_sc2(),
			total_damage_taken: details.total_damage_taken.get_or_default().into_sc2(),
			total_healed: details.total_healed.get_or_default().into_sc2(),
			current_apm: details.current_apm(),
			current_effective_apm: details.current_effective_apm(),
		}
	}
}
