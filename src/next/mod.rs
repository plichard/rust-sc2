//! Prototype of an alternative architecture. The existing API is untouched.
//!
//! Instead of one `Bot` that your struct derefs to, the library hands you a [`Context`]:
//!
//! - [`World`]: the game state of this step. Rebuilt every step and never mutated
//!   during your turn, so it needs no locks and is `Send + Sync` in every build.
//! - [`Commands`]: the only thing you write to during your turn.
//! - `self`: your bot, which holds nothing but your own state.
//!
//! Those are separate borrows, so this compiles without any clones:
//!
//! ```no_run
//! use rust_sc2::next::{Context, Player};
//! use rust_sc2::prelude::{PlayerSettings, Race, SC2Result, Target};
//! use std::collections::HashSet;
//!
//! #[derive(Default)]
//! struct MyBot {
//!     seen: HashSet<u64>,
//! }
//!
//! impl Player for MyBot {
//!     fn settings(&self) -> PlayerSettings<'_> {
//!         PlayerSettings::new(Race::Random)
//!     }
//!     fn on_step(&mut self, ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
//!         for w in ctx.world.my().workers() {
//!             self.seen.insert(w.tag);
//!             ctx.cmds.attack(w, Target::Pos(ctx.world.map.enemy_start), false);
//!         }
//!         Ok(())
//!     }
//! }
//! ```
//!
//! To remember a unit between steps, keep its tag and look it up again. Keeping the
//! view itself doesn't compile, because it can't outlive the step it belongs to:
//!
//! ```compile_fail
//! use rust_sc2::next::{Context, Player, UnitRef};
//! use rust_sc2::prelude::{PlayerSettings, Race, SC2Result};
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

mod commands;
mod runner;
mod unit;
mod units;
mod world;

pub use commands::Commands;
pub use runner::run_vs_computer;
pub use unit::{UnitData, UnitRef};
pub use units::{PlayerUnits, Units};
pub use world::{MapData, World};

use crate::{api::API, debug::Debugger, player::GameResult, Event, PlayerSettings, SC2Result};

/// Everything the library gives your bot during its turn.
///
/// The fields are public on purpose, so they can be borrowed independently.
pub struct Context {
	/// Game state of the current step.
	pub world: World,
	/// Orders to send when your turn ends.
	pub cmds: Commands,
	/// Debug drawing and cheats.
	pub debug: Debugger,
	/// Connection to SC2, for placement and pathing queries.
	pub api: API,
	first_events: Vec<Event>,
}

/// Trait that bots implement. `self` holds only your own state.
pub trait Player {
	/// Returns settings used to connect bot to the game.
	fn settings(&self) -> PlayerSettings<'_>;
	/// Called once on the first step.
	fn on_start(&mut self, _ctx: &mut Context) -> SC2Result<()> {
		Ok(())
	}
	/// Called on every game step.
	fn on_step(&mut self, _ctx: &mut Context, _iteration: usize) -> SC2Result<()> {
		Ok(())
	}
	/// Called when different events happen, before `on_start` or `on_step`.
	fn on_event(&mut self, _ctx: &mut Context, _event: Event) -> SC2Result<()> {
		Ok(())
	}
	/// Called once when the game ended.
	fn on_end(&mut self, _result: GameResult) -> SC2Result<()> {
		Ok(())
	}
}
