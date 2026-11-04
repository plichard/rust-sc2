use super::{
	world::{build_world, MapData},
	Commands, Context, Player, World,
};
use crate::{
	api::API,
	bot::{Bot, Expansion},
	client::{
		connect_to_websocket, create_computer_setup, create_player_setup, get_unused_port, join_game,
		launch_client, save_replay, set_static_data, LaunchOptions, HOST,
	},
	consts::RACE_VALUES,
	debug::Debugger,
	game_state::update_state,
	ids::AbilityId,
	paths::{get_map_path, get_path_to_sc2},
	player::{Computer, Race},
	IntoProto, IntoSC2, PlayerSettings, SC2Result,
};
use num_traits::FromPrimitive;
use rustc_hash::{FxHashMap, FxHashSet};
use sc2_proto::{
	query::RequestQueryAvailableAbilities,
	raw::Alliance as ProtoAlliance,
	sc2api::{Request, Response, ResponseObservation, Status},
};
use std::{
	mem,
	ops::{Deref, DerefMut},
	process::Child,
	sync::Arc,
};

/// Runs a game against the built-in AI.
pub fn run_vs_computer<P: Player>(
	player: &mut P,
	computer: Computer,
	map_name: &str,
	options: LaunchOptions,
) -> SC2Result<()> {
	let sc2_path = get_path_to_sc2();
	let map_path = get_map_path(&sc2_path, map_name);

	let (port, process) = match options.port {
		Some(port) => (port, None),
		None => {
			let port = get_unused_port();
			(port, Some(launch_client(&sc2_path, port, options.sc2_version)))
		}
	};
	let mut session = Session {
		external: process.is_none(),
		process,
		api: Some(API::new(connect_to_websocket(HOST, port)?)),
	};

	let settings = player.settings();
	let player_id = create_game(session.api(), &settings, &computer, &map_path, options.realtime)?;

	let mut ctx = session.start(player_id)?;
	let result = play(player, &mut ctx, options.realtime, options.save_replay_as);
	session.api = Some(ctx.api);
	session.close();
	result
}

/// Owns the SC2 process and shuts it down however the game ends.
struct Session {
	process: Option<Child>,
	external: bool,
	api: Option<API>,
}

impl Session {
	fn api(&self) -> &API {
		self.api.as_ref().expect("API is in use")
	}

	/// Reads the static data and builds the first world.
	///
	/// Expansion and ramp detection is still written against the old `Bot`, so it
	/// runs once here and its results are moved out. Porting `Bot::prepare_start`
	/// to work on a `World` would remove the last dependency on `Bot`.
	fn start(&mut self, player_id: u32) -> SC2Result<Context> {
		let mut boot = Bootstrap::default();
		boot.external_client = true;
		boot.api = self.api.take();
		boot.player_id = player_id;

		let started = (|| -> SC2Result<(ResponseObservation, FxHashMap<_, _>)> {
			set_static_data(&mut boot)?;
			let mut req = Request::new();
			req.mut_observation().set_disable_fog(true);
			let mut res = boot.api().send(req)?;
			let observation = res.take_observation();
			boot.init_data_for_unit();
			update_state(&mut boot, &observation)?;
			boot.prepare_start();
			let abilities = query_abilities(boot.api(), &observation)?;
			Ok((observation, abilities))
		})();
		let api = boot.api.take();
		let (observation, abilities) = match started {
			Ok(started) => started,
			Err(e) => {
				self.api = api;
				return Err(e);
			}
		};
		let api = api.expect("API is in use");

		let race = boot.race;
		let enemy_race = boot.enemy_race;
		let expansions: Vec<Expansion> = mem::take(&mut boot.expansions);
		let map = MapData {
			player_id,
			enemy_player_id: boot.enemy_player_id,
			race,
			race_values: RACE_VALUES[&race].clone(),
			game_info: mem::take(&mut boot.game_info),
			game_data: (*boot.game_data).clone(),
			start_location: boot.start_location,
			enemy_start: boot.enemy_start,
			start_center: boot.start_center,
			enemy_start_center: boot.enemy_start_center,
			ramps: mem::take(&mut boot.ramps),
			vision_blockers: mem::take(&mut boot.vision_blockers),
		};
		drop(boot);

		let initial = World::initial(Arc::new(map), enemy_race, expansions);
		let (world, events) = build_world(&initial, &observation, abilities);
		Ok(Context {
			world,
			cmds: Commands::default(),
			debug: Debugger::default(),
			api,
			first_events: events,
		})
	}

	fn close(&mut self) {
		if let Some(api) = &self.api {
			let mut req = Request::new();
			req.mut_leave_game();
			if let Err(e) = api.send_request(req) {
				error!("Request LeaveGame failed: {}", e);
			}
			if !self.external {
				let mut req = Request::new();
				req.mut_quit();
				if let Err(e) = api.send_request(req) {
					error!("Request QuitGame failed: {}", e);
				}
			}
		}
		self.api = None;
		if let Some(process) = &mut self.process {
			if let Err(e) = process.kill() {
				error!("Can't kill SC2 process: {}", e);
			}
		}
		self.process = None;
	}
}
impl Drop for Session {
	fn drop(&mut self) {
		self.close();
	}
}

/// Just enough of the old bot to run the old map analysis.
#[derive(Default)]
struct Bootstrap(Bot);
impl Deref for Bootstrap {
	type Target = Bot;

	fn deref(&self) -> &Bot {
		&self.0
	}
}
impl DerefMut for Bootstrap {
	fn deref_mut(&mut self) -> &mut Bot {
		&mut self.0
	}
}
impl crate::Player for Bootstrap {
	fn get_player_settings(&self) -> PlayerSettings<'_> {
		PlayerSettings::new(Race::Random)
	}
}

fn create_game(
	api: &API,
	settings: &PlayerSettings,
	computer: &Computer,
	map_path: &str,
	realtime: bool,
) -> SC2Result<u32> {
	let mut req = Request::new();
	let create = req.mut_create_game();
	create.mut_local_map().set_map_path(map_path.to_string());
	create_player_setup(settings, create);
	create_computer_setup(computer, create);
	create.set_realtime(realtime);

	let res = api.send(req)?;
	let created = res.create_game();
	if created.has_error() {
		return Err(format!("{:?}: {}", created.error(), created.error_details()).into());
	}
	join_game(settings, api, None)
}

fn query_abilities(
	api: &API,
	observation: &ResponseObservation,
) -> SC2Result<FxHashMap<u64, FxHashSet<AbilityId>>> {
	let mut req = Request::new();
	let query = &mut req.mut_query().abilities;
	for u in &observation.observation.raw_data.units {
		if matches!(u.alliance(), ProtoAlliance::Self_) {
			let mut unit = RequestQueryAvailableAbilities::new();
			unit.unit_tag = u.tag;
			query.push(unit);
		}
	}
	let res: Response = api.send(req)?;
	Ok(res
		.query()
		.abilities
		.iter()
		.map(|a| {
			(
				a.unit_tag(),
				a.abilities
					.iter()
					.filter_map(|ab| AbilityId::from_i32(ab.ability_id()))
					.collect(),
			)
		})
		.collect())
}

fn play<P: Player>(
	player: &mut P,
	ctx: &mut Context,
	realtime: bool,
	save_replay_as: Option<&str>,
) -> SC2Result<()> {
	for event in mem::take(&mut ctx.first_events) {
		player.on_event(ctx, event)?;
	}
	player.on_start(ctx)?;
	end_turn(ctx, realtime)?;

	let mut iteration = 0;
	loop {
		let mut req = Request::new();
		req.mut_observation().set_disable_fog(false);
		let mut res = ctx.api.send(req)?;

		if matches!(res.status(), Status::ended) {
			let result = res.observation().player_result[ctx.world.map.player_id as usize - 1]
				.result()
				.into_sc2();
			player.on_end(result)?;
			break;
		}

		let observation = res.take_observation();
		let abilities = query_abilities(&ctx.api, &observation)?;
		let (world, events) = build_world(&ctx.world, &observation, abilities);
		ctx.world = world;

		for event in events {
			player.on_event(ctx, event)?;
		}
		player.on_step(ctx, iteration)?;
		if ctx.cmds.leave_game {
			break;
		}
		end_turn(ctx, realtime)?;
		iteration += 1;
	}

	if let Some(path) = save_replay_as {
		save_replay(&ctx.api, path)?;
	}
	Ok(())
}

/// Sends everything the bot queued, then asks SC2 to advance the game.
fn end_turn(ctx: &mut Context, realtime: bool) -> SC2Result<()> {
	let actions = ctx.cmds.take_actions();
	if !actions.is_empty() {
		let mut req = Request::new();
		let proto = &mut req.mut_action().actions;
		for action in &actions {
			proto.push(action.into_proto());
		}
		ctx.api.send_request(req)?;
	}

	let debug_commands = ctx.debug.get_commands();
	if !debug_commands.is_empty() {
		let mut req = Request::new();
		let proto = &mut req.mut_debug().debug;
		for command in debug_commands.iter() {
			proto.push(command.into_proto());
		}
		ctx.api.send_request(req)?;
	}
	ctx.debug.clear_commands();

	// The world doesn't change during a turn; these apply to the next one.
	if let Some(frames) = ctx.cmds.game_step.take() {
		ctx.world.game_step = frames;
	}
	ctx.world.enemy_upgrades.extend(ctx.cmds.enemy_upgrades.drain(..));

	if !realtime {
		let mut req = Request::new();
		req.mut_step().set_count(ctx.world.game_step);
		ctx.api.send_request(req)?;
	}
	Ok(())
}
