use super::{UnitRef, World};
use crate::{
	distance::{Center, Distance},
	geometry::Point2,
	ids::UnitTypeId,
	units::Container,
};
use std::{borrow::Cow, cmp::Ordering, iter::Sum};

/// A group of units on the current step, stored as indices into [`World`].
///
/// Taking a group or iterating it doesn't touch any reference counts.
#[derive(Clone)]
pub struct Units<'w> {
	world: &'w World,
	ids: Cow<'w, [u32]>,
	// Ids of current units in ascending order, which makes `get` a binary search.
	searchable: bool,
}

impl<'w> Units<'w> {
	pub(super) fn borrowed(world: &'w World, ids: &'w [u32]) -> Self {
		Self {
			world,
			ids: Cow::Borrowed(ids),
			searchable: true,
		}
	}
	pub(super) fn unordered(world: &'w World, ids: Cow<'w, [u32]>) -> Self {
		Self {
			world,
			ids,
			searchable: false,
		}
	}
	fn with_ids(&self, ids: Vec<u32>) -> Units<'w> {
		Units {
			world: self.world,
			ids: Cow::Owned(ids),
			searchable: self.searchable,
		}
	}
	/// Number of units in the group.
	pub fn len(&self) -> usize {
		self.ids.len()
	}
	/// Checks if the group is empty.
	pub fn is_empty(&self) -> bool {
		self.ids.is_empty()
	}
	/// Iterates over the units.
	pub fn iter(&self) -> Iter<'_, 'w> {
		Iter {
			world: self.world,
			ids: self.ids.iter(),
		}
	}
	/// Iterates over the tags of the units.
	pub fn tags(&self) -> impl Iterator<Item = u64> + '_ {
		self.iter().map(|u| u.tag)
	}
	/// First unit of the group.
	pub fn first(&self) -> Option<UnitRef<'w>> {
		self.ids.first().map(|&id| self.world.unit_at(id))
	}
	/// Finds unit with given tag, if it's in this group.
	pub fn get(&self, tag: u64) -> Option<UnitRef<'w>> {
		if self.searchable {
			let id = self.world.index_of(tag)?;
			self.ids.binary_search(&id).ok()?;
			Some(self.world.unit_at(id))
		} else {
			self.iter().find(|u| u.tag == tag)
		}
	}
	/// Checks if unit with given tag is in this group.
	pub fn contains_tag(&self, tag: u64) -> bool {
		self.get(tag).is_some()
	}
	/// Finds units with given tags, in the order of the tags.
	pub fn find_tags<'a>(&self, tags: impl IntoIterator<Item = &'a u64>) -> Units<'w> {
		let ids = tags
			.into_iter()
			.filter_map(|&tag| self.get(tag).map(UnitRef::id))
			.collect::<Vec<_>>();
		Units::unordered(self.world, Cow::Owned(ids))
	}
	/// Units for which given predicate returns `true`.
	pub fn filter(&self, mut f: impl FnMut(UnitRef<'w>) -> bool) -> Units<'w> {
		let world = self.world;
		self.with_ids(
			self.ids
				.iter()
				.copied()
				.filter(|&id| f(world.unit_at(id)))
				.collect(),
		)
	}
	/// Units of given type.
	pub fn of_type(&self, type_id: UnitTypeId) -> Units<'w> {
		self.filter(|u| u.type_id == type_id)
	}
	/// Units not of given type.
	pub fn exclude_type(&self, type_id: UnitTypeId) -> Units<'w> {
		self.filter(|u| u.type_id != type_id)
	}
	/// Units of any of given types.
	pub fn of_types<T: Container<UnitTypeId>>(&self, types: &T) -> Units<'w> {
		self.filter(|u| types.contains(&u.type_id))
	}
	/// Units not of any of given types.
	pub fn exclude_types<T: Container<UnitTypeId>>(&self, types: &T) -> Units<'w> {
		self.filter(|u| !types.contains(&u.type_id))
	}
	/// Ground units.
	pub fn ground(&self) -> Units<'w> {
		self.filter(|u| !u.is_flying)
	}
	/// Flying units.
	pub fn flying(&self) -> Units<'w> {
		self.filter(|u| u.is_flying)
	}
	/// Complete units and structures.
	pub fn ready(&self) -> Units<'w> {
		self.filter(|u| u.is_ready())
	}
	/// Units and structures still in progress.
	pub fn not_ready(&self) -> Units<'w> {
		self.filter(|u| !u.is_ready())
	}
	/// Units with no orders.
	pub fn idle(&self) -> Units<'w> {
		self.filter(|u| u.is_idle())
	}
	/// Units with no orders, or whose only order is more than 95% complete.
	pub fn almost_idle(&self) -> Units<'w> {
		self.filter(|u| u.is_almost_idle())
	}
	/// Idle units, or production buildings with a free reactor slot.
	pub fn unused(&self) -> Units<'w> {
		self.filter(|u| u.is_unused())
	}
	/// As [`unused`](Self::unused), counting orders more than 95% complete as finished.
	pub fn almost_unused(&self) -> Units<'w> {
		self.filter(|u| u.is_almost_unused())
	}
	/// Fully visible units.
	pub fn visible(&self) -> Units<'w> {
		self.filter(|u| u.is_visible())
	}
	/// Units that can be attacked by given unit, without considering upgrades.
	pub fn in_range_of(&self, unit: UnitRef<'_>, gap: f32) -> Units<'w> {
		self.filter(|u| unit.in_range(u, gap))
	}
	/// Units that can attack given unit, without considering upgrades.
	pub fn in_range(&self, unit: UnitRef<'_>, gap: f32) -> Units<'w> {
		self.filter(|u| u.in_range(unit, gap))
	}
	/// Units that can be attacked by given unit, considering upgrades.
	pub fn in_real_range_of(&self, unit: UnitRef<'_>, gap: f32) -> Units<'w> {
		self.filter(|u| unit.in_real_range(u, gap))
	}
	/// Units that can attack given unit, considering upgrades.
	pub fn in_real_range(&self, unit: UnitRef<'_>, gap: f32) -> Units<'w> {
		self.filter(|u| u.in_real_range(unit, gap))
	}
	/// Units closer than given distance to target.
	pub fn closer<P: Into<Point2> + Copy>(&self, distance: f32, target: P) -> Units<'w> {
		let target = target.into();
		self.filter(|u| u.position.is_closer(distance, target))
	}
	/// Units further than given distance from target.
	pub fn further<P: Into<Point2> + Copy>(&self, distance: f32, target: P) -> Units<'w> {
		let target = target.into();
		self.filter(|u| u.position.is_further(distance, target))
	}
	/// Closest unit to target.
	pub fn closest<P: Into<Point2> + Copy>(&self, target: P) -> Option<UnitRef<'w>> {
		let target = target.into();
		self.min(|u| u.position.distance_squared(target))
	}
	/// Furthest unit from target.
	pub fn furthest<P: Into<Point2> + Copy>(&self, target: P) -> Option<UnitRef<'w>> {
		let target = target.into();
		self.max(|u| u.position.distance_squared(target))
	}
	/// Distance from target to the closest unit.
	pub fn closest_distance<P: Into<Point2> + Copy>(&self, target: P) -> Option<f32> {
		self.closest_distance_squared(target).map(f32::sqrt)
	}
	/// Distance from target to the furthest unit.
	pub fn furthest_distance<P: Into<Point2> + Copy>(&self, target: P) -> Option<f32> {
		self.furthest_distance_squared(target).map(f32::sqrt)
	}
	/// Squared distance from target to the closest unit.
	pub fn closest_distance_squared<P: Into<Point2> + Copy>(&self, target: P) -> Option<f32> {
		let target = target.into();
		self.min_value(|u| u.position.distance_squared(target))
	}
	/// Squared distance from target to the furthest unit.
	pub fn furthest_distance_squared<P: Into<Point2> + Copy>(&self, target: P) -> Option<f32> {
		let target = target.into();
		self.max_value(|u| u.position.distance_squared(target))
	}
	/// Center of the units, or `None` if the group is empty.
	pub fn center(&self) -> Option<Point2> {
		self.iter().map(|u| u.position).center()
	}
	/// Sum of values given function returns for each unit.
	pub fn sum<T: Sum, F: FnMut(UnitRef<'w>) -> T>(&self, f: F) -> T {
		self.iter().map(f).sum()
	}
	/// Unit with the smallest value of given function.
	pub fn min<T: PartialOrd, F: FnMut(UnitRef<'w>) -> T>(&self, mut f: F) -> Option<UnitRef<'w>> {
		self.iter()
			.map(|u| (f(u), u))
			.reduce(|a, b| if b.0 < a.0 { b } else { a })
			.map(|(_, u)| u)
	}
	/// The smallest value of given function.
	pub fn min_value<T: PartialOrd, F: FnMut(UnitRef<'w>) -> T>(&self, f: F) -> Option<T> {
		self.iter().map(f).reduce(|a, b| if b < a { b } else { a })
	}
	/// Unit with the largest value of given function.
	pub fn max<T: PartialOrd, F: FnMut(UnitRef<'w>) -> T>(&self, mut f: F) -> Option<UnitRef<'w>> {
		self.iter()
			.map(|u| (f(u), u))
			.reduce(|a, b| if b.0 > a.0 { b } else { a })
			.map(|(_, u)| u)
	}
	/// The largest value of given function.
	pub fn max_value<T: PartialOrd, F: FnMut(UnitRef<'w>) -> T>(&self, f: F) -> Option<T> {
		self.iter().map(f).reduce(|a, b| if b > a { b } else { a })
	}
	/// A copy of the group, sorted by given key in ascending order.
	pub fn sorted<T: PartialOrd, F: FnMut(UnitRef<'w>) -> T>(&self, mut f: F) -> Units<'w> {
		let world = self.world;
		let mut keyed = self
			.ids
			.iter()
			.map(|&id| (f(world.unit_at(id)), id))
			.collect::<Vec<_>>();
		keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
		Units::unordered(world, Cow::Owned(keyed.into_iter().map(|(_, id)| id).collect()))
	}
}

#[cfg(feature = "rayon")]
impl<'w> Units<'w> {
	/// Iterates over the units in parallel.
	pub fn par_iter(&self) -> impl rayon::iter::ParallelIterator<Item = UnitRef<'w>> + '_ {
		use rayon::prelude::*;
		let world = self.world;
		self.ids.par_iter().map(move |&id| world.unit_at(id))
	}
	/// Iterates over the tags of the units in parallel.
	pub fn par_tags(&self) -> impl rayon::iter::ParallelIterator<Item = u64> + '_ {
		use rayon::prelude::*;
		self.par_iter().map(|u| u.tag)
	}
	/// As [`sorted`](Self::sorted), computing the keys and sorting in parallel.
	pub fn par_sorted<T, F>(&self, f: F) -> Units<'w>
	where
		T: PartialOrd + Send,
		F: Fn(UnitRef<'w>) -> T + Sync + Send,
	{
		use rayon::prelude::*;
		let world = self.world;
		let mut keyed = self.ids.par_iter().map(|&id| (f(world.unit_at(id)), id)).collect::<Vec<_>>();
		keyed.par_sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
		Units::unordered(world, Cow::Owned(keyed.into_iter().map(|(_, id)| id).collect()))
	}
	/// As [`filter`](Self::filter), running the predicate in parallel. Keeps the order of the group.
	pub fn par_filter(&self, f: impl Fn(UnitRef<'w>) -> bool + Sync + Send) -> Units<'w> {
		use rayon::prelude::*;
		let world = self.world;
		self.with_ids(
			self.ids
				.par_iter()
				.copied()
				.filter(|&id| f(world.unit_at(id)))
				.collect(),
		)
	}
}

/// Iterator over a borrowed group.
pub struct Iter<'a, 'w> {
	world: &'w World,
	ids: std::slice::Iter<'a, u32>,
}
impl<'w> Iterator for Iter<'_, 'w> {
	type Item = UnitRef<'w>;

	fn next(&mut self) -> Option<Self::Item> {
		self.ids.next().map(|&id| self.world.unit_at(id))
	}
	fn size_hint(&self) -> (usize, Option<usize>) {
		self.ids.size_hint()
	}
}
impl<'a, 'w> IntoIterator for &'a Units<'w> {
	type Item = UnitRef<'w>;
	type IntoIter = Iter<'a, 'w>;

	fn into_iter(self) -> Self::IntoIter {
		self.iter()
	}
}

/// Iterator that owns its group, so `for u in world.my().workers()` works.
pub struct IntoIter<'w> {
	units: Units<'w>,
	pos: usize,
}
impl<'w> Iterator for IntoIter<'w> {
	type Item = UnitRef<'w>;

	fn next(&mut self) -> Option<Self::Item> {
		let id = *self.units.ids.get(self.pos)?;
		self.pos += 1;
		Some(self.units.world.unit_at(id))
	}
}
impl<'w> IntoIterator for Units<'w> {
	type Item = UnitRef<'w>;
	type IntoIter = IntoIter<'w>;

	fn into_iter(self) -> Self::IntoIter {
		IntoIter { units: self, pos: 0 }
	}
}

/// Indices of one player's units, by category.
#[derive(Default, Clone)]
pub(super) struct PlayerGroups {
	pub all: Vec<u32>,
	pub units: Vec<u32>,
	pub structures: Vec<u32>,
	pub placeholders: Vec<u32>,
	pub townhalls: Vec<u32>,
	pub workers: Vec<u32>,
	pub gas_buildings: Vec<u32>,
	pub larvas: Vec<u32>,
}

/// One player's units on the current step.
/// Get it with [`World::my`], [`World::enemy`] or [`World::enemy_memory`].
#[derive(Clone, Copy)]
pub struct PlayerUnits<'w> {
	pub(super) world: &'w World,
	pub(super) groups: &'w PlayerGroups,
	pub(super) searchable: bool,
}

macro_rules! player_groups {
	($($(#[$doc:meta])* $name:ident),* $(,)?) => {
		impl<'w> PlayerUnits<'w> {
			$(
				$(#[$doc])*
				pub fn $name(self) -> Units<'w> {
					let ids = &self.groups.$name;
					if self.searchable {
						Units::borrowed(self.world, ids)
					} else {
						Units::unordered(self.world, Cow::Borrowed(ids))
					}
				}
			)*
		}
	};
}
player_groups! {
	/// All units and structures.
	all,
	/// Units only, without structures.
	units,
	/// Structures only.
	structures,
	/// Buildings that haven't started construction yet (your own only).
	placeholders,
	/// Townhalls.
	townhalls,
	/// Workers, without MULEs.
	workers,
	/// Gas buildings.
	gas_buildings,
	/// Larvas.
	larvas,
}
