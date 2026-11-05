<!-- markdown-toc start - Don't edit this section. Run M-x markdown-toc-refresh-toc -->
**Table of Contents**

- [rust-sc2](#rust-sc2)
- [Getting started](#getting-started)
    - [Rust](#rust)
    - [StarCraft II](#starcraft-ii)
        - [Installation](#installation)
            - [Windows and macOS](#windows-and-macos)
            - [Linux](#linux)
                - [Headfull (Lutris and Wine)](#headfull-lutris-and-wine)
                - [Headless (no graphics)](#headless-no-graphics)
- [Example](#example)
    - [Running Example](#running-example)
        - [Headfull](#headfull)
        - [Headless](#headless)
    - [Runnint the advanced examples](#running-the-advanced-examples)
    - [Optional features](#optional-features)
    - [Making bot step by step](#making-bot-step-by-step)

<!-- markdown-toc end -->


# rust-sc2
[![crates.io](https://img.shields.io/crates/v/rust-sc2.svg)](https://crates.io/crates/rust-sc2)
[![Documentation](https://docs.rs/rust-sc2/badge.svg)](https://docs.rs/rust-sc2)

Rust implementation of StarCraft II API

The library aims to be simple and easy to use, being very fast and functional at the same time. However, it provides both high and low level abstractions. This lib is inspired by [python-sc2](https://github.com/BurnySc2/python-sc2) lib, so people might find it easy to switch to rust-sc2. It was originally created because other rust libs were old, not functional and low level.

Feel free to ask questions in `#rust` channel of [Starcraft 2 AI Discord](https://discord.gg/Emm5Ztz) server

# Getting started
## Rust
[Install latest stable Rust](https://www.rust-lang.org/tools/install)
(Older versions may also work, but compatibility is not guaranteed)

Create your project

`cargo new <project_name>`

Add to dependencies in Cargo.toml:
```toml
[dependencies]
rust-sc2 = "1"
```
Or if you want developer version directly from github:
```toml
[dependencies]
rust-sc2 = { git = "https://github.com/UltraMachine/rust-sc2" }
```
Or if you want to use a local version:
```toml
[dependencies]
rust-sc2 = { path = "/path/to/rust-sc2" }
```

**NOTE:** *Version of this library on crates.io is outdated and lacks many features. Unfortunately, I can't update it yet, so it's highly recommended to use github version for now.*

## StarCraft II

### Installation
#### Windows and macOS

Install SC2 through [Battle.net](https://www.blizzard.com/en-us/apps/battle.net/desktop).

#### Linux
##### Headfull (Lutris and Wine)

1. Install Lutris from your package manager
2. [Install Battle.net dependencies](https://github.com/lutris/docs/blob/master/Battle.Net.md). (Wine and Vulkan drivers)
3. [Install SC2 through Lutris](https://lutris.net/games/starcraft-ii/)

##### Headless (no graphics)

1. Download most recent [Linux Package](https://github.com/Blizzard/s2client-proto#linux-packages) (Maps will come with the zip)
2. Unzip to ~/StarCraftII (you'll need the End User License Agreement Password above the Linux Packages link)
3. Move your `.SC2Map` files up out of their `LadderXXXXSeasonX` directory to `Maps` directory. (Since there are multiple versions of the same map, there is no way of knowing which one you want.)


# Example
A simple bot that sends its workers at the enemy. Copy this into your `/path/to/project/main.rs`
```rust
use rust_sc2::prelude::*;

struct WorkerRush;

impl Player for WorkerRush {
    fn settings(&self) -> PlayerSettings<'_> {
        PlayerSettings::new(Race::Protoss)
    }
    fn on_start(&mut self, ctx: &mut Context) -> SC2Result<()> {
        for worker in ctx.world.my().workers() {
            ctx.cmds.attack(worker, ctx.world.map.enemy_start, false);
        }
        Ok(())
    }
}

fn main() -> SC2Result<()> {
    run_vs_computer(
        &mut WorkerRush,
        Computer::new(Race::Random, Difficulty::Medium, None),
        "EternalEmpireLE",
        Default::default(),
    )
}
```
Note: The Linux client doesn't have the map `EternalEmpireLE` so you'll need to download it, or reference another map from the `LadderXXXXSeasonX` directories.

## Running Example
### Headfull
1. `export SC2PATH="/home/<user>/Games/starcraft-ii/drive_c/Program Files (x86)/StarCraft II"`
2. Make sure you have this snippet in your project's **Cargo.toml**:
```toml
[features]
wine_sc2 = ["rust-sc2/wine_sc2"]

```
3. `cargo run --features wine_sc2`

### Headless
1. `export SC2PATH=/abs/path/to/StarCraftII`
2. `cargo run`

## Running the advanced examples
There are more advanced examples in the [`examples`](https://github.com/UltraMachine/rust-sc2/tree/master/examples) folder. To run one of these examples on your own machine, say the Reaper Rush one, clone this repository, navigate to the root folder and run the command
```
cargo run --example reaper-rush -- local
```
In addition to `local` (or `human` if you want to play against your own bot), these examples take several arguments. For a full list in either case, run the commands
```
cargo run --example reaper-rush -- local --help
cargo run --example reaper-rush -- human --help
```


To play on an SC2 client that's already running, pass its port:
```
cargo run --example reaper-rush -- local --map <map> --port <port>
```

## Optional features
- `"serde"` - adds implementation of `Serialize`, `Deserialize` to ids, Race, GameResult, ...
- `"wine_sc2"` - allows you to run headful SC2 through Lutris and Wine

## Making bot step by step
First of all, import rust-sc2 lib:
```rust
use rust_sc2::prelude::*;
```
Create your bot's struct. It only holds your own state, the library's state is passed to you:
```rust
#[derive(Default)]
struct MyBot {
    /* fields here */
}
```
Then implement the `Player` trait for your bot:
```rust
impl Player for MyBot {
    // Must be implemented
    fn settings(&self) -> PlayerSettings<'_> {
        // Race can be Terran, Zerg, Protoss or Random
        PlayerSettings::new(Race::Random)
    }

    // Methods below aren't necessary to implement (Empty by default)

    // Called once on first step
    fn on_start(&mut self, ctx: &mut Context) -> SC2Result<()> {
        /* your awesome code here */
    }

    // Called on every game step
    fn on_step(&mut self, ctx: &mut Context, iteration: usize) -> SC2Result<()> {
        /* your awesome code here */
    }

    // Called once on last step
    // "result" says if your bot won or lost game
    fn on_end(&mut self, result: GameResult) -> SC2Result<()> {
        /* your awesome code here */
    }

    // Called on different events, see more in `examples/events.rs`
    fn on_event(&mut self, ctx: &mut Context, event: Event) -> SC2Result<()> {
        /* your awesome code here */
    }
}
```
`Context` gives you everything you need during your turn:
- `ctx.world`: the game state of this step, such as `ctx.world.my().workers()` or `ctx.world.minerals`
- `ctx.cmds`: orders for your units, such as `ctx.cmds.attack(unit, target, false)`
- `ctx.budget`: resources left to spend this turn, such as `ctx.budget.can_afford(ctx.world, UnitTypeId::Marine, true)`
- `ctx.api`: questions for SC2, such as `ctx.api.find_placement(...)`
- `ctx.debug`: debug drawing and cheats

These are independent, so you can give orders and change your bot's fields while looping over units.

`World` and `UnitRef` are `Send + Sync`, so if a bot has expensive per-unit work it can run that in parallel with any library. For example, with `rayon`:
```rust
let workers: Vec<_> = ctx.world.my().workers().iter().collect();
let scores: Vec<_> = workers.par_iter().map(|u| score(*u)).collect();
```

To remember a unit between steps, keep its `UnitTag` and look it up with `ctx.world.unit(tag)`, which returns `None` once the unit is gone from the observation. A `UnitRef` can't be kept past the step it belongs to. The exception is anything SC2 shows as a snapshot in the fog of war: enemy buildings, mineral fields and geysers get temporary tags there. Remember those by position instead, which is why `Expansion` stores positions, and `ctx.world.minerals_of(expansion)` finds its minerals on the current step.

The rest is to run it:
```rust
fn main() -> SC2Result<()> {
    let mut bot = MyBot::default();
    run_vs_computer(
        &mut bot,
        Computer::new(
            Race::Random,
            Difficulty::VeryEasy,
            None, // AI Build (random here)
        ),
        "EternalEmpireLE", // Map name
        LaunchOptions::default(),
    )
}
```

## Updating game IDs
IDs of units, abilities, upgrades and buffs change between game versions. If they don't match your client, the library logs an error at game start and lists the differences in `world.map.id_mismatches`. To fix it, regenerate them from the `stableid.json` your client writes when it starts. Under Wine that file is in the Wine prefix, not your own Documents folder:
```
python3 generate_ids.py "/path/to/Documents/StarCraft II/stableid.json"
```
