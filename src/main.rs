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
use std::fmt::Display;
use crate::cfg::Config;
use crate::keyword::KeywordDetails;

mod schema;
mod models;
mod cfg;
mod keyword;

pub struct App {
    cli: IrcClient,
    pg: PgConnection,
    cfg: Config
}

impl App {
    pub fn report_error<T: Display>(&mut self, nick: &str, chan: &str, msg: T) -> Result<(), Error> {
        self.cli.send_notice(nick, format!("[{}] \x0304Error:\x0f {}", chan, msg))?;
        Ok(())
    }
    pub fn keyword_from_captures(&mut self, learn: &::regex::Captures, nick: &str, chan: &str) -> Result<KeywordDetails, Error> {
        debug!("Fetching keyword for captures: {:?}", learn);
        let subj = &learn["subj"];
        let learn_chan = if learn.name("gen").is_some() {
            "*"
        }
        else {
            chan
        };
        if !chan.starts_with("#") && learn_chan != "*" {
            Err(format_err!("Only general entries may be taught via PM."))?;
        }
        debug!("Fetching keyword '{}' for chan {}", subj, learn_chan);
        let kwd = KeywordDetails::get_or_create(subj, learn_chan, &self.pg)?;
        if kwd.keyword.chan == "*" && !self.cfg.admins.contains(nick) {
            Err(format_err!("Only administrators can create or modify general entries."))?;
        }
        Ok(kwd)
    }
    pub fn handle_privmsg(&mut self, from: &str, chan: &str, msg: &str) -> Result<(), Error> {
        lazy_static! {
            static ref LEARN_RE: Regex = Regex::new(r#"^\?\?(?P<gen>!)?\s*(?P<subj>[^\[:]*):\s*(?P<val>.*)"#).unwrap();
            static ref QUERY_RE: Regex = Regex::new(r#"^\?\?\s*(?P<subj>[^\[:]*)(?P<idx>\[[^\]]+\])?"#).unwrap();
            static ref INCREMENT_RE: Regex = Regex::new(r#"^\?\?(?P<gen>!)?\s*(?P<subj>[^\[:]*)(?P<incrdecr>(++|--))"#).unwrap();
            static ref MOVE_RE: Regex = Regex::new(r#"^\?\?(?P<gen>!)?\s*(?P<subj>[^\[:]*)(?P<idx>\[[^\]]+\])->(?P<new_idx>.*)"#).unwrap();
        }
        let nick = from.split("!").next().ok_or(format_err!("Invalid source"))?;
        let tgt = if chan.starts_with("#") {
            chan
        }
        else {
            nick
        };
        debug!("[{}] <{}> {}", chan, nick, msg);
        if let Some(learn) = LEARN_RE.captures(msg) {
            let val = &learn["val"];
            let mut kwd = self.keyword_from_captures(&learn, nick, chan)?;
            let idx = kwd.learn(nick, val, &self.pg)?;
            self.cli.send_notice(tgt, kwd.format_entry(idx).unwrap())?;
        }
        else if let Some(mv) = MOVE_RE.captures(msg) {
            let idx = &mv["idx"];
            let idx = match idx[1..(idx.len()-1)].parse::<usize>() {
                Ok(i) => i,
                Err(e) => {
                    self.report_error(nick, chan, format!("Could not parse index: {}", e))?;
                    return Ok(());
                }
            };
            let new_idx = match mv["new_idx"].parse::<i32>() {
                Ok(i) => i,
                Err(e) => {
                    self.report_error(nick, chan, format!("Could not parse target index: {}", e))?;
                    return Ok(());
                }
            };
            let mut kwd = self.keyword_from_captures(&mv, nick, chan)?;
            if new_idx < 0 {
                kwd.delete(idx, &self.pg)?;
                self.cli.send_notice(tgt, format!("\x02{}\x0f: Deleted entry {}.", kwd.keyword.name, idx))?;
            }
            else {
                kwd.swap(idx, new_idx as _, &self.pg)?;
                self.cli.send_notice(tgt, format!("\x02{}\x0f: Swapped entries {} and {}.", kwd.keyword.name, idx, new_idx))?;
            }
        }
        else if let Some(icr) = INCREMENT_RE.captures(msg) {
            let mut kwd = self.keyword_from_captures(&icr, nick, chan)?;
            let is_incr = &icr["incrdecr"] == "++";
            let now = chrono::Utc::now().naive_utc().date();
            let mut idx = None;
            for (i, ent) in kwd.entries.iter().enumerate() {
                if ent.creation_ts.date() == now {
                    if let Ok(val) = ent.text.parse::<i32>() {
                        let val = if is_incr {
                            val + 1
                        }
                        else {
                            val - 1
                        };
                        idx = Some((i+1, val));
                    }
                }
            }
            if let Some((i, val)) = idx {
                kwd.update(i, &val.to_string(), &self.pg)?;
                self.cli.send_notice(tgt, kwd.format_entry(i).unwrap())?;
            }
            else {
                let val = if is_incr { 1 } else { -1 };
                let idx = kwd.learn(nick, &val.to_string(), &self.pg)?;
                self.cli.send_notice(tgt, kwd.format_entry(idx).unwrap())?;
            }
        }
        else if let Some(query) = QUERY_RE.captures(msg) {
            let subj = &query["subj"];
            let idx = match query.name("idx") {
                Some(i) => {
                    let i = i.as_str();
                    match &i[1..(i.len()-1)] {
                        "*" => Some(-1),
                        x => x.parse::<usize>().map(|x| x as i32).ok(),
                    }
                },
                None => None,
            };
            debug!("Querying {} with idx {:?}", subj, idx);
            match KeywordDetails::get(subj, chan, &self.pg)? {
                Some(kwd) => {
                    if let Some(mut idx) = idx {
                        if idx == -1 {
                            for i in 0..kwd.entries.len() {
                                self.cli.send_notice(tgt, kwd.format_entry(i+1).unwrap())?;
                            }
                        }
                        else {
                            if idx == 0 {
                                idx = 1;
                            }
                            if let Some(ent) = kwd.format_entry(idx as _) {
                                self.cli.send_notice(tgt, ent)?;
                            }
                            else {
                                let pluralised = if kwd.entries.len() == 1 {
                                    "entry"
                                }
                                else {
                                    "entries"
                                };
                                self.cli.send_notice(tgt, format!("\x02{}\x0f: only has \x02\x0304{}\x0f {}", subj, kwd.entries.len(), pluralised))?;
                            }
                        }
                    }
                    else {
                        if let Some(ent) = kwd.format_entry(1) {
                            self.cli.send_notice(tgt, ent)?;
                        }
                        else {
                            self.cli.send_notice(tgt, format!("\x02{}\x0f: no entries yet", subj))?;
                        }
                    }
                },
                None => {
                    self.cli.send_notice(tgt, format!("\x02{}\x0f: never heard of it", subj))?;
                }
            }
        }
        Ok(())
    }
    pub fn handle_msg(&mut self, m: Message) -> Result<(), Error> {
        if let Command::PRIVMSG(channel, message) = m.command {
            if let Some(src) = m.prefix {
                if let Err(e) = self.handle_privmsg(&src, &channel, &message) {
                    if let Some(nick) = src.split("!").next() {
                        self.report_error(nick, &channel, e)?;
                    }
                }
            }
        }
        Ok(())
    }
}
fn main() -> Result<(), Error> {
    println!("[+] loading configuration");
    let default_log_filter = "paroxysm=info".to_string();
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("paroxysm"))?;
    let cfg: Config = settings.try_into()?;
    let env = env_logger::Env::new().default_filter_or(cfg.log_filter.clone().unwrap_or(default_log_filter));
    env_logger::init_from_env(env);
    info!("paroxysm starting up");
    info!("connecting to database");
    let pg = PgConnection::establish(&cfg.database_url)?;
    info!("connecting to IRC");
    let cli = IrcClient::new(&cfg.irc_config_path)?;
    cli.identify()?;
    let st = cli.stream();
    let mut app = App { cli, pg, cfg };
    info!("running!");
    st.for_each_incoming(|m| {
        if let Err(e) = app.handle_msg(m) {
            warn!("Error processing message: {}", e);
        }
    })?;
    Ok(())
}
