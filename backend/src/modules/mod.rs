pub mod calendar;
pub mod drive;
pub mod photos;

use crate::core::manager::Service;

pub const REGISTERED: &[Service] = &[drive::SERVICE, photos::SERVICE, calendar::SERVICE];
