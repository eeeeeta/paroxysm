use chrono::NaiveDateTime;
use crate::schema::{keywords, entries};

#[derive(Queryable)]
pub struct Keyword {
    id: i32,
    name: String,
    chan: String
}
#[derive(Queryable)]
pub struct Entry {
    id: i32,
    keyword_id: i32,
    idx: i32,
    creation_ts: NaiveDateTime,
    created_by: String
}
#[derive(Insertable)]
#[table_name="keywords"]
pub struct NewKeyword<'a> {
    name: &'a str,
    chan: &'a str
}
#[derive(Insertable)]
#[table_name="entries"]
pub struct NewEntry<'a> {
    keyword_id: i32,
    idx: i32,
    creation_ts: NaiveDateTime,
    created_by: &'a str
}
