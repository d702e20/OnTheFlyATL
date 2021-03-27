extern crate num_cpus;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate tracing;
#[macro_use]
mod simple_edg;

pub mod atl;
mod com;
mod common;
pub mod edg;
pub mod hasher;
pub mod lcgs;
mod printer;
