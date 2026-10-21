//! Applying plans: API call planning, execution, and writing files back.

mod execute;
mod files;
mod planner;

pub use execute::{Outcome, execute};
pub(crate) use files::duration;
pub use files::{profile_toml, replace_preserving, slug, write_id};
pub use planner::{ApiCall, calls, empties_a_list, list_path};

#[cfg(test)]
mod tests;
