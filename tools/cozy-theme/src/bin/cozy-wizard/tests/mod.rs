//! The wizard's tests, one module per screen.
//!
//! Split the same way the code is: a test about the packages page sits beside
//! the other tests about the packages page, not two hundred lines away from
//! them in one file that holds everything. `util` carries the fixtures they
//! share — chiefly the chain that walks an `App` to a given screen, which each
//! page's tests start from.

pub mod util;

mod apply;
mod greeting;
mod hostpages;
mod navigation;
mod packages;
mod patches;
mod schemes;
mod sticky;
mod themes;
