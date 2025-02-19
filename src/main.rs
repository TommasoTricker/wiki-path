use std::{
    collections::HashMap,
    thread,
    time::{Duration, Instant},
};

use clap::{self, Parser};
use jiff;
use reqwest as rw;
use scraper as sc;

const DEFAULT_MAX_DEPTH: u32 = 25;

const REQ_WAIT_SECS: f32 = 0.5;

#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
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
}

fn main() {
    let c = Cli::parse();

    let start_time = Instant::now();

    let req_wait = Duration::from_secs_f32(REQ_WAIT_SECS);
    let mut prev_req = Instant::now()
        .checked_sub(req_wait)
        .unwrap_or_else(|| Instant::now());

    let mut articles = vec![String::new(), c.start];
    let mut article_parent = HashMap::from([(1, 0)]);

    let mut curr_idx = 0;
    let mut level_len;
    let mut next_level_len = 1;

    for depth in 0..(c.max_depth + 1) {
        level_len = next_level_len;
        next_level_len = 0;

        let end_idx = curr_idx + level_len;
        while curr_idx < end_idx {
            curr_idx += 1;

            let article = &articles[curr_idx];

            if c.verbose {
                println!("{} {}", article, depth);
            }

            // Build request
            let url = format!("https://en.wikipedia.org/wiki/{}", article);

            let client = rw::blocking::Client::new();
            let request = client.get(&url);

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
                    eprintln!("{}", err);
                    continue;
                }
            };

            let body = match res.text() {
                Ok(body) => body,
                Err(err) => {
                    eprintln!("{}", err);
                    continue;
                }
            };

            let document = sc::Html::parse_document(&body);
            let selector = sc::Selector::parse("a[href]").unwrap();

            for element in document.select(&selector) {
                if let Some(href) = element.value().attr("href") {
                    if let Some(mut name) = href.strip_prefix("/wiki/") {
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

                                if name == c.end {
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

                                    let elapsed_sdur =
                                        jiff::SignedDuration::from_secs_f64(elapsed.as_secs_f64());
                                    println!("Took {elapsed_sdur:#}");

                                    if !c.all {
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
