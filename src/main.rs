use std::{
    collections::HashMap,
    thread,
    time::{Duration, Instant},
};

use clap::{self, Parser};
use dyn_fmt::AsStrFormatExt;
use jiff;
use preferences::{self as prf, Preferences};
use reqwest as rw;
use scraper as sc;
use strum_macros as sm;

const APP_INFO: prf::AppInfo = prf::AppInfo {
    name: "wiki-path",
    author: "TommasoTricker",
};

const HOUR_SECS: u32 = 3600;

const ANON_RATE_LIMIT: u32 = 500; // https://api.wikimedia.org/wiki/Rate_limits#Anonymous_requests
const API_RATE_LIMIT: u32 = 5000; // https://api.wikimedia.org/wiki/Rate_limits#Personal_requests

const ANON_URL_FORMAT: &str = "https://en.wikipedia.org/wiki/{}";
const API_URL_FORMAT: &str = "https://en.wikipedia.org/w/rest.php/v1/page/{}/html";

const ANON_PREFIX: &str = "/wiki/";
const API_PREFIX: &str = "./";

const DEFAULT_MAX_DEPTH: u32 = 25;

#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Find paths from one article to another
    Path {
        start: String,
        end: String,

        /// Print article name and depth for each searched article
        #[arg(short, long)]
        verbose: bool,

        /// Maximum depth to search
        #[arg(short = 'd', long, value_name = "DEPTH", default_value_t = DEFAULT_MAX_DEPTH)]
        max_depth: u32,

        /// Find all paths up to DEPTH
        #[arg(short, long)]
        all: bool,

        /// Search articles in the "External links" section
        #[arg(short, long)]
        external: bool,
    },

    /// Manage Wikimedia API Token that decreases request wait from 7.2s to 0.72s (https://api.wikimedia.org/wiki/Authentication#Personal_API_tokens)
    Token {
        /// Token
        #[arg(short, long)]
        token: Option<String>,

        /// Unset
        #[arg(short, long)]
        unset: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Path {
            start,
            end,
            verbose,
            all,
            max_depth,
            external,
        } => cmd_path(start, end, verbose, all, max_depth, external),
        Command::Token { token, unset } => cmd_token(token, unset),
    }
}

fn cmd_path(start: String, end: String, verbose: bool, all: bool, max_depth: u32, external: bool) {
    let token = match load_token() {
        Ok(token) => token,
        Err(err) => {
            eprintln!("Error loading preferences: {}", err);
            None
        }
    };

    let start_time = Instant::now();

    let (rate_limit, prefix, url_format) = match token {
        Some(_) => (API_RATE_LIMIT, API_PREFIX, API_URL_FORMAT),
        None => (ANON_RATE_LIMIT, ANON_PREFIX, ANON_URL_FORMAT),
    };

    let req_wait = Duration::from_secs_f32(HOUR_SECS as f32 / rate_limit as f32);
    let mut prev_req = Instant::now()
        .checked_sub(req_wait)
        .unwrap_or_else(|| Instant::now());

    let mut articles = vec![String::new(), start];
    let mut article_parent = HashMap::from([(1, 0)]);

    let mut curr_idx = 0;
    let mut level_len;
    let mut next_level_len = 1;

    for depth in 0..(max_depth + 1) {
        level_len = next_level_len;
        next_level_len = 0;

        let end_idx = curr_idx + level_len;
        while curr_idx < end_idx {
            curr_idx += 1;

            let article = &articles[curr_idx];

            if verbose {
                println!("{} {}", article, depth);
            }

            // Build request
            let url = url_format.format(&[&article]);

            let client = rw::blocking::Client::new();
            let mut request = client.get(&url);

            if let Some(token) = token.as_ref() {
                request = request.header(rw::header::AUTHORIZATION, format!("Bearer {}", token));
            }

            // Rate-limit
            let elapsed = prev_req.elapsed();
            if elapsed < req_wait {
                thread::sleep(req_wait - elapsed);
            }
            prev_req = Instant::now();

            // Send request
            let res = match request.send() {
                Ok(res) => res,
                Err(err) => {
                    eprintln!("Error GET-ing {}: {}", url, err);
                    continue;
                }
            };

            let body = match res.text() {
                Ok(body) => body,
                Err(err) => {
                    eprintln!("Error reading response body of {}: {}", url, err);
                    continue;
                }
            };

            let document = sc::Html::parse_document(&body);
            let selector = sc::Selector::parse("*").unwrap();

            for element in document.select(&selector) {
                // External_links is OP
                if !external && element.value().id() == Some("External_links") {
                    break;
                }

                if element.value().name() == "a" {
                    if let Some(href) = element.value().attr("href") {
                        if let Some(mut name) = href.strip_prefix(prefix) {
                            // Remove #fragments
                            if let Some(idx) = name.find('#') {
                                name = &name[..idx];
                            }
                            // Exclude "Main_Page" or Special: / Talk: etc
                            if name != "Main_Page" && !name.contains(':') {
                                let new_article = name.to_string();

                                if !articles.contains(&new_article) {
                                    articles.push(new_article);
                                    article_parent.insert(articles.len() - 1, curr_idx);

                                    next_level_len += 1;

                                    if name == end {
                                        let elapsed = start_time.elapsed();

                                        let mut path = Vec::new();

                                        let mut current = articles.len() - 1;
                                        while current != 0 {
                                            path.push(&articles[current]);
                                            current = article_parent[&current];
                                        }

                                        path.reverse();

                                        println!("Path: {:?}", path);
                                        println!("Length: {}", path.len());

                                        let elapsed_sdur = jiff::SignedDuration::from_secs_f64(
                                            elapsed.as_secs_f64(),
                                        );
                                        println!("Took {elapsed_sdur:#}");

                                        if !all {
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(sm::EnumString, sm::AsRefStr)]
enum Prefs {
    #[strum(serialize = "token")]
    Token,
}

fn cmd_token(token: Option<String>, unset: bool) {
    let mut config: prf::PreferencesMap<String> = prf::PreferencesMap::new();

    if unset {
        config.remove(Prefs::Token.as_ref());
    } else if let Some(token) = token {
        config.insert(Prefs::Token.as_ref().into(), token.into());
    } else {
        match load_token() {
            Ok(opt_token) => {
                if let Some(token) = opt_token {
                    println!("{}", token);
                }
            }
            Err(err) => {
                eprintln!("Error loading preferences: {}", err);
            }
        };
    }

    if let Err(err) = config.save(&APP_INFO, "config") {
        eprintln!("Error saving configuration: {}", err);
    }
}

fn load_token() -> Result<Option<String>, prf::PreferencesError> {
    match prf::PreferencesMap::<String>::load(&APP_INFO, "config") {
        Ok(mut config) => Ok(config.remove(Prefs::Token.as_ref())),
        Err(err) => Err(err),
    }
}
