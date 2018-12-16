#![allow(proc_macro_derive_resolution_fallback)] // Needed until diesel fixes their stuff

extern crate irc;
extern crate serde;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate diesel;
extern crate chrono;
extern crate config;
extern crate env_logger;
#[macro_use] extern crate log;
#[macro_use] extern crate failure;
extern crate regex;
#[macro_use] extern crate lazy_static;

use irc::client::prelude::*;
use diesel::prelude::*;
use diesel::pg::PgConnection;
use failure::Error;
use regex::Regex;
use self::models::{Keyword, Entry, NewKeyword, NewEntry};
use std::fmt::Display;

mod schema;
mod models;

#[derive(Deserialize)]
pub struct Config {
    database_url: String,
    irc_config_path: String,
    #[serde(default)]
    log_filter: Option<String>
}
pub struct App {
    cli: IrcClient,
    pg: PgConnection
}
pub struct KeywordDetails {
    keyword: Keyword,
    entries: Vec<Entry>
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
            use self::schema::entries;
            ::diesel::insert_into(entries::table)
                .values(ins)
                .get_result(dbc)?
        };
        self.entries.push(new);
        Ok(self.entries.len())
    }
    pub fn format_entry(&self, idx: usize) -> Option<String> {
        if let Some(ent) = self.entries.get(idx.wrapping_sub(1)) {
            Some(format!("\x02{}\x0f[{}/{}]: {} \x0303[{}]\x0f", self.keyword.name, idx, self.entries.len(), ent.text, ent.creation_ts.date()))
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
            use self::schema::keywords;
            ::diesel::insert_into(keywords::table)
                .values(val)
                .get_result(dbc)?
        };
        Ok(KeywordDetails {
            keyword: ret,
            entries: vec![]
        })
    }
    pub fn get(word: &str, c: &str, dbc: &PgConnection) -> Result<Option<Self>, Error> {
        let keyword: Option<Keyword> = {
            use self::schema::keywords::dsl::*;
            keywords.filter(name.ilike(word).and(chan.eq(c)))
                .first(dbc)
                .optional()?
        };
        if let Some(k) = keyword {
            let entries: Vec<Entry> = {
                use self::schema::entries::dsl::*;
                entries.filter(keyword_id.eq(k.id))
                    .order_by(idx.asc())
                    .load(dbc)?
            };
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
impl App {
    pub fn report_error<T: Display>(&mut self, nick: &str, chan: &str, msg: T) -> Result<(), Error> {
        self.cli.send_notice(chan, format!("{}: Error: {}", nick, msg))?;
        Ok(())
    }
    pub fn handle_privmsg(&mut self, from: &str, chan: &str, msg: &str) -> Result<(), Error> {
        lazy_static! {
            static ref LEARN_RE: Regex = Regex::new(r#"^\?\?\s*(.*):\s*(.*)"#).unwrap();
            static ref QUERY_RE: Regex = Regex::new(r#"^\?\?\s*([^\[]*)(\[[0-9]+|\*\])?"#).unwrap();
        }
        let nick = from.split("!").next().ok_or(format_err!("Invalid source"))?;
        if let Some(learn) = LEARN_RE.captures(msg) {
            let subj = &learn[1];
            let val = &learn[2];
            debug!("Learning {}: {}", subj, val);
            let mut kwd = KeywordDetails::get_or_create(subj, chan, &self.pg)?;
            let idx = kwd.learn(nick, val, &self.pg)?;
            self.cli.send_notice(chan, kwd.format_entry(idx).unwrap())?;
        }
        else if let Some(query) = QUERY_RE.captures(msg) {
            let subj = &query[1];
            let idx = match query.get(2) {
                Some(i) => {
                    Some(match i.as_str() {
                        "all" => -1,
                        x => x.get(1..x.len()).ok_or(format_err!("invalid index"))?.parse::<i32>()?,
                    })
                },
                None => None,
            };
            debug!("Querying {} with idx {:?}", subj, idx);
            match KeywordDetails::get(subj, chan, &self.pg)? {
                Some(kwd) => {
                    if let Some(idx) = idx {
                        if idx == -1 {
                            for i in 0..kwd.entries.len() {
                                self.cli.send_notice(chan, kwd.format_entry(i+1).unwrap())?;
                            }
                        }
                        else {
                            if let Some(ent) = kwd.format_entry(idx as _) {
                                self.cli.send_notice(chan, ent)?;
                            }
                            else {
                                self.cli.send_notice(chan, format!("{}: only has {} entries", subj, kwd.entries.len()))?;
                            }
                        }
                    }
                    else {
                        if let Some(ent) = kwd.format_entry(1) {
                            self.cli.send_notice(chan, ent)?;
                        }
                        else {
                            self.cli.send_notice(chan, format!("{}: blank keyword", subj))?;
                        }
                    }
                },
                None => {
                    self.cli.send_notice(chan, format!("{}: no entries yet", subj))?;
                }
            }
        }
        Ok(())
    }
    pub fn handle_msg(&mut self, m: Message) -> Result<(), Error> {
        if let Command::PRIVMSG(channel, message) = m.command {
            if let Some(src) = m.prefix {
                self.handle_privmsg(&src, &channel, &message)?;
            }
        }
        Ok(())
    }
}
fn main() -> Result<(), Error> {
    println!("[+] loading configuration");
    let default_log_filter = concat!("paroxysm=info").to_string();
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("paroxysm"))?;
    let cfg: Config = settings.try_into()?;
    let env = env_logger::Env::new().default_filter_or(cfg.log_filter.unwrap_or(default_log_filter));
    env_logger::init_from_env(env);
    info!("paroxysm starting up");
    info!("connecting to database");
    let pg = PgConnection::establish(&cfg.database_url)?;
    info!("connecting to IRC");
    let cli = IrcClient::new(cfg.irc_config_path)?;
    cli.identify()?;
    let st = cli.stream();
    let mut app = App { cli, pg };
    info!("running!");
    st.for_each_incoming(|m| {
        if let Err(e) = app.handle_msg(m) {
            warn!("Error processing message: {}", e);
        }
    })?;
    Ok(())
}
