use crate::error::{Error, Result};
use axum::{Json, extract::rejection::JsonRejection, http::StatusCode};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use uuid::Uuid;
#[derive(Deserialize)]
pub(super) struct ListQuery {
    q: Option<String>,
    cursor: Option<String>,
    pub(super) limit: Option<String>,
}

pub(super) struct ListOptions {
    pub(super) pattern: Option<String>,
    pub(super) before_time: Option<OffsetDateTime>,
    pub(super) before_id: Option<Uuid>,
    pub(super) limit: i64,
}

impl ListQuery {
    pub(super) fn options(self) -> Result<ListOptions> {
        let limit = self
            .limit
            .as_deref()
            .unwrap_or("50")
            .parse()
            .map_err(|_| Error::bad("invalid_limit"))?;
        if !(1..=100).contains(&limit) {
            return Err(Error::bad("invalid_limit"));
        }

        let pattern = self
            .q
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(search_pattern)
            .transpose()?;
        let (before_time, before_id) = match self.cursor {
            Some(cursor) => {
                let invalid = || Error::bad("invalid_cursor");
                let (date, id) = cursor.split_once('|').ok_or_else(invalid)?;
                let date = parse_date(date).ok_or_else(invalid)?;
                let id = Uuid::parse_str(id).map_err(|_| invalid())?;
                (Some(date), Some(id))
            }
            None => (None, None),
        };
        Ok(ListOptions {
            pattern,
            before_time,
            before_id,
            limit,
        })
    }
}

#[derive(Serialize)]
pub(super) struct Page<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
}

pub(super) fn page<T>(
    mut items: Vec<T>,
    limit: i64,
    key: impl Fn(&T) -> (OffsetDateTime, Uuid),
) -> Result<Json<Page<T>>> {
    let has_more = items.len() > limit as usize;
    items.truncate(limit as usize);
    let next_cursor = items
        .last()
        .filter(|_| has_more)
        .map(|item| {
            let (date, id) = key(item);
            date.format(&Rfc3339)
                .map(|date| format!("{date}|{id}"))
                .map_err(|_| Error::unavailable())
        })
        .transpose()?;
    Ok(Json(Page { items, next_cursor }))
}

pub(super) fn search_pattern(q: &str) -> Result<String> {
    if q.chars().count() > 200 || q.contains('\0') {
        return Err(Error::bad("invalid_search"));
    }
    let mut pattern = String::from("%");
    for character in q.chars() {
        if matches!(character, '!' | '%' | '_') {
            pattern.push('!');
        }
        pattern.push(character);
    }
    pattern.push('%');
    Ok(pattern)
}

pub(super) fn validate_title(title: &str) -> Result<&str> {
    let title = title.trim();
    if !(1..=200).contains(&title.chars().count()) || title.contains('\0') {
        return Err(Error::bad("invalid_title"));
    }
    Ok(title)
}

pub(super) fn parse_date(value: &str) -> Option<OffsetDateTime> {
    let date = OffsetDateTime::parse(value, &Rfc3339)
        .ok()?
        .checked_to_offset(UtcOffset::UTC)?;
    (0..=9999).contains(&date.year()).then_some(date)
}

pub(super) fn id_from_path(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| Error::bad("invalid_id"))
}

pub(super) fn body<T>(value: std::result::Result<Json<T>, JsonRejection>) -> Result<T> {
    value
        .map(|Json(value)| value)
        .map_err(|rejection| match rejection.status() {
            StatusCode::PAYLOAD_TOO_LARGE => {
                Error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")
            }
            StatusCode::UNSUPPORTED_MEDIA_TYPE => {
                Error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required")
            }
            _ => Error::bad("invalid_json"),
        })
}
