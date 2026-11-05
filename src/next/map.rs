//! Map analysis, done once at the start of the game.

use super::{
	build::{is_geyser_type, is_mineral_type},
	UnitData,
};
use crate::{
	action::Target,
	api::API,
	bot::Expansion,
	consts::{RaceValues, RACE_VALUES},
	distance::{Center, Distance},
	game_data::GameData,
	game_info::GameInfo,
	game_state::Alliance,
	geometry::Point2,
	player::Race,
	ramp::{Ramp, Ramps},
	utils::{dbscan, range_query},
	SC2Result,
};
use indexmap::IndexSet;
use itertools::iproduct;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use std::hash::BuildHasherDefault;

type FxIndexSet<T> = IndexSet<T, BuildHasherDefault<FxHasher>>;

/// Data that doesn't change during a game. Shared by every [`World`](super::World).
pub struct MapData {
	/// Your in-game id.
	pub player_id: u32,
	/// Opponent's in-game id.
	pub enemy_player_id: u32,
	/// Opponent id on ladder, filled in `--OpponentId`.
	pub opponent_id: String,
	/// Actual race of your bot.
	pub race: Race,
	/// Values which depend on your race.
	pub race_values: RaceValues,
	/// Information about the map.
	pub game_info: GameInfo,
	/// Constant information about abilities, unit types, upgrades, buffs and effects.
	pub game_data: GameData,
	/// Your starting location.
	pub start_location: Point2,
	/// Opponent's starting location.
	pub enemy_start: Point2,
	/// Center of the resources at your start location.
	pub start_center: Point2,
	/// Center of the resources at the opponent's start location.
	pub enemy_start_center: Point2,
	/// Ramps on the map.
	pub ramps: Ramps,
	/// Obstacles that block vision of ground units, but are pathable.
	pub vision_blockers: Vec<Point2>,
}

fn is_placeable(game_info: &GameInfo, pos: (usize, usize)) -> bool {
	game_info.placement_grid.get(pos).map_or(false, |p| p.is_empty())
}
fn is_pathable(game_info: &GameInfo, pos: (usize, usize)) -> bool {
	game_info.pathing_grid.get(pos).map_or(false, |p| p.is_empty())
}
fn get_height(game_info: &GameInfo, pos: (usize, usize)) -> u8 {
	game_info.terrain_height.get(pos).copied().unwrap_or(0)
}

/// Center of the resources around a location, counting the location itself.
fn resource_center(resources: &[&UnitData], location: Point2) -> Point2 {
	let near = resources
		.iter()
		.filter(|r| r.position.is_closer(11.0, location))
		.collect::<Vec<_>>();
	let sum = near.iter().fold(Point2::default(), |sum, r| sum + r.position);
	(sum + location) / (near.len() + 1) as f32
}

/// Reads the static map data from the first observation's units.
/// Returns the map data, the opponent's requested race and the expansions.
pub(super) fn analyze(
	api: &API,
	player_id: u32,
	opponent_id: String,
	game_info: GameInfo,
	game_data: GameData,
	units: &[UnitData],
) -> SC2Result<(MapData, Race, Vec<Expansion>)> {
	let players = &game_info.players;
	let race = players[&player_id]
		.race_actual
		.ok_or("SC2 didn't report the bot's race")?;
	let (enemy_player_id, enemy_race) = match players.len() {
		2 => (3 - player_id, players[&(3 - player_id)].race_requested),
		_ => (0, Race::Random),
	};

	let townhall = units.iter().find(|u| u.is_mine() && u.is_townhall());
	let start_location = townhall.map_or_else(Point2::default, |t| t.position);
	let enemy_start = game_info.start_locations.first().copied().unwrap_or_default();

	let resources = units
		.iter()
		.filter(|u| u.is_neutral() && (is_mineral_type(u.type_id) || is_geyser_type(u.type_id)))
		.collect::<Vec<_>>();
	let start_center = resource_center(&resources, start_location);
	let enemy_start_center = resource_center(&resources, enemy_start);

	// Expansions
	const RESOURCE_SPREAD: f32 = 72.25; // 8.5

	let all_resources = resources
		.iter()
		.filter(|r| r.type_id != crate::ids::UnitTypeId::MineralField450)
		.copied()
		.collect::<Vec<_>>();
	let by_tag = all_resources
		.iter()
		.map(|r| (r.tag, *r))
		.collect::<FxHashMap<_, _>>();
	let positions = all_resources
		.iter()
		.map(|r| (r.position, r.tag))
		.collect::<Vec<(Point2, u64)>>();

	let groups = dbscan(
		&positions,
		range_query(
			&positions,
			|(a, _), (b, _)| a.distance_squared(*b),
			RESOURCE_SPREAD,
		),
		1,
	)
	.0;

	const OFFSET: isize = 7;
	let offsets = iproduct!((-OFFSET..=OFFSET), (-OFFSET..=OFFSET))
		.filter(|(x, y)| {
			let d = x * x + y * y;
			16 < d && d <= 64
		})
		.collect::<Vec<(isize, isize)>>();

	let mut expansions = groups
		.into_iter()
		.map(|group| {
			let resources = group.iter().map(|(_, tag)| by_tag[tag]).collect::<Vec<_>>();
			let is_geyser = |r: &UnitData| is_geyser_type(r.type_id);
			let center = resources.iter().map(|r| r.position).center().unwrap().floor() + 0.5;

			let (loc, center, alliance, base) = if center.is_closer(4.0, start_center) {
				(
					start_location,
					start_center,
					Alliance::Own,
					townhall.map(|t| t.tag),
				)
			} else if center.is_closer(4.0, enemy_start_center) {
				(enemy_start, enemy_start_center, Alliance::Enemy, None)
			} else {
				let location = offsets
					.iter()
					.filter_map(|(x, y)| {
						let pos = center.offset(*x as f32, *y as f32);
						if !is_placeable(&game_info, (pos.x as usize, pos.y as usize)) {
							return None;
						}
						let mut distance_sum = 0_f32;
						let far_enough = resources.iter().all(|r| {
							let dist = pos.distance_squared(r.position);
							distance_sum += dist;
							dist >= if is_geyser(r) { 49.0 } else { 36.0 }
						});
						far_enough.then_some((pos, distance_sum))
					})
					.min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
					.ok_or("Can't detect right position for expansion")?
					.0;
				let sum = resources
					.iter()
					.fold(Point2::default(), |sum, r| sum + r.position);
				(
					location,
					(sum + location) / (resources.len() + 1) as f32,
					Alliance::Neutral,
					None,
				)
			};

			let mut minerals = FxIndexSet::default();
			let mut geysers = FxHashSet::default();
			for r in &resources {
				if is_geyser(r) {
					geysers.insert(r.tag);
				} else {
					minerals.insert(r.tag);
				}
			}
			minerals.sort_by(|a, b| {
				let dist = |t: &u64| by_tag[t].position.distance_squared(loc);
				dist(a).partial_cmp(&dist(b)).unwrap()
			});

			Ok(Expansion {
				loc,
				center,
				minerals,
				geysers,
				alliance,
				base,
			})
		})
		.collect::<SC2Result<Vec<_>>>()?;

	// Closest to the start location first
	let start = Target::Pos(start_location);
	let paths = api.query_pathing(expansions.iter().map(|exp| (start, exp.loc)).collect())?;
	let paths = expansions
		.iter()
		.zip(paths)
		.map(|(exp, path)| (exp.loc, path.unwrap_or(f32::INFINITY)))
		.collect::<FxHashMap<Point2, f32>>();
	expansions.sort_unstable_by(|a, b| paths[&a.loc].partial_cmp(&paths[&b.loc]).unwrap());

	// Ramps
	let mut vision_blockers = vec![];
	let mut ramp_points = FxHashSet::default();
	let around = |(x, y): (usize, usize)| {
		[
			(x + 1, y),
			(x - 1, y),
			(x, y + 1),
			(x, y - 1),
			(x + 1, y + 1),
			(x - 1, y - 1),
			(x + 1, y - 1),
			(x - 1, y + 1),
		]
	};

	let area = game_info.playable_area;
	for pos in iproduct!(area.x0..area.x1, area.y0..area.y1) {
		if !is_pathable(&game_info, pos) || is_placeable(&game_info, pos) {
			continue;
		}
		let height = get_height(&game_info, pos);
		if around(pos).iter().all(|p| get_height(&game_info, *p) == height) {
			vision_blockers.push(Point2::new(pos.0 as f32, pos.1 as f32));
		} else {
			ramp_points.insert(pos);
		}
	}

	let all_ramps = dbscan(
		&ramp_points,
		|&pos| {
			around(pos)
				.iter()
				.filter(|n| ramp_points.contains(n))
				.copied()
				.collect()
		},
		1,
	)
	.0
	.into_iter()
	.filter(|points| points.len() >= 8)
	.map(|points| Ramp::new(points, &game_info.terrain_height, start_location))
	.collect::<Vec<Ramp>>();

	let closest_ramp = |loc: Point2| {
		let (loc_x, loc_y) = <(usize, usize)>::from(loc);
		let cmp = |r: &&Ramp| {
			let (x, y) = r.top_center().unwrap();
			let dx = loc_x.abs_diff(x);
			let dy = loc_y.abs_diff(y);
			dx * dx + dy * dy
		};
		let with_upper = |a: usize, b: usize| {
			all_ramps
				.iter()
				.filter(|r| {
					let upper = r.upper().len();
					upper == a || upper == b
				})
				.min_by_key(cmp)
		};
		with_upper(2, 5).or_else(|| with_upper(4, 9)).cloned()
	};
	let mut ramps = Ramps::default();
	if let Some(ramp) = closest_ramp(start_location) {
		ramps.my = ramp;
	}
	if let Some(ramp) = closest_ramp(enemy_start) {
		ramps.enemy = ramp;
	}
	ramps.all = all_ramps;

	let map = MapData {
		player_id,
		enemy_player_id,
		opponent_id,
		race,
		race_values: RACE_VALUES[&race].clone(),
		game_info,
		game_data,
		start_location,
		enemy_start,
		start_center,
		enemy_start_center,
		ramps,
		vision_blockers,
	};
	Ok((map, enemy_race, expansions))
}
