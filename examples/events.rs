//! How to use events.

use rust_sc2::prelude::*;

struct EmptyBot;

impl Player for EmptyBot {
	fn settings(&self) -> PlayerSettings<'_> {
		PlayerSettings::new(Race::Random)
	}

	fn on_event(&mut self, ctx: &mut Context, event: Event) -> SC2Result<()> {
		match event {
			Event::UnitDestroyed(_tag, alliance) => {
				match alliance {
					Some(Alliance::Own) => { /* your code here */ }
					Some(Alliance::Enemy) => { /* your code here */ }
					Some(Alliance::Neutral) => { /* your code here */ }
					// Allied units, and units that were never seen
					_ => { /* your code here */ }
				}
			}
			Event::UnitCreated(tag) => {
				if let Some(_u) = ctx.world.my().units().get(tag) { /* your code here */ }
			}
			Event::ConstructionStarted(tag) => {
				if let Some(_u) = ctx.world.my().structures().get(tag) { /* your code here */ }
			}
			Event::ConstructionComplete(tag) => {
				if let Some(_u) = ctx.world.my().structures().get(tag) { /* your code here */ }
			}
			Event::RandomRaceDetected(_race) => { /* your code here */ }
		}
		Ok(())
	}
}

fn main() -> SC2Result<()> {
	run_vs_computer(
		&mut EmptyBot,
		Computer::new(Race::Random, Difficulty::VeryEasy, None),
		"EverDreamLE",
		Default::default(),
	)
}
