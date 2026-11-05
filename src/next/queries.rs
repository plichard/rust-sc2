//! Questions SC2 itself has to answer, such as whether a building fits somewhere.

use super::{UnitRef, World};
use crate::{
	action::{ActionResult, Target},
	api::API,
	bot::{Expansion, PlacementOptions},
	distance::Distance,
	geometry::Point2,
	ids::{AbilityId, UnitTypeId},
	FromProto, IntoProto, SC2Result,
};
use num_traits::ToPrimitive;
use rand::prelude::*;
use sc2_proto::{
	query::{RequestQueryBuildingPlacement, RequestQueryPathing},
	sc2api::Request,
};

impl API {
	/// Sends pathing requests. Takes (start, goal) pairs, where start is a position or a unit tag.
	/// Returns the path length for each pair, or `None` if there's no path.
	pub fn query_pathing(&self, paths: Vec<(Target, Point2)>) -> SC2Result<Vec<Option<f32>>> {
		let mut req = Request::new();
		let req_pathing = &mut req.mut_query().pathing;
		for (start, goal) in paths {
			let mut pathing = RequestQueryPathing::new();
			match start {
				Target::Tag(tag) => pathing.set_unit_tag(tag),
				Target::Pos(pos) => pathing.set_start_pos(pos.into_proto()),
				Target::None => return Err("start of a pathing query must be a position or a tag".into()),
			}
			pathing.end_pos.0 = Some(Box::new(goal.into_proto()));
			req_pathing.push(pathing);
		}
		let res = self.send(req)?;
		Ok(res.query().pathing.iter().map(|result| result.distance).collect())
	}

	/// Sends placement requests. Takes (build ability, position, tag of builder or `None`) triples.
	/// Takes creep, psionic matrix and obstacles into account.
	pub fn query_placement(
		&self,
		places: Vec<(AbilityId, Point2, Option<u64>)>,
		check_resources: bool,
	) -> SC2Result<Vec<ActionResult>> {
		let mut req = Request::new();
		let req_query = req.mut_query();
		req_query.set_ignore_resource_requirements(!check_resources);
		let req_placement = &mut req_query.placements;
		for (ability, pos, builder) in places {
			let mut placement = RequestQueryBuildingPlacement::new();
			placement.set_ability_id(ability.to_i32().unwrap());
			*placement.target_pos.mut_or_insert_default() = pos.into_proto();
			if let Some(tag) = builder {
				placement.set_placing_unit_tag(tag);
			}
			req_placement.push(placement);
		}
		let res = self.send(req)?;
		Ok(res
			.query()
			.placements
			.iter()
			.map(|result| ActionResult::from_proto(result.result()))
			.collect())
	}

	/// Checks if it's possible to build given building on given position.
	pub fn can_place(&self, world: &World, building: UnitTypeId, pos: Point2) -> SC2Result<bool> {
		Ok(self.can_place_some(world, vec![(building, pos)])?[0])
	}

	/// Checks if it's possible to build each of given buildings on its position.
	pub fn can_place_some(&self, world: &World, places: Vec<(UnitTypeId, Point2)>) -> SC2Result<Vec<bool>> {
		let units = &world.map.game_data.units;
		let places = places
			.into_iter()
			.map(|(building, pos)| {
				let ability = units.get(&building).and_then(|d| d.ability);
				let ability = ability.ok_or_else(|| format!("{:?} can't be built", building))?;
				Ok((ability, pos, None))
			})
			.collect::<SC2Result<Vec<_>>>()?;
		Ok(self
			.query_placement(places, false)?
			.into_iter()
			.map(|r| r == ActionResult::Success)
			.collect())
	}

	/// Finds a position near `near` where given building can be placed.
	pub fn find_placement(
		&self,
		world: &World,
		building: UnitTypeId,
		near: Point2,
		options: PlacementOptions,
	) -> SC2Result<Option<Point2>> {
		let ability = match world.map.game_data.units.get(&building).and_then(|d| d.ability) {
			Some(ability) => ability,
			None => return Ok(None),
		};
		let addon_pos = |pos: Point2| (AbilityId::TerranBuildSupplyDepot, pos.offset(2.5, -0.5), None);
		let success = |r: &ActionResult| *r == ActionResult::Success;

		let mut first = vec![(ability, near, None)];
		if options.addon {
			first.push(addon_pos(near));
		}
		if self.query_placement(first, false)?.iter().all(success) {
			return Ok(Some(near));
		}

		let step = options.step;
		for distance in (step..options.max_distance).step_by(step as usize) {
			let positions = (-distance..=distance)
				.step_by(step as usize)
				.flat_map(|offset| {
					let (offset, distance) = (offset as f32, distance as f32);
					[
						near.offset(offset, -distance),
						near.offset(offset, distance),
						near.offset(-distance, offset),
						near.offset(distance, offset),
					]
				})
				.collect::<Vec<Point2>>();
			let results =
				self.query_placement(positions.iter().map(|p| (ability, *p, None)).collect(), false)?;
			let mut valid = positions
				.into_iter()
				.zip(results)
				.filter(|(_, r)| success(r))
				.map(|(p, _)| p)
				.collect::<Vec<_>>();

			if options.addon {
				let results = self.query_placement(valid.iter().map(|p| addon_pos(*p)).collect(), false)?;
				valid = valid
					.into_iter()
					.zip(results)
					.filter(|(_, r)| success(r))
					.map(|(p, _)| p)
					.collect();
			}

			if !valid.is_empty() {
				return Ok(if options.random {
					valid.choose(&mut thread_rng()).copied()
				} else {
					valid
						.into_iter()
						.min_by(|a, b| a.distance_squared(near).total_cmp(&b.distance_squared(near)))
				});
			}
		}
		Ok(None)
	}

	/// Finds a free geyser near given base.
	pub fn find_gas_placement<'w>(&self, world: &'w World, base: Point2) -> SC2Result<Option<UnitRef<'w>>> {
		let map = &world.map;
		let ability = match map
			.game_data
			.units
			.get(&map.race_values.gas)
			.and_then(|d| d.ability)
		{
			Some(ability) => ability,
			None => return Ok(None),
		};
		let geysers = world.vespene_geysers().closer(11.0, base);
		let results = self.query_placement(
			geysers.iter().map(|u| (ability, u.position, None)).collect(),
			false,
		)?;
		Ok(geysers
			.iter()
			.zip(results)
			.find(|(_, r)| *r == ActionResult::Success)
			.map(|(geyser, _)| geyser))
	}

	/// The free expansion with the shortest path from the opponent's start location.
	pub fn get_enemy_expansion<'w>(&self, world: &'w World) -> SC2Result<Option<&'w Expansion>> {
		let expansions = world.free_expansions().collect::<Vec<_>>();
		let start = Target::Pos(world.map.enemy_start);
		let paths = self.query_pathing(expansions.iter().map(|exp| (start, exp.loc)).collect())?;
		Ok(expansions
			.into_iter()
			.zip(paths)
			.filter_map(|(exp, path)| Some((exp, path?)))
			.min_by(|(_, a), (_, b)| a.total_cmp(b))
			.map(|(exp, _)| exp))
	}
}
