extern crate irc;
extern crate serde;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate diesel;
extern crate chrono;
extern crate config;
extern crate env_logger;
#[macro_use] extern crate log;
extern crate failure;
extern crate regex;
#[macro_use] extern crate lazy_static;

use irc::client::prelude::*;
use diesel::prelude::*;
use diesel::pg::PgConnection;
use failure::Error;
use regex::Regex;
use self::models::{Keyword, Entry, NewKeyword, NewEntry};

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
    pub fn get(word: &str, dbc: &PgConnection) -> Result<Option<Self>, Error> {
        let keyword = {
            use self::schema::keywords::dsl::*;
            keywords.filter(name.ilike(word))
                .first(dbc)
                .optional()?;
        };
        if let Some(k) = keyword {
            let entries = {
                use self::schema::entries::dsl::*;
                entries.filter(keyword_id.eq(keyword.id))
                    .get_result()?;
            };
        }
        else {
            Ok(None)
        }
        unimplemented!()
    }
}
impl App {
    pub fn handle_privmsg(&mut self, from: &str, chan: &str, msg: &str) -> Result<(), Error> {
        lazy_static! {
            static ref LEARN_RE: Regex = Regex::new(r#"\?\?\s*(.*):\s*(.*)"#).unwrap();
            static ref QUERY_RE: Regex = Regex::new(r#"\?\?\s*(.*)"#).unwrap();
        }
        if let Some(learn) = LEARN_RE.captures(msg) {
            let subj = &learn[1];
            let val = &learn[2];
            debug!("Learning {}: {}", subj, val);
        }
        else if let Some(query) = QUERY_RE.captures(msg) {
            let subj = &query[1];
            debug!("Querying {}", subj);
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
