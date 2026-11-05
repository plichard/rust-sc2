//! Everything needed to run games in SC2.
//!
//! Contains runner structures for verbose configuration and multiple games,
//! and simple runner functions for playing once.

use crate::{
	api::API,
	build::build_world,
	debug::Debugger,
	game_data::GameData,
	game_info::GameInfo,
	id_check::{check_ids, report},
	ids::AbilityId,
	map::analyze,
	paths::*,
	pixel_map::VisibilityMap,
	player::Computer,
	Budget, Commands, Context, Event, FromProto, IntoProto, IntoSC2, Player, PlayerSettings, UnitData,
	UnitTag, World,
};
use num_traits::FromPrimitive;
use rustc_hash::{FxHashMap, FxHashSet};
use sc2_proto::{
	query::RequestQueryAvailableAbilities,
	raw::Alliance as ProtoAlliance,
	sc2api::{PlayerSetup, PlayerType, PortSet, Request, RequestCreateGame, ResponseObservation, Status},
};
use std::{
	error::Error,
	fmt,
	fs::File,
	io::Write,
	mem,
	net::{TcpListener, TcpStream},
	process::{Child, Command},
	sync::Arc,
};
use tungstenite::{connect, stream::MaybeTlsStream, WebSocket};

pub(crate) type WS = WebSocket<MaybeTlsStream<TcpStream>>;
/// Result used throughout the library. Can hold any error.
pub type SC2Result<T> = Result<T, Box<dyn Error>>;

#[cfg(all(feature = "wine_sc2", not(target_os = "linux")))]
compile_error!("Wine is only supported on linux");

const HOST: &str = "127.0.0.1";
const SC2_BINARY: &str = {
	#[cfg(any(target_os = "windows", feature = "wine_sc2"))]
	{
		#[cfg(target_arch = "x86_64")]
		{
			"SC2_x64.exe"
		}
		#[cfg(target_arch = "x86")]
		{
			"SC2.exe"
		}
		#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
		{
			compile_error!("Unsupported Arch");
		}
	}
	#[cfg(all(target_os = "linux", not(feature = "wine_sc2")))]
	{
		#[cfg(target_arch = "x86_64")]
		{
			"SC2_x64"
		}
		#[cfg(target_arch = "x86")]
		{
			"SC2"
		}
		#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
		{
			compile_error!("Unsupported Arch");
		}
	}
	#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
	{
		"SC2.app/Contents/MacOS/SC2"
	}

	#[cfg(not(any(
		target_os = "windows",
		target_os = "linux",
		all(target_os = "macos", target_arch = "aarch64")
	)))]
	{
		compile_error!("Unsupported OS");
	}
};

#[derive(Debug)]
struct ProtoError(String);
impl ProtoError {
	fn new<E: fmt::Debug>(error: E, details: &str) -> Self {
		Self(format!("{:?}: {}", error, details))
	}
}
impl fmt::Display for ProtoError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}
impl Error for ProtoError {}

struct Ports {
	// shared: i32,
	server: (i32, i32),
	client: Vec<(i32, i32)>,
}

/// Options for [`run_vs_computer`] and [`run_vs_human`].
#[derive(Default)]
pub struct LaunchOptions<'a> {
	/// SC2 version to play on, otherwise latest available will be used.
	pub sc2_version: Option<&'a str>,
	/// Save replay after the game in given path.
	pub save_replay_as: Option<&'a str>,
	/// Play in real time.
	pub realtime: bool,
	/// Port of an already running client to use instead of launching one (vs computer only).
	/// The client is left running after the game.
	pub port: Option<i32>,
	/// Frames between steps. [Default: `1`]
	pub game_step: Option<u32>,
}

/// One SC2 client: closes the game, and the client if we launched it, however things end.
#[derive(Default)]
struct Client {
	process: Option<Child>,
	external: bool,
	api: Option<API>,
}

impl Client {
	fn launch(sc2_path: &str, port: i32, sc2_version: Option<&str>) -> Self {
		Self {
			process: Some(launch_client(sc2_path, port, sc2_version)),
			external: false,
			api: None,
		}
	}
	fn connect(&mut self, host: &str, port: i32) -> SC2Result<()> {
		self.api = Some(API::new(connect_to_websocket(host, port)?));
		Ok(())
	}
	fn api(&self) -> SC2Result<&API> {
		self.api
			.as_ref()
			.ok_or_else(|| "SC2 client isn't connected".into())
	}
	fn close(&mut self) {
		if let Some(api) = self.api.take() {
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
		if let Some(mut process) = self.process.take() {
			if let Err(e) = process.kill() {
				error!("Can't kill SC2 process: {}", e);
			}
		}
	}
}
impl Drop for Client {
	fn drop(&mut self) {
		self.close();
	}
}

/// Runner for games vs built-in AI. Can play several games on one client.
pub struct RunnerSingle<'a> {
	client: Client,
	sc2_path: String,
	sc2_version: Option<&'a str>,
	map_path: String,
	/// Computer opponent configuration.
	pub computer: Computer,
	/// Play in real time.
	pub realtime: bool,
	/// Save replay after the game in given path.
	pub save_replay_as: Option<&'a str>,
	/// Frames between steps. [Default: `1`]
	pub game_step: u32,
}

impl<'a> RunnerSingle<'a> {
	/// Constructs new runner.
	pub fn new(computer: Computer, map: &str, sc2_version: Option<&'a str>) -> Self {
		let sc2_path = get_path_to_sc2();
		let map_path = get_map_path(&sc2_path, map);
		Self {
			client: Client::default(),
			sc2_path,
			sc2_version,
			map_path,
			computer,
			realtime: false,
			save_replay_as: None,
			game_step: 1,
		}
	}
	/// Launches an SC2 client and connects to it.
	pub fn launch(&mut self) -> SC2Result<()> {
		let port = get_unused_port();
		self.client = Client::launch(&self.sc2_path, port, self.sc2_version);
		self.client.connect(HOST, port)
	}
	/// Connects to an already running SC2 client, which is left running afterwards.
	pub fn connect(&mut self, port: i32) -> SC2Result<()> {
		self.client.external = true;
		self.client.connect(HOST, port)
	}
	/// Runs one game.
	pub fn run_game<P: Player>(&mut self, player: &mut P) -> SC2Result<()> {
		let settings = player.settings();
		let api = self.client.api()?;

		let mut req = Request::new();
		let create = req.mut_create_game();
		create.mut_local_map().set_map_path(self.map_path.clone());
		create_player_setup(&settings, create);
		create_computer_setup(&self.computer, create);
		create.set_realtime(self.realtime);
		check_game_created(api, req)?;

		let player_id = join_game(&settings, api, None)?;
		let game = Game {
			player_id,
			opponent_id: String::new(),
			realtime: self.realtime,
			save_replay_as: self.save_replay_as,
			game_step: self.game_step,
		};
		game.run(player, &mut self.client)
	}
	/// Changes the map to play on.
	///
	/// # Panics
	/// Panics if the map doesn't exist in the maps directory.
	pub fn set_map(&mut self, map: &str) {
		self.map_path = get_map_path(&self.sc2_path, map);
	}
	/// Leaves the game, and closes the client if it was launched by this runner.
	pub fn close(&mut self) {
		self.client.close();
	}
}

/// Runner for games vs a human, which launches two SC2 clients.
pub struct RunnerMulti<'a> {
	bot_client: Client,
	human_client: Client,
	sc2_path: String,
	sc2_version: Option<&'a str>,
	map_path: String,
	/// Configuration of the human opponent.
	pub human_settings: PlayerSettings<'a>,
	/// Play in real time.
	pub realtime: bool,
	/// Save replay after the game in given path.
	pub save_replay_as: Option<&'a str>,
	/// Frames between steps. [Default: `1`]
	pub game_step: u32,
}

impl<'a> RunnerMulti<'a> {
	/// Constructs new runner.
	pub fn new(human_settings: PlayerSettings<'a>, map: &str, sc2_version: Option<&'a str>) -> Self {
		let sc2_path = get_path_to_sc2();
		let map_path = get_map_path(&sc2_path, map);
		Self {
			bot_client: Client::default(),
			human_client: Client::default(),
			sc2_path,
			sc2_version,
			map_path,
			human_settings,
			realtime: false,
			save_replay_as: None,
			game_step: 1,
		}
	}
	/// Launches both SC2 clients and connects to them.
	pub fn launch(&mut self) -> SC2Result<()> {
		let ports = get_unused_ports(2);
		self.human_client = Client::launch(&self.sc2_path, ports[1], self.sc2_version);
		self.bot_client = Client::launch(&self.sc2_path, ports[0], self.sc2_version);
		self.human_client.connect(HOST, ports[1])?;
		self.bot_client.connect(HOST, ports[0])
	}
	/// Runs one game.
	pub fn run_game<P: Player>(&mut self, player: &mut P) -> SC2Result<()> {
		let bot_settings = player.settings();
		let human_api = self.human_client.api()?;
		let bot_api = self.bot_client.api()?;

		let mut req = Request::new();
		let create = req.mut_create_game();
		create.mut_local_map().set_map_path(self.map_path.clone());
		create_player_setup(&self.human_settings, create);
		create_player_setup(&bot_settings, create);
		create.set_realtime(self.realtime);
		check_game_created(human_api, req)?;

		let ports = get_unused_ports(6);
		let ports = Ports {
			server: (ports[0], ports[1]),
			client: vec![(ports[2], ports[3]), (ports[4], ports[5])],
		};
		join_game2(&self.human_settings, human_api, Some(&ports))?;
		join_game2(&bot_settings, bot_api, Some(&ports))?;
		wait_join(human_api)?;
		let player_id = wait_join(bot_api)?;

		let game = Game {
			player_id,
			opponent_id: String::new(),
			realtime: self.realtime,
			save_replay_as: self.save_replay_as,
			game_step: self.game_step,
		};
		game.run(player, &mut self.bot_client)
	}
	/// Changes the map to play on.
	///
	/// # Panics
	/// Panics if the map doesn't exist in the maps directory.
	pub fn set_map(&mut self, map: &str) {
		self.map_path = get_map_path(&self.sc2_path, map);
	}
	/// Closes both SC2 clients.
	pub fn close(&mut self) {
		self.bot_client.close();
		self.human_client.close();
	}
}

/// Runs a game against the built-in AI.
pub fn run_vs_computer<P: Player>(
	player: &mut P,
	computer: Computer,
	map_name: &str,
	options: LaunchOptions,
) -> SC2Result<()> {
	let mut runner = RunnerSingle::new(computer, map_name, options.sc2_version);
	match options.port {
		Some(port) => runner.connect(port)?,
		None => runner.launch()?,
	}
	runner.realtime = options.realtime;
	runner.save_replay_as = options.save_replay_as;
	runner.game_step = options.game_step.unwrap_or(1);
	runner.run_game(player)
}

/// Runs a game against a human.
pub fn run_vs_human<P: Player>(
	player: &mut P,
	human_settings: PlayerSettings,
	map_name: &str,
	options: LaunchOptions,
) -> SC2Result<()> {
	let mut runner = RunnerMulti::new(human_settings, map_name, options.sc2_version);
	runner.launch()?;
	runner.realtime = options.realtime;
	runner.save_replay_as = options.save_replay_as;
	runner.game_step = options.game_step.unwrap_or(1);
	runner.run_game(player)
}

/// Joins a ladder game.
pub fn run_ladder_game<P: Player>(
	player: &mut P,
	host: &str,
	port: i32,
	player_port: i32,
	opponent_id: Option<&str>,
	game_step: Option<u32>,
) -> SC2Result<()> {
	let mut client = Client::default();
	client.connect(host, port)?;
	let player_id = join_game(
		&player.settings(),
		client.api()?,
		Some(&Ports {
			server: (player_port + 2, player_port + 3),
			client: vec![(player_port + 4, player_port + 5)],
		}),
	)?;
	let game = Game {
		player_id,
		opponent_id: opponent_id.unwrap_or_default().to_string(),
		realtime: false,
		save_replay_as: None,
		game_step: game_step.unwrap_or(1),
	};
	game.run(player, &mut client)
}

fn check_game_created(api: &API, req: Request) -> SC2Result<()> {
	let res = api.send(req)?;
	let created = res.create_game();
	if created.has_error() {
		return Err(format!("{:?}: {}", created.error(), created.error_details()).into());
	}
	Ok(())
}

fn query_abilities(
	api: &API,
	observation: &ResponseObservation,
) -> SC2Result<FxHashMap<UnitTag, FxHashSet<AbilityId>>> {
	let mut req = Request::new();
	let query = &mut req.mut_query().abilities;
	for u in &observation.observation.raw_data.units {
		if matches!(u.alliance(), ProtoAlliance::Self_) {
			let mut unit = RequestQueryAvailableAbilities::new();
			unit.unit_tag = u.tag;
			query.push(unit);
		}
	}
	let res = api.send(req)?;
	Ok(res
		.query()
		.abilities
		.iter()
		.map(|a| {
			(
				UnitTag(a.unit_tag()),
				a.abilities
					.iter()
					.filter_map(|ab| AbilityId::from_i32(ab.ability_id()))
					.collect(),
			)
		})
		.collect())
}

/// A game that has been joined and is ready to play.
struct Game<'a> {
	player_id: u32,
	opponent_id: String,
	realtime: bool,
	save_replay_as: Option<&'a str>,
	game_step: u32,
}

/// What a bot's turns share. Bots get a [`Context`] with shared access to the world.
struct Turn {
	world: World,
	cmds: Commands,
	budget: Budget,
	debug: Debugger,
	api: API,
}

impl Turn {
	fn context(&mut self) -> Context<'_> {
		Context {
			world: &self.world,
			cmds: &mut self.cmds,
			budget: &mut self.budget,
			debug: &mut self.debug,
			api: &self.api,
		}
	}
}

impl Game<'_> {
	fn run<P: Player>(self, player: &mut P, client: &mut Client) -> SC2Result<()> {
		let api = client.api.take().ok_or("SC2 client isn't connected")?;
		let (world, events) = match self.start(&api) {
			Ok(started) => started,
			Err(e) => {
				client.api = Some(api);
				return Err(e);
			}
		};
		let mut turn = Turn {
			budget: Budget::new(&world),
			world,
			cmds: Commands::default(),
			debug: Debugger::default(),
			api,
		};
		let result = self.play(player, &mut turn, events);
		client.api = Some(turn.api);
		result
	}

	/// Reads the static data and builds the first world.
	fn start(&self, api: &API) -> SC2Result<(World, Vec<Event>)> {
		let mut req = Request::new();
		req.mut_game_info();
		let game_info: GameInfo = api.send(req)?.take_game_info().into_sc2();

		let mut req = Request::new();
		let data = req.mut_data();
		data.set_ability_id(true);
		data.set_unit_type_id(true);
		data.set_upgrade_id(true);
		data.set_buff_id(true);
		data.set_effect_id(true);
		let mut res = api.send(req)?;
		let id_mismatches = check_ids(res.data());
		report(&id_mismatches);
		let game_data: GameData = res.take_data().into_sc2();

		// SC2 reports every resource on the map even under the fog of
		// war, so a normal observation is all that map analysis needs.
		let mut req = Request::new();
		req.mut_observation();
		let observation = api.send(req)?.take_observation();
		let raw = &observation.observation.raw_data;
		let visibility = VisibilityMap::from_proto(raw.map_state.visibility.get_or_default());
		let units = raw
			.units
			.iter()
			.filter_map(|u| UnitData::from_proto(&visibility, u))
			.collect::<Vec<_>>();
		let (map, enemy_race, expansions) = analyze(
			api,
			self.player_id,
			self.opponent_id.clone(),
			game_info,
			game_data,
			id_mismatches,
			&units,
		)?;

		let abilities = query_abilities(api, &observation)?;
		let mut initial = World::initial(Arc::new(map), enemy_race, expansions);
		initial.game_step = self.game_step;
		Ok(build_world(&initial, &observation, abilities))
	}

	fn play<P: Player>(&self, player: &mut P, turn: &mut Turn, events: Vec<Event>) -> SC2Result<()> {
		for event in events {
			player.on_event(&mut turn.context(), event)?;
		}
		player.on_start(&mut turn.context())?;
		self.end_turn(turn)?;

		let mut iteration = 0;
		loop {
			let mut req = Request::new();
			req.mut_observation();
			let mut res = turn.api.send(req)?;

			if matches!(res.status(), Status::ended) {
				let result = res.observation().player_result[self.player_id as usize - 1]
					.result()
					.into_sc2();
				player.on_end(result)?;
				break;
			}

			let observation = res.take_observation();
			let abilities = query_abilities(&turn.api, &observation)?;
			let (world, events) = build_world(&turn.world, &observation, abilities);
			turn.budget = Budget::new(&world);
			turn.world = world;

			for event in events {
				player.on_event(&mut turn.context(), event)?;
			}
			player.on_step(&mut turn.context(), iteration)?;
			if turn.cmds.leave_game {
				let mut req = Request::new();
				req.mut_leave_game();
				turn.api.send_request(req)?;
				break;
			}
			self.end_turn(turn)?;
			iteration += 1;
		}

		if let Some(path) = self.save_replay_as {
			save_replay(&turn.api, path)?;
		}
		Ok(())
	}

	/// Sends everything the bot queued, then asks SC2 to advance the game.
	fn end_turn(&self, turn: &mut Turn) -> SC2Result<()> {
		let actions = turn.cmds.take_actions();
		if !actions.is_empty() {
			let mut req = Request::new();
			let proto = &mut req.mut_action().actions;
			for action in &actions {
				proto.push(action.into_proto());
			}
			turn.api.send_request(req)?;
		}

		let debug_commands = turn.debug.get_commands();
		if !debug_commands.is_empty() {
			let mut req = Request::new();
			let proto = &mut req.mut_debug().debug;
			for command in debug_commands.iter() {
				proto.push(command.into_proto());
			}
			turn.api.send_request(req)?;
		}
		turn.debug.clear_commands();

		// The world doesn't change during a turn; these apply to the next one.
		if let Some(frames) = turn.cmds.game_step.take() {
			turn.world.game_step = frames;
		}
		turn.world
			.enemy_upgrades
			.extend(turn.cmds.enemy_upgrades.drain(..));
		let game_loop = turn.world.game_loop;
		for (tag, frames) in mem::take(&mut turn.cmds.sleeping) {
			turn.world.available_frames.insert(tag, game_loop + frames);
		}

		if !self.realtime {
			let mut req = Request::new();
			req.mut_step().set_count(turn.world.game_step);
			turn.api.send_request(req)?;
		}
		Ok(())
	}
}

fn get_unused_port() -> i32 {
	(5000..65535)
		.find(|port| TcpListener::bind((HOST, *port)).is_ok())
		.unwrap() as i32
}

fn get_unused_ports(n: usize) -> Vec<i32> {
	let mut ports = Vec::with_capacity(n);
	for port in 5000..65535 {
		if TcpListener::bind((HOST, port)).is_ok() {
			ports.push(port as i32);
			if ports.len() >= n {
				break;
			}
		}
	}
	ports
}

// Helpers
fn create_player_setup(settings: &PlayerSettings, req_create_game: &mut RequestCreateGame) {
	let mut setup = PlayerSetup::new();

	setup.set_race(settings.race.into_proto());
	setup.set_type(PlayerType::Participant);
	if let Some(name) = &settings.name {
		setup.set_player_name(name.to_string());
	}
	req_create_game.player_setup.push(setup);
}

fn create_computer_setup(computer: &Computer, req_create_game: &mut RequestCreateGame) {
	let mut setup = PlayerSetup::new();

	setup.set_race(computer.race.into_proto());
	setup.set_type(PlayerType::Computer);
	setup.set_difficulty(computer.difficulty.into_proto());
	if let Some(ai_build) = computer.ai_build {
		setup.set_ai_build(ai_build.into_proto());
	}
	req_create_game.player_setup.push(setup);
}

fn join_game(settings: &PlayerSettings, api: &API, ports: Option<&Ports>) -> SC2Result<u32> {
	join_game2(settings, api, ports)?;
	wait_join(api)
}
fn join_game2(settings: &PlayerSettings, api: &API, ports: Option<&Ports>) -> SC2Result<()> {
	let mut req = Request::new();
	let req_join_game = req.mut_join_game();

	req_join_game.set_race(settings.race.into_proto());

	let options = req_join_game.options.mut_or_insert_default();
	options.set_raw(true);
	options.set_score(true);
	// options.mut_feature_layer()
	// options.mut_render();
	options.set_show_cloaked(true);
	options.set_show_burrowed_shadows(true);
	options.set_show_placeholders(true);
	options.set_raw_affects_selection(settings.raw_affects_selection);
	options.set_raw_crop_to_playable_area(settings.raw_crop_to_playable_area);
	if let Some(name) = &settings.name {
		req_join_game.set_player_name(name.to_string());
	}

	if let Some(ports) = ports {
		// req_join_game.set_shared_port(ports.shared);

		let server_ports = req_join_game.server_ports.mut_or_insert_default();
		server_ports.set_game_port(ports.server.0);
		server_ports.set_base_port(ports.server.1);

		let client_ports = &mut req_join_game.client_ports;
		for client in &ports.client {
			let mut port_set = PortSet::new();
			port_set.set_game_port(client.0);
			port_set.set_base_port(client.1);
			client_ports.push(port_set);
		}
	}

	api.send_only(req)?;
	Ok(())
}
fn wait_join(api: &API) -> SC2Result<u32> {
	let res = api.wait_response()?;

	let res_join_game = res.join_game();
	if res_join_game.has_error() {
		let err = ProtoError::new(res_join_game.error(), res_join_game.error_details());
		error!("{}", err);
		Err(Box::new(err))
	} else {
		Ok(res_join_game.player_id())
	}
}

fn save_replay(api: &API, path: &str) -> SC2Result<()> {
	let mut req = Request::new();
	req.mut_save_replay();

	let res = api.send(req)?;

	let mut path = path.to_string();
	if !path.ends_with(".SC2Replay") {
		path.push_str(".SC2Replay");
	}
	let mut file = File::create(path)?;
	file.write_all(res.save_replay().data())?;
	Ok(())
}

fn launch_client(sc2_path: &str, port: i32, sc2_version: Option<&str>) -> Child {
	let (base_version, data_hash) = match sc2_version {
		Some(ver) => get_version_info(ver),
		None => (get_latest_base_version(sc2_path), ""),
	};
	let sc2_full_path = format!("{}/Versions/Base{}/{}", sc2_path, base_version, SC2_BINARY);

	let mut process = if cfg!(feature = "wine_sc2") {
		let wine = std::env::var("WINE").unwrap_or_else(|_| "wine".to_string());
		let mut command = Command::new(wine);
		command.arg(sc2_full_path);
		command
	} else {
		Command::new(sc2_full_path)
	};
	let cwd = {
		#[cfg(any(target_os = "windows", feature = "wine_sc2"))]
		{
			#[cfg(target_arch = "x86_64")]
			{
				format!("{}/Support64", sc2_path)
			}
			#[cfg(target_arch = "x86")]
			{
				format!("{}/Support", sc2_path)
			}
		}
		#[cfg(any(target_os = "macos", all(target_os = "linux", not(feature = "wine_sc2"))))]
		{
			sc2_path
		}
	};
	process
		.current_dir(cwd)
		.arg("-listen")
		.arg(HOST)
		.arg("-port")
		.arg(port.to_string())
		// 0 - windowed, 1 - fullscreen
		.arg("-displayMode")
		.arg("0");
	if !data_hash.is_empty() {
		process.arg("-dataVersion").arg(data_hash);
	}
	process.spawn().expect("Can't launch SC2 process.")
}

fn connect_to_websocket(host: &str, port: i32) -> SC2Result<WS> {
	let url = format!("ws://{}:{}/sc2api", host, port);
	let (ws, _rs) = loop {
		if let Ok(result) = connect(&url) {
			break result;
		}
	};
	Ok(ws)
}
