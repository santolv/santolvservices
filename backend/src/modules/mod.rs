pub mod calendar;
pub mod contacts;
pub mod drive;
pub mod mail;
pub mod notes;
pub mod photos;
mod record_support;
pub mod tasks;

use crate::core::manager::Service;

pub const REGISTERED: &[Service] = &[
    drive::SERVICE,
    photos::SERVICE,
    calendar::SERVICE,
    notes::SERVICE,
    tasks::SERVICE,
    contacts::SERVICE,
    mail::SERVICE,
];
