//! An alternative architecture, next to the existing API.
//!
//! Instead of one `Bot` that your struct derefs to, the library hands you a [`Context`]:
//!
//! - [`World`]: the game state of this step. Rebuilt every step, and you only ever get
//!   a shared reference to it, so it can't change during your turn.
//! - [`Commands`]: the orders you give during your turn.
//! - `self`: your bot, which holds nothing but your own state.
//!
//! Those are separate borrows, so this compiles without any clones:
//!
//! ```no_run
//! use rust_sc2::next::prelude::*;
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
//! use rust_sc2::next::prelude::*;
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
//! use rust_sc2::next::prelude::*;
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

mod build;
mod combat;
mod commands;
mod id_check;
mod map;
mod queries;
mod runner;
mod unit;
mod units;
mod world;

pub use combat::CalcTarget;
pub use commands::{Budget, Commands};
pub use id_check::IdMismatch;
pub use map::{Expansion, MapData};
pub use runner::{run_ladder_game, run_vs_computer, run_vs_human, LaunchOptions, RunnerMulti, RunnerSingle};
pub use unit::{UnitData, UnitRef, UnitTag};
pub use units::{PlayerUnits, Units};
pub use world::{CountOptions, World};

/// Everything a bot usually needs: `use rust_sc2::next::prelude::*;`.
pub mod prelude {
	pub use super::{
		run_ladder_game, run_vs_computer, run_vs_human, Budget, CalcTarget, Commands, Context, Event,
		Expansion, LaunchOptions, MapData, Player, PlayerUnits, RunnerMulti, RunnerSingle, UnitData, UnitRef,
		UnitTag, Units, World,
	};
	pub use crate::{
		action::Target,
		bot::PlacementOptions,
		client::SC2Result,
		consts::{ALL_PRODUCERS, PRODUCERS, RESEARCHERS, TECH_REQUIREMENTS},
		debug::Debugger,
		distance::{Center, Distance, DistanceIterator, DistanceSlice},
		game_state::Alliance,
		geometry::{Point2, Point3},
		ids::*,
		player::{AIBuild, Computer, Difficulty, GameResult, Race},
		PlayerSettings,
	};
}

use crate::{
	api::API,
	debug::Debugger,
	game_state::Alliance,
	player::{GameResult, Race},
	PlayerSettings, SC2Result,
};

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
