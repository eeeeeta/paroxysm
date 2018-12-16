use chrono::NaiveDateTime;
use crate::schema::{keywords, entries};

#[derive(Queryable)]
pub struct Keyword {
    pub id: i32,
    pub name: String,
    pub chan: String
}
#[derive(Queryable)]
pub struct Entry {
    pub id: i32,
    pub keyword_id: i32,
    pub idx: i32,
    pub text: String,
    pub creation_ts: NaiveDateTime,
    pub created_by: String
}
#[derive(Insertable)]
#[table_name="keywords"]
pub struct NewKeyword<'a> {
    pub name: &'a str,
    pub chan: &'a str
}
#[derive(Insertable)]
#[table_name="entries"]
pub struct NewEntry<'a> {
    pub keyword_id: i32,
    pub idx: i32,
    pub text: &'a str,
    pub creation_ts: NaiveDateTime,
    pub created_by: &'a str
}
