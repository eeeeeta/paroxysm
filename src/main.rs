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
        self.cli.send_privmsg(chan, format!("{}: \x0304Error:\x0f {}", nick, msg))?;
        Ok(())
    }
    pub fn handle_privmsg(&mut self, from: &str, chan: &str, msg: &str) -> Result<(), Error> {
        lazy_static! {
            static ref LEARN_RE: Regex = Regex::new(r#"^\?\?(?P<gen>!)?\s*(?P<subj>.*):\s*(?P<val>.*)"#).unwrap();
            static ref QUERY_RE: Regex = Regex::new(r#"^\?\?\s*(?P<subj>[^\[]*)(?P<idx>\[[^\]]+\])?"#).unwrap();
        }
        let nick = from.split("!").next().ok_or(format_err!("Invalid source"))?;
        if let Some(learn) = LEARN_RE.captures(msg) {
            let subj = &learn["subj"];
            let val = &learn["val"];
            let learn_chan = if learn.name("gen").is_some() {
                "*"
            }
            else {
                chan
            };
            debug!("Learning {}: {}", subj, val);
            let mut kwd = KeywordDetails::get_or_create(subj, learn_chan, &self.pg)?;
            if kwd.keyword.chan == "*" && !self.cfg.admins.contains(nick) {
                self.report_error(nick, chan, "Only administrators can create or modify general entries.")?;
                return Ok(());
            }
            let idx = kwd.learn(nick, val, &self.pg)?;
            self.cli.send_notice(chan, kwd.format_entry(idx).unwrap())?;
        }
        else if let Some(query) = QUERY_RE.captures(msg) {
            let subj = &query["subj"];
            let idx = match query.name("idx") {
                Some(i) => {
                    let i = i.as_str();
                    if let Some(x) = i.get(1..(i.len()-1)) {
                        match x {
                            "*" => Some(-1),
                            x => x.parse::<i32>().ok(),
                        }
                    }
                    else {
                        None
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
                                self.cli.send_notice(chan, kwd.format_entry(i+1).unwrap())?;
                            }
                        }
                        else {
                            if idx == 0 {
                                idx = 1;
                            }
                            if let Some(ent) = kwd.format_entry(idx as _) {
                                self.cli.send_notice(chan, ent)?;
                            }
                            else {
                                let pluralised = if kwd.entries.len() == 1 {
                                    "entry"
                                }
                                else {
                                    "entries"
                                };
                                self.cli.send_notice(chan, format!("\x02{}\x0f: only has \x02\x0304{}\x0f {}", subj, kwd.entries.len(), pluralised))?;
                            }
                        }
                    }
                    else {
                        if let Some(ent) = kwd.format_entry(1) {
                            self.cli.send_notice(chan, ent)?;
                        }
                        else {
                            self.cli.send_notice(chan, format!("\x02{}\x0f: no entries yet", subj))?;
                        }
                    }
                },
                None => {
                    self.cli.send_notice(chan, format!("\x02{}\x0f: never heard of it", subj))?;
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
