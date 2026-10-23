//! The command line. `search` and `list` print and exit; the other commands
//! act in the app's window (the running app's, when there is one), so they
//! suit keybindings and launchers.

use crate::{api, completion, seerr, session, ui};
use gtk::glib;
use serde_json::{Value, json};
use std::io::Write;
use std::rc::Rc;

pub const USAGE: &str = "\
Usage: jelly [COMMAND]

Without a command, opens the window. Commands:

  continue           Play what you were watching: from where you stopped, or
                     the next episode of the show you finished one of
  play NAME|ID       Play a movie or episode, or a show's next episode
  open NAME|ID       Show a movie's or show's page
  search TEXT        List the movies, shows and episodes matching TEXT
  list continue      List what's in progress
  list next-up       List the next episodes of the shows you're watching
  list latest        List what's new
  list requests      List what's been requested through Seerr and isn't in
                     the library yet, and how its download is going
  completions SHELL  Print the completion script for bash, zsh or fish
  help               Show this
  version            Show Jelly's version

search and list print 'description<TAB>ID' lines, or JSON with --json.
continue, play and open use the open window, if there is one.
For development: --play FILE plays a local file.";

pub enum Command {
    Window,
    Continue,
    /// An item ID or a name
    Play(String),
    Open(String),
    PlayFile(String),
    Search {
        text: String,
        json: bool,
    },
    List {
        what: List,
        json: bool,
    },
    /// A shell's completion script
    Completions(String),
    /// What can come next on a command line (for the completion scripts)
    Complete(Vec<String>),
    Help,
    Version,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum List {
    Continue,
    NextUp,
    Latest,
    Requests,
}

/// `args`: without the program's name
pub fn parse(args: &[String]) -> Result<Command, String> {
    // As typed, --json and all
    if let Some(("__complete", words)) = args.split_first().map(|(a, w)| (a.as_str(), w)) {
        return Ok(Command::Complete(words.to_vec()));
    }
    let json = args.iter().any(|a| a == "--json");
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--json")
        .collect();
    let rest = |words: &[&str]| words.join(" ");
    match words.as_slice() {
        [] => Ok(Command::Window),
        ["help" | "--help" | "-h"] => Ok(Command::Help),
        ["version" | "--version" | "-V"] => Ok(Command::Version),
        ["continue"] => Ok(Command::Continue),
        ["play", query @ ..] if !query.is_empty() => Ok(Command::Play(rest(query))),
        ["open", query @ ..] if !query.is_empty() => Ok(Command::Open(rest(query))),
        ["--play", file] => Ok(Command::PlayFile(file.to_string())),
        ["completions", shell] => match completion::script(shell) {
            Some(_) => Ok(Command::Completions(shell.to_string())),
            None => Err(format!(
                "Can't complete for “{shell}”: try bash, zsh or fish"
            )),
        },
        ["search", text @ ..] if !text.is_empty() => Ok(Command::Search {
            text: rest(text),
            json,
        }),
        ["list", what] => {
            let what = match *what {
                "continue" => List::Continue,
                "next-up" => List::NextUp,
                "latest" => List::Latest,
                "requests" => List::Requests,
                other => {
                    return Err(format!(
                        "Can't list “{other}”: try continue, next-up, latest or requests"
                    ));
                }
            };
            Ok(Command::List { what, json })
        }
        _ => Err(format!("Unknown command: {}", args.join(" "))),
    }
}

/// Carry out a command that only prints, if it is one, giving the exit code
pub fn print(command: &Command) -> Option<glib::ExitCode> {
    let result = match command {
        Command::Help => {
            let _ = writeln!(std::io::stdout(), "{USAGE}");
            Ok(())
        }
        Command::Version => {
            let _ = writeln!(std::io::stdout(), "jelly {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Completions(shell) => {
            let script = completion::script(shell).unwrap_or_default();
            let _ = write!(std::io::stdout(), "{script}");
            Ok(())
        }
        Command::Complete(words) => {
            completion::print(words);
            Ok(())
        }
        Command::Search { text, json } => client()
            .and_then(|c| c.search(text).map_err(|e| e.to_string()))
            .map(|items| output(&items, *json)),
        Command::List {
            what: List::Requests,
            json,
        } => requests().map(|underway| output_requests(&underway, *json)),
        Command::List { what, json } => client()
            .and_then(|c| list(&c, *what).map_err(|e| e.to_string()))
            .map(|items| output(&items, *json)),
        _ => return None,
    };
    Some(match result {
        Ok(()) => glib::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("jelly: {e}");
            glib::ExitCode::FAILURE
        }
    })
}

/// A client with the app's saved sign-in
pub fn client() -> Result<api::Client, String> {
    let s =
        session::load().ok_or_else(|| "Not signed in: open Jelly and sign in first".to_string())?;
    Ok(api::Client::new(&s.server, &session::device_id()).signed_in(&s.token, &s.user_id))
}

/// What's been requested and isn't in the library yet, and Seerr's address
fn requests() -> Result<(Vec<seerr::Underway>, String), String> {
    let s = session::load_seerr().ok_or_else(|| {
        "Not signed in to Seerr: sign in on the app's search page first".to_string()
    })?;
    let underway = seerr::Client::new(&s.url, &s.cookie)
        .underway()
        .map_err(|e| e.to_string())?;
    Ok((underway, s.url))
}

fn output_requests((underway, seerr): &(Vec<seerr::Underway>, String), json: bool) {
    let mut out = std::io::stdout().lock();
    if json {
        let items: Vec<Value> = underway
            .iter()
            .map(|u| {
                let media = u.found.media_info.clone().unwrap_or_default();
                json!({
                    "tmdb_id": u.found.id,
                    "type": u.found.media_type,
                    "name": u.found.title(),
                    "year": u.found.year(),
                    "progress": media.download_progress(),
                    "status": u.progress(),
                    "awaiting_approval": u.awaiting_approval,
                    "url": u.url(seerr),
                })
            })
            .collect();
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&items).unwrap_or_default()
        );
    } else {
        for u in underway {
            let name = match u.found.year() {
                Some(year) => format!("{} ({year})", u.found.title()),
                None => u.found.title().to_string(),
            };
            if writeln!(out, "{name} · {}\t{}", u.progress(), u.url(seerr)).is_err() {
                return;
            }
        }
    }
}

fn list(client: &api::Client, what: List) -> api::Result<Vec<api::Item>> {
    match what {
        List::Continue => client.resume(),
        List::NextUp => client.next_up(None),
        List::Requests => Ok(Vec::new()),
        List::Latest => {
            let mut items = Vec::new();
            for view in client.views()? {
                if matches!(
                    view.collection_type.as_deref(),
                    Some("movies" | "tvshows") | None
                ) {
                    items.extend(client.latest(&view.id)?);
                }
            }
            Ok(items)
        }
    }
}

fn output(items: &[api::Item], json: bool) {
    let mut out = std::io::stdout().lock();
    // A reader that stops early (`| head`) just ends the listing
    if json {
        let items: Vec<Value> = items.iter().map(to_json).collect();
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&items).unwrap_or_default()
        );
    } else {
        for item in items {
            if writeln!(out, "{}\t{}", describe(item), item.id).is_err() {
                return;
            }
        }
    }
}

/// "The O.C. · S1:E25 · The Shower · 45% watched", "La La Land (2016)"
fn describe(item: &api::Item) -> String {
    let mut parts = match item.kind.as_str() {
        "Episode" => vec![
            item.series_name.clone().unwrap_or_default(),
            item.episode_code().unwrap_or_default(),
            item.name.clone(),
        ],
        "Season" => vec![
            item.series_name.clone().unwrap_or_default(),
            item.name.clone(),
        ],
        _ => vec![match item.production_year {
            Some(year) => format!("{} ({year})", item.name),
            None => item.name.clone(),
        }],
    };
    if item.kind == "Series" {
        parts.push("show".into());
    }
    if item.progress() > 0.0 && !item.played() {
        parts.push(format!("{:.0}% watched", item.progress() * 100.0));
    }
    parts.retain(|p| !p.is_empty());
    parts.join(" · ")
}

fn to_json(item: &api::Item) -> Value {
    json!({
        "id": item.id,
        "type": item.kind,
        "name": item.name,
        "label": describe(item),
        "series": item.series_name,
        "episode": item.episode_code(),
        "year": item.production_year,
        "resume_seconds": item.resume_seconds(),
        "played": item.played(),
    })
}

/// Carry out a command in the window; `done` gets what went wrong, if anything
pub fn act(ctx: &Rc<ui::Ctx>, command: Command, done: impl FnOnce(Result<(), String>) + 'static) {
    if matches!(
        command,
        Command::Continue | Command::Play(_) | Command::Open(_)
    ) && ctx.client().is_none()
    {
        return done(Err("Not signed in: sign in first".into()));
    }
    let c = ctx.clone();
    match command {
        Command::Continue => resolve(
            ctx,
            |client| client.what_to_continue(),
            "Nothing to continue".into(),
            move |item| ui::play(&c, &item, false),
            done,
        ),
        Command::Play(query) => {
            let not_found = format!("Nothing found for “{query}”");
            let then = move |(item, play)| {
                if play {
                    ui::play(&c, &item, false)
                } else {
                    ui::open(&c, &item)
                }
            };
            resolve(
                ctx,
                move |client| playable(&client, &query),
                not_found,
                then,
                done,
            );
        }
        Command::Open(query) => {
            let not_found = format!("Nothing found for “{query}”");
            resolve(
                ctx,
                move |client| find(&client, &query),
                not_found,
                move |item| ui::open(&c, &item),
                done,
            );
        }
        Command::PlayFile(file) => {
            ui::player::open(ctx, ui::player::Media::File(file));
            done(Ok(()));
        }
        Command::Window
        | Command::Completions(_)
        | Command::Complete(_)
        | Command::Help
        | Command::Version
        | Command::Search { .. }
        | Command::List { .. } => done(Ok(())),
    }
}

/// Look something up, then act on it, or say why not
fn resolve<T: Send + 'static>(
    ctx: &Rc<ui::Ctx>,
    look_up: impl FnOnce(api::Client) -> api::Result<Option<T>> + Send + 'static,
    not_found: String,
    then: impl FnOnce(T) + 'static,
    done: impl FnOnce(Result<(), String>) + 'static,
) {
    ui::run_result(ctx, look_up, move |result| match result {
        Ok(Some(found)) => {
            then(found);
            done(Ok(()));
        }
        Ok(None) => done(Err(not_found)),
        Err(e) => done(Err(e.to_string())),
    });
}

/// What `play` does with `query`: play a movie or episode, or a show's next
/// episode (true); open a show with none left, or anything else (false)
fn playable(client: &api::Client, query: &str) -> api::Result<Option<(api::Item, bool)>> {
    let Some(item) = find(client, query)? else {
        return Ok(None);
    };
    Ok(Some(match item.kind.as_str() {
        "Movie" | "Episode" | "Video" => (item, true),
        "Series" => match client.next_up(Some(&item.id))?.into_iter().next() {
            Some(next) => (next, true),
            None => (item, false),
        },
        _ => (item, false),
    }))
}

/// An item by its ID, or the best match for a name
fn find(client: &api::Client, query: &str) -> api::Result<Option<api::Item>> {
    if query.len() == 32 && query.chars().all(|c| c.is_ascii_hexdigit()) {
        return client.item(query).map(Some);
    }
    Ok(best_match(client.search(query)?, query))
}

/// The closest name (the same, then starting with it, then containing it),
/// movies and shows before episodes, else the server's order
fn best_match(items: Vec<api::Item>, query: &str) -> Option<api::Item> {
    let query = query.to_lowercase();
    let closeness = |name: &str| {
        let name = name.to_lowercase();
        match () {
            _ if name == query => 0,
            _ if name.starts_with(&query) => 1,
            _ if name.contains(&query) => 2,
            _ => 3,
        }
    };
    items
        .into_iter()
        .enumerate()
        .min_by_key(|(i, item)| (closeness(&item.name), item.kind == "Episode", *i))
        .map(|(_, item)| item)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    fn item(kind: &str, name: &str) -> api::Item {
        api::Item {
            kind: kind.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn parses_commands() {
        assert!(matches!(parse(&args("")), Ok(Command::Window)));
        assert!(matches!(parse(&args("continue")), Ok(Command::Continue)));
        assert!(
            matches!(parse(&args("play la la land")), Ok(Command::Play(q)) if q == "la la land")
        );
        assert!(matches!(parse(&args("open abc")), Ok(Command::Open(q)) if q == "abc"));
        assert!(matches!(parse(&args("--version")), Ok(Command::Version)));
        assert!(parse(&args("--open abc")).is_err());
        assert!(
            matches!(parse(&args("--play /tmp/a.mkv")), Ok(Command::PlayFile(f)) if f == "/tmp/a.mkv")
        );
        assert!(matches!(
            parse(&args("list next-up --json")),
            Ok(Command::List {
                what: List::NextUp,
                json: true
            })
        ));
        assert!(matches!(
            parse(&args("search frieren")),
            Ok(Command::Search { json: false, .. })
        ));
        assert!(parse(&args("list everything")).is_err());
        assert!(
            matches!(parse(&args("completions zsh")), Ok(Command::Completions(s)) if s == "zsh")
        );
        assert!(parse(&args("completions tcsh")).is_err());
        assert!(
            matches!(parse(&args("__complete list latest --json")), Ok(Command::Complete(w)) if w.len() == 3)
        );
        assert!(parse(&args("play")).is_err());
        assert!(parse(&args("dance")).is_err());
    }

    #[test]
    fn prefers_shows_and_movies_to_episodes() {
        // As the server orders them for "frieren"
        let found = vec![
            item("Episode", "Frieren the Slayer"),
            item("Series", "Frieren: Beyond Journey's End"),
        ];
        assert_eq!(
            best_match(found, "frieren").map(|i| i.kind),
            Some("Series".into())
        );
    }

    #[test]
    fn prefers_the_same_name() {
        let found = vec![
            item("Movie", "La La Land Live"),
            item("Movie", "La La Land"),
        ];
        assert_eq!(
            best_match(found, "la la land").map(|i| i.name),
            Some("La La Land".into())
        );
        assert_eq!(best_match(Vec::new(), "anything").map(|i| i.name), None);
    }
}
