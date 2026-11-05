//! How to use runners.

use rust_sc2::prelude::*;

struct EmptyBot;

impl Player for EmptyBot {
	fn settings(&self) -> PlayerSettings<'_> {
		PlayerSettings::new(Race::Random)
	}
}

fn main() -> SC2Result<()> {
	let mut bot = EmptyBot;

	// Bot vs Computer
	// 1. Initialize runner
	let mut runner = RunnerSingle::new(
		Computer::new(Race::Random, Difficulty::VeryEasy, None),
		"EverDreamLE",
		Some("4.10"), // Client version can be specified, otherwise the latest available is used
	);

	// 2. Configure runner
	runner.set_map("EternalEmpireLE");
	runner.computer = Computer::new(Race::Protoss, Difficulty::VeryHard, Some(AIBuild::Air));
	runner.realtime = true; // Default: false
	runner.save_replay_as = Some("path/to/replay/MyReplay.SC2Replay"); // Default: None == don't save replay
	runner.game_step = 2; // Default: 1

	// 3. Launch SC2, or use runner.connect(port) for a client that's already running
	runner.launch()?;

	// 4. Run games
	// Run game once
	runner.run_game(&mut bot)?;

	const MAPS: &[&str] = &["EverDreamLE", "GoldenWallLE", "IceandChromeLE"];
	const RACES: &[Race] = &[Race::Zerg, Race::Terran, Race::Protoss];
	const DIFFICULTIES: &[Difficulty] = &[Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];

	// Run multiple times
	for i in 0..3 {
		// Configuration can be changed between games
		runner.set_map(MAPS[i]);
		runner.computer.race = RACES[i];
		runner.computer.difficulty = DIFFICULTIES[i];

		runner.run_game(&mut bot)?;
	}

	// Better to close runner manually before launching another
	runner.close();

	// Human vs Bot
	// 1. Initialize runner
	let mut runner = RunnerMulti::new(
		PlayerSettings::new(Race::Random).with_name("Name"),
		"PillarsofGoldLE",
		None,
	);

	// 2. Configure runner
	runner.set_map("PillarsofGoldLE");
	runner.human_settings = PlayerSettings::new(Race::Random).with_name("Name");
	runner.realtime = false;
	runner.save_replay_as = None;

	// 3. Launch SC2
	runner.launch()?;

	// 4. Run games
	runner.run_game(&mut bot)?;

	// Runners dropped here:
	// Any launched sc2 clients will be closed automatically

	Ok(())
}
