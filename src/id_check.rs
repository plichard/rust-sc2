//! Compares the generated ID tables with what the connected SC2 client reports.
//!
//! IDs change between game versions, and using tables from another version doesn't fail:
//! every number still maps to a name, just the wrong one.

use crate::ids::{AbilityId, BuffId, EffectId, UnitTypeId, UpgradeId};
use num_traits::FromPrimitive;
use sc2_proto::sc2api::ResponseData;
use std::fmt::Debug;

/// An ID where the tables disagree with the connected client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdMismatch {
	/// What the ID is for: `unit`, `ability`, `upgrade`, `buff` or `effect`.
	pub kind: &'static str,
	/// The ID.
	pub id: u32,
	/// What the client calls it.
	pub client_name: String,
	/// What the tables call it, or `None` if they don't have it.
	pub table_name: Option<String>,
}

fn normalize(name: &str) -> String {
	name.chars()
		.filter(char::is_ascii_alphanumeric)
		.map(|c| c.to_ascii_lowercase())
		.collect()
}

/// The generator builds names from the client's, sometimes adding to them, so one may contain the other.
fn names_match(table_name: &str, client_names: &[&str]) -> bool {
	let table_name = normalize(table_name);
	client_names
		.iter()
		.map(|name| normalize(name))
		.filter(|name| !name.is_empty())
		.any(|name| table_name.contains(&name) || name.contains(&table_name))
}

fn check<T: Debug>(
	kind: &'static str,
	id: u32,
	entry: Option<T>,
	client_names: &[&str],
	missing_is_a_problem: bool,
	mismatches: &mut Vec<IdMismatch>,
) {
	if id == 0 || client_names.iter().all(|name| name.is_empty()) {
		return;
	}
	let client_name = || {
		client_names
			.iter()
			.find(|name| !name.is_empty())
			.unwrap()
			.to_string()
	};
	match entry.map(|entry| format!("{:?}", entry)) {
		Some(table_name) if names_match(&table_name, client_names) => {}
		Some(table_name) => mismatches.push(IdMismatch {
			kind,
			id,
			client_name: client_name(),
			table_name: Some(table_name),
		}),
		None if missing_is_a_problem => mismatches.push(IdMismatch {
			kind,
			id,
			client_name: client_name(),
			table_name: None,
		}),
		None => {}
	}
}

/// Finds the IDs where the tables disagree with given response from the client.
///
/// Abilities missing from the tables aren't reported, because `generate_ids.py`
/// leaves out thousands that a bot can't use on purpose.
pub(super) fn check_ids(data: &ResponseData) -> Vec<IdMismatch> {
	let mut found = vec![];
	for u in &data.units {
		let id = u.unit_id();
		check(
			"unit",
			id,
			UnitTypeId::from_u32(id),
			&[u.name()],
			true,
			&mut found,
		);
	}
	for a in &data.abilities {
		let id = a.ability_id();
		let names = [a.link_name(), a.button_name(), a.friendly_name()];
		check("ability", id, AbilityId::from_u32(id), &names, false, &mut found);
	}
	for u in &data.upgrades {
		let id = u.upgrade_id();
		check(
			"upgrade",
			id,
			UpgradeId::from_u32(id),
			&[u.name()],
			true,
			&mut found,
		);
	}
	for b in &data.buffs {
		let id = b.buff_id();
		check("buff", id, BuffId::from_u32(id), &[b.name()], true, &mut found);
	}
	for e in &data.effects {
		let id = e.effect_id();
		let names = [e.name(), e.friendly_name()];
		check("effect", id, EffectId::from_u32(id), &names, true, &mut found);
	}
	found
}

/// Logs a summary at error level, so it shows up with default logging settings.
pub(super) fn report(mismatches: &[IdMismatch]) {
	if let Some(first) = mismatches.first() {
		error!(
			"{} IDs in rust-sc2's tables disagree with this SC2 client, so units, abilities or upgrades \
			 may be misidentified. Run generate_ids.py with this client's stableid.json. First: {:?}",
			mismatches.len(),
			first
		);
	}
}
