//! Avatar & Character System (P1)
//!
//! These modules implement player and NPC characters with movement,
//! camera following, collision, and dialogue systems.

pub mod camera;
pub mod dialogue;
pub mod npc;
pub mod player;
pub mod spawn_point;

pub use camera::*;
pub use dialogue::*;
pub use npc::*;
pub use player::*;
pub use spawn_point::*;
