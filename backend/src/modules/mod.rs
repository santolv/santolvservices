pub mod calendar;
pub mod contacts;
pub mod documents;
pub mod drive;
pub mod mail;
pub mod notes;
pub mod pdf;
pub mod photos;
pub mod presentations;
mod record_support;
pub mod spreadsheets;
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
    documents::SERVICE,
    spreadsheets::SERVICE,
    presentations::SERVICE,
    pdf::SERVICE,
];
