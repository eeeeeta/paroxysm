use crate::models::{Keyword, Entry, NewKeyword, NewEntry};
use failure::Error;
use diesel::prelude::*;
use diesel::pg::PgConnection;

pub struct KeywordDetails {
    pub keyword: Keyword,
    pub entries: Vec<Entry>
}
impl KeywordDetails {
    pub fn learn(&mut self, nick: &str, text: &str, dbc: &PgConnection) -> Result<usize, Error> {
        let now = ::chrono::Utc::now().naive_utc();
        let ins = NewEntry {
            keyword_id: self.keyword.id,
            idx: (self.entries.len()+1) as _,
            text,
            creation_ts: now,
            created_by: nick
        };
        let new = {
            use crate::schema::entries;
            ::diesel::insert_into(entries::table)
                .values(ins)
                .get_result(dbc)?
        };
        self.entries.push(new);
        Ok(self.entries.len())
    }
    pub fn process_moves(&mut self, moves: &[(i32, i32)], dbc: &PgConnection) -> Result<(), Error> {
        for (oid, new_idx) in moves {
            {
                use crate::schema::entries::dsl::*;
                ::diesel::update(entries.filter(id.eq(oid)))
                    .set(idx.eq(new_idx))
                    .execute(dbc)?;
            }
        }
        self.entries = Self::get_entries(self.keyword.id, dbc)?;
        Ok(())
    }
    pub fn swap(&mut self, idx_a: usize, idx_b: usize, dbc: &PgConnection) -> Result<(), Error> {
        let mut moves = vec![];
        for ent in self.entries.iter() {
            if ent.idx == idx_a as i32 {
                moves.push((ent.id, idx_b as i32));
            }
            if ent.idx == idx_b as i32 {
                moves.push((ent.id, idx_a as i32));
            }
        }
        if moves.len() != 2 {
            Err(format_err!("Invalid swap operation."))?;
        }
        self.process_moves(&moves, dbc)?;
        Ok(())
    }
    pub fn delete(&mut self, idx: usize, dbc: &PgConnection) -> Result<(), Error> {
        // step 1: delete the element
        {
            let ent = self.entries.get(idx.saturating_sub(1)).ok_or(format_err!("No such element to delete."))?;
            {
                use crate::schema::entries::dsl::*;
                ::diesel::delete(entries.filter(id.eq(ent.id)))
                    .execute(dbc)?;
            }
        }
        // step 2: move all the elements in front of it back one
        let mut moves = vec![];
        for ent in self.entries.iter() {
            if idx > ent.idx as _ {
                moves.push((ent.id, ent.idx.saturating_sub(1)));
            }
        }
        self.process_moves(&moves, dbc)?;
        Ok(())
    }
    pub fn format_entry(&self, idx: usize) -> Option<String> {
        if let Some(ent) = self.entries.get(idx.saturating_sub(1)) {
            let gen_clr = if self.keyword.chan == "*" { "\x0307" } else { "" };
            Some(format!("\x02{}{}\x0f\x0315[{}/{}]\x0f: {} \x0f\x0314[{}]\x0f", gen_clr, self.keyword.name, idx, self.entries.len(), ent.text, ent.creation_ts.date()))
        }
        else {
            None
        }
    }
    pub fn get_or_create(word: &str, c: &str, dbc: &PgConnection) -> Result<Self, Error> {
        if let Some(ret) = Self::get(word, c, dbc)? {
            Ok(ret)
        }
        else {
            Ok(Self::create(word, c, dbc)?)
        }
    }
    pub fn create(word: &str, c: &str, dbc: &PgConnection) -> Result<Self, Error> {
        let val = NewKeyword {
            name: word,
            chan: c
        };
        let ret: Keyword = {
            use crate::schema::keywords;
            ::diesel::insert_into(keywords::table)
                .values(val)
                .get_result(dbc)?
        };
        Ok(KeywordDetails {
            keyword: ret,
            entries: vec![]
        })
    }
    fn get_entries(kid: i32, dbc: &PgConnection) -> Result<Vec<Entry>, Error> {
        let entries: Vec<Entry> = {
            use crate::schema::entries::dsl::*;
            entries.filter(keyword_id.eq(kid))
                .order_by(idx.asc())
                .load(dbc)?
        };
        Ok(entries)
    }
    pub fn get(word: &str, c: &str, dbc: &PgConnection) -> Result<Option<Self>, Error> {
        let keyword: Option<Keyword> = {
            use crate::schema::keywords::dsl::*;
            keywords.filter(name.ilike(word).and(chan.eq(c).or(chan.eq("*"))))
                .first(dbc)
                .optional()?
        };
        if let Some(k) = keyword {
            let entries = Self::get_entries(k.id, dbc)?;
            Ok(Some(KeywordDetails {
                keyword: k,
                entries
            }))
        }
        else {
            Ok(None)
        }
    }
}
