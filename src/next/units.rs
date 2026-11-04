use super::{UnitRef, World};
use crate::distance::Distance;
use crate::geometry::Point2;
use crate::ids::UnitTypeId;
use std::borrow::Cow;

/// A group of units on the current step, stored as indices into [`World`].
///
/// Taking a group or iterating it doesn't touch any reference counts.
/// Indices stay in ascending order, which [`get`](Self::get) relies on.
#[derive(Clone)]
pub struct Units<'w> {
	world: &'w World,
	ids: Cow<'w, [u32]>,
}

impl<'w> Units<'w> {
	pub(super) fn borrowed(world: &'w World, ids: &'w [u32]) -> Self {
		Self {
			world,
			ids: Cow::Borrowed(ids),
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
	/// First unit of the group.
	pub fn first(&self) -> Option<UnitRef<'w>> {
		self.ids.first().map(|&id| self.world.unit_at(id))
	}
	/// Finds unit with given tag, if it's in this group.
	pub fn get(&self, tag: u64) -> Option<UnitRef<'w>> {
		let id = self.world.index_of(tag)?;
		self.ids.binary_search(&id).ok()?;
		Some(self.world.unit_at(id))
	}
	/// Units for which given predicate returns `true`.
	pub fn filter(&self, mut f: impl FnMut(UnitRef<'w>) -> bool) -> Units<'w> {
		let world = self.world;
		Units {
			world,
			ids: self
				.ids
				.iter()
				.copied()
				.filter(|&id| f(world.unit_at(id)))
				.collect(),
		}
	}
	/// Units of given type.
	pub fn of_type(&self, type_id: UnitTypeId) -> Units<'w> {
		self.filter(|u| u.type_id == type_id)
	}
	/// Units closer than given distance to target.
	pub fn closer<P: Into<Point2> + Copy>(&self, distance: f32, target: P) -> Units<'w> {
		let target = target.into();
		self.filter(|u| u.position.distance_squared(target) < distance * distance)
	}
	/// Closest unit to target.
	pub fn closest<P: Into<Point2> + Copy>(&self, target: P) -> Option<UnitRef<'w>> {
		let target = target.into();
		self.iter().min_by(|a, b| {
			a.position
				.distance_squared(target)
				.total_cmp(&b.position.distance_squared(target))
		})
	}
	/// Furthest unit from target.
	pub fn furthest<P: Into<Point2> + Copy>(&self, target: P) -> Option<UnitRef<'w>> {
		let target = target.into();
		self.iter().max_by(|a, b| {
			a.position
				.distance_squared(target)
				.total_cmp(&b.position.distance_squared(target))
		})
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

/// One player's units on the current step. Get it with [`World::my`] or [`World::enemy`].
#[derive(Clone, Copy)]
pub struct PlayerUnits<'w> {
	pub(super) world: &'w World,
	pub(super) groups: &'w PlayerGroups,
}

macro_rules! player_groups {
	($($(#[$doc:meta])* $name:ident),* $(,)?) => {
		impl<'w> PlayerUnits<'w> {
			$(
				$(#[$doc])*
				pub fn $name(self) -> Units<'w> {
					Units::borrowed(self.world, &self.groups.$name)
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
