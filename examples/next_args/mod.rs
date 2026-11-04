//! Shared command line for the `next-*` examples.

use clap::Parser;
use rust_sc2::next::{run_vs_computer, Player};
use rust_sc2::prelude::*;

#[derive(Parser)]
struct Args {
	/// Map name, relative to the "StarCraft II/Maps" directory.
	#[clap(long)]
	map: String,
	/// Connect to a running SC2 client on this port instead of launching one.
	#[clap(long)]
	port: Option<i32>,
	/// Play in real time.
	#[clap(long)]
	realtime: bool,
	/// Save a replay to this path.
	#[clap(long)]
	save_replay: Option<String>,
}

pub fn run(mut bot: impl Player) -> SC2Result<()> {
	let args = Args::parse();
	run_vs_computer(
		&mut bot,
		Computer::new(Race::Random, Difficulty::VeryEasy, None),
		&args.map,
		LaunchOptions {
			port: args.port,
			realtime: args.realtime,
			save_replay_as: args.save_replay.as_deref(),
			..Default::default()
		},
	)
}
