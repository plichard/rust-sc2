//! Rust implementation of the StarCraft II API.
//!
//! Every callback of your bot gets a [`Context`]:
//!
//! - [`World`]: the game state of this step. Rebuilt every step, and you only ever get
//!   a shared reference to it, so it can't change during your turn.
//! - [`Commands`]: the orders you give during your turn.
//! - `self`: your bot, which holds nothing but your own state.
//!
//! Those are separate borrows, so this compiles without any clones:
//!
//! ```no_run
//! use rust_sc2::prelude::*;
//! use std::collections::HashSet;
//!
//! #[derive(Default)]
//! struct MyBot {
//!     seen: HashSet<UnitTag>,
//! }
//!
//! impl Player for MyBot {
//!     fn settings(&self) -> PlayerSettings<'_> {
//!         PlayerSettings::new(Race::Random)
//!     }
//!     fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
//!         for w in ctx.world.my().workers() {
//!             self.seen.insert(w.tag);
//!             ctx.cmds.attack(w, ctx.world.map.enemy_start, false);
//!         }
//!         Ok(())
//!     }
//! }
//! ```
//!
//! To remember a unit between steps, keep its [`UnitTag`] and look it up again. Keeping the
//! view itself doesn't compile, because it can't outlive the step it belongs to:
//!
//! ```compile_fail
//! use rust_sc2::prelude::*;
//!
//! struct MyBot<'a> {
//!     scout: Option<UnitRef<'a>>,
//! }
//!
//! impl<'a> Player for MyBot<'a> {
//!     fn settings(&self) -> PlayerSettings<'_> {
//!         PlayerSettings::new(Race::Random)
//!     }
//!     fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
//!         self.scout = ctx.world.my().workers().first();
//!         Ok(())
//!     }
//! }
//! ```
//!
//! The world can't be changed during your turn either:
//!
//! ```compile_fail
//! use rust_sc2::prelude::*;
//!
//! struct MyBot;
//!
//! impl Player for MyBot {
//!     fn settings(&self) -> PlayerSettings<'_> {
//!         PlayerSettings::new(Race::Random)
//!     }
//!     fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
//!         ctx.world.minerals = 10_000;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! See `examples/` for complete bots, and the README for how to run them.

#![deny(rustdoc::broken_intra_doc_links)]
#![allow(clippy::upper_case_acronyms)]

#[macro_use]
extern crate num_derive;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate sc2_macro;
#[macro_use]
extern crate maplit;
#[macro_use]
extern crate log;

/// Everything a bot usually needs: `use rust_sc2::prelude::*;`.
pub mod prelude {
	pub use crate::{
		action::Target,
		client::{
			run_ladder_game, run_vs_computer, run_vs_human, LaunchOptions, RunnerMulti, RunnerSingle, SC2Result,
		},
		consts::{ALL_PRODUCERS, PRODUCERS, RESEARCHERS, TECH_REQUIREMENTS},
		debug::Debugger,
		distance::{Center, Distance, DistanceIterator, DistanceSlice},
		game_state::Alliance,
		geometry::{Point2, Point3},
		ids::*,
		player::{AIBuild, Computer, Difficulty, GameResult, Race},
		queries::PlacementOptions,
		Budget, CalcTarget, Commands, Context, Event, Expansion, MapData, Player, PlayerSettings, PlayerUnits,
		UnitData, UnitRef, UnitTag, Units, World,
	};
}

mod build;
mod combat;
mod commands;
mod id_check;
mod map;
mod paths;
mod queries;
mod world;

pub mod action;
pub mod api;
pub mod client;
pub mod consts;
pub mod debug;
pub mod distance;
pub mod game_data;
pub mod game_info;
pub mod game_state;
pub mod geometry;
pub mod ids;
pub mod pixel_map;
pub mod player;
pub mod ramp;
pub mod score;
pub mod unit;
pub mod units;
pub mod utils;

pub use combat::CalcTarget;
pub use commands::{Budget, Commands};
pub use id_check::IdMismatch;
pub use map::{Expansion, MapData};
pub use queries::PlacementOptions;
pub use unit::{UnitData, UnitRef, UnitTag};
pub use units::{PlayerUnits, Units};
pub use world::{CountOptions, World};

#[doc(inline)]
pub use client::SC2Result;
/// Request to the SC2 API, sent with [`ctx.api`](Context::api) for anything the library doesn't cover.
pub use sc2_proto::sc2api::Request;

use crate::{api::API, debug::Debugger, game_state::Alliance, player::{GameResult, Race}};

/// Everything the library gives your bot during its turn.
///
/// The fields can be borrowed independently, and units you get from the
/// world aren't tied to this struct, so you can keep using it while holding them.
pub struct Context<'a> {
	/// Game state of the current step.
	pub world: &'a World,
	/// Orders to send when your turn ends.
	pub cmds: &'a mut Commands,
	/// Resources left to spend this turn.
	pub budget: &'a mut Budget,
	/// Debug drawing and cheats.
	pub debug: &'a mut Debugger,
	/// Connection to SC2, for placement and pathing queries.
	pub api: &'a API,
}

/// Things that happen during a game.
#[derive(Debug, Clone, Copy)]
pub enum Event {
	/// Unit died or structure was destroyed. The alliance is `None` for a unit that was never seen.
	UnitDestroyed(UnitTag, Option<Alliance>),
	/// Unit finished training (yours only).
	UnitCreated(UnitTag),
	/// Worker started to build a structure (yours only).
	ConstructionStarted(UnitTag),
	/// Construction of a structure finished (yours only).
	ConstructionComplete(UnitTag),
	/// Detected the actual race of a random opponent.
	RandomRaceDetected(Race),
}

/// Trait that bots implement. `self` holds only your own state.
pub trait Player {
	/// Returns settings used to connect bot to the game.
	fn settings(&self) -> PlayerSettings<'_>;
	/// Called once on the first step.
	fn on_start(&mut self, _ctx: &mut Context<'_>) -> SC2Result<()> {
		Ok(())
	}
	/// Called on every game step.
	fn on_step(&mut self, _ctx: &mut Context<'_>, _iteration: usize) -> SC2Result<()> {
		Ok(())
	}
	/// Called when different events happen, before `on_start` or `on_step`.
	fn on_event(&mut self, _ctx: &mut Context<'_>, _event: Event) -> SC2Result<()> {
		Ok(())
	}
	/// Called once when the game ended.
	fn on_end(&mut self, _result: GameResult) -> SC2Result<()> {
		Ok(())
	}
}

/// Settings that must be provided by a player when joining a game.
///
/// if name is `None`, it'll be shown as "foo(whatever)" in game.
///
/// if `raw_affects_selection` is `true`, bot will select units to which it gives orders.
///
/// if `raw_crop_to_playable_area` is `true`, maps will be crooped to the size of
/// [`playable_area`](game_info::GameInfo::playable_area).
///
/// Defaults:
/// `name`: `None`
/// `raw_affects_selection`: `false`
/// `raw_crop_to_playable_area`: `false`
pub struct PlayerSettings<'a> {
	pub race: Race,
	pub name: Option<&'a str>,
	pub raw_affects_selection: bool,
	pub raw_crop_to_playable_area: bool,
}
impl<'a> PlayerSettings<'a> {
	/// Constructs new settings with given `Race`.
	pub fn new(race: Race) -> Self {
		Self {
			race,
			name: None,
			raw_affects_selection: false,
			raw_crop_to_playable_area: false,
		}
	}
	/// Sets name of the player.
	pub fn with_name(mut self, name: &'a str) -> Self {
		self.name = Some(name);
		self
	}
	/// Sets `raw_affects_selection` to a given value.
	pub fn raw_affects_selection(mut self, val: bool) -> Self {
		self.raw_affects_selection = val;
		self
	}
	/// Sets `raw_crop_to_playable_area` to a given value.
	pub fn raw_crop_to_playable_area(mut self, val: bool) -> Self {
		self.raw_crop_to_playable_area = val;
		self
	}
}
impl Default for PlayerSettings<'_> {
	fn default() -> Self {
		Self {
			race: Race::Random,
			name: None,
			raw_affects_selection: false,
			raw_crop_to_playable_area: false,
		}
	}
}

trait FromProto<T>
where
	Self: Sized,
{
	fn from_proto(p: T) -> Self;
}

trait IntoSC2<T> {
	fn into_sc2(self) -> T;
}
impl<T, U: FromProto<T>> IntoSC2<U> for T {
	fn into_sc2(self) -> U {
		U::from_proto(self)
	}
}

trait TryFromProto<T>
where
	Self: Sized,
{
	fn try_from_proto(p: T) -> Option<Self>;
}

trait IntoProto<T> {
	fn into_proto(self) -> T;
}

pub trait IntoProtoField<T> {
	fn into_proto_field(self) -> ::protobuf::MessageField<T>;
}

impl<T, U> IntoProtoField<U> for T
where
	T: IntoProto<U>,
{
	fn into_proto_field(self) -> ::protobuf::MessageField<U> {
		::protobuf::MessageField::<U>(Some(Box::new(self.into_proto())))
	}
}

/*trait FromSC2<T> {
	fn from_sc2(s: T) -> Self;
}
impl<T, U: IntoProto<T>> FromSC2<U> for T {
	fn from_sc2(s: U) -> T {
		s.into_proto()
	}
}*/
