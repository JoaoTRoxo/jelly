//! Completing the command line in bash, zsh and fish. `jelly completions
//! SHELL` prints a script that asks `jelly __complete WORDS…` what can come
//! next, so that movies and shows complete from the server too: `jelly play
//! fri<Tab>` gives `jelly play Frieren: Beyond Journey's End`.

use crate::api;
use std::io::Write;
use std::time::Duration;

const BASH: &str = include_str!("../data/completions/jelly.bash");
const ZSH: &str = include_str!("../data/completions/_jelly");
const FISH: &str = include_str!("../data/completions/jelly.fish");

const SHELLS: &[(&str, &str)] = &[("bash", ""), ("zsh", ""), ("fish", "")];

const JSON: &[(&str, &str)] = &[("--json", "Print JSON")];

const COMMANDS: &[(&str, &str)] = &[
    ("continue", "Play what you were watching"),
    ("play", "Play a movie or episode, or a show's next episode"),
    ("open", "Show a movie's or show's page"),
    ("search", "List what matches some text"),
    ("list", "List what's in progress, next up, new or requested"),
    ("completions", "Print a completion script for a shell"),
    ("help", "Show the commands"),
    ("version", "Show Jelly's version"),
];

const LISTS: &[(&str, &str)] = &[
    ("continue", "What's in progress"),
    ("next-up", "The next episodes of the shows you're watching"),
    ("latest", "What's new"),
    ("requests", "What's been requested through Seerr"),
];

/// How long a title's completion may wait for the server
const SERVER_LIMIT: Duration = Duration::from_secs(3);

pub fn script(shell: &str) -> Option<&'static str> {
    match shell {
        "bash" => Some(BASH),
        "zsh" => Some(ZSH),
        "fish" => Some(FISH),
        _ => None,
    }
}

/// Print what can take the place of the last of `words` (everything after
/// `jelly`, the last one being typed): "word<TAB>description" lines
pub fn print(words: &[String]) {
    let mut out = std::io::stdout().lock();
    for (word, description) in candidates(words) {
        if writeln!(out, "{word}\t{description}").is_err() {
            return;
        }
    }
}

fn candidates(words: &[String]) -> Vec<(String, String)> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let Some((&current, before)) = words.split_last() else {
        return fixed(COMMANDS, "");
    };
    match before {
        [] => fixed(COMMANDS, current),
        ["list"] => fixed(LISTS, current),
        ["completions"] => fixed(SHELLS, current),
        ["list", _] => fixed(JSON, current),
        ["search", _, ..] if current.starts_with('-') => fixed(JSON, current),
        ["play" | "open", typed @ ..] => titles(typed, current),
        _ => Vec::new(),
    }
}

fn fixed(choices: &[(&str, &str)], current: &str) -> Vec<(String, String)> {
    choices
        .iter()
        .filter(|(word, _)| word.starts_with(current))
        .map(|(word, description)| (word.to_string(), description.to_string()))
        .collect()
}

/// Movies and shows whose names start with what's typed: from where `current`
/// is, as the words before it are typed already. Nothing typed yet, what
/// you're watching.
fn titles(typed: &[&str], current: &str) -> Vec<(String, String)> {
    let Ok(client) = crate::cli::client() else {
        return Vec::new();
    };
    let client = client.time_limit(SERVER_LIMIT);
    let query = typed
        .iter()
        .chain([&current])
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    let found = if query.trim().is_empty() {
        watching(&client)
    } else {
        client
            .search_titles(query.trim())
            .unwrap_or_default()
            .iter()
            .filter_map(|item| Some((item.name.clone(), kind(item)?)))
            .collect()
    };
    complete_names(found, typed.len(), &query)
}

/// The shows and movies in progress or next up
fn watching(client: &api::Client) -> Vec<(String, String)> {
    let resume = client.resume().unwrap_or_default();
    let next_up = client.next_up(None).unwrap_or_default();
    let mut found: Vec<(String, String)> = Vec::new();
    for (item, description) in resume
        .iter()
        .map(|i| (i, "in progress"))
        .chain(next_up.iter().map(|i| (i, "next up")))
    {
        let name = match item.kind.as_str() {
            "Episode" => item.series_name.clone().unwrap_or_default(),
            _ => item.name.clone(),
        };
        if !name.is_empty() && !found.iter().any(|(n, _)| *n == name) {
            found.push((name, description.to_string()));
        }
    }
    found
}

/// "show", or a movie's year: episodes are left out, as their names don't say
/// which show they're from
fn kind(item: &api::Item) -> Option<String> {
    match item.kind.as_str() {
        "Series" => Some("show".into()),
        "Movie" => Some(
            item.production_year
                .map_or_else(|| "movie".to_string(), |y| y.to_string()),
        ),
        _ => None,
    }
}

/// The names that start with `query` (any case), from their word number
/// `skip` on, as that's where the word being completed starts
fn complete_names(found: Vec<(String, String)>, skip: usize, query: &str) -> Vec<(String, String)> {
    let query = query.to_lowercase();
    let mut names: Vec<(String, String)> = Vec::new();
    for (name, description) in found {
        if !name.to_lowercase().starts_with(&query) {
            continue;
        }
        let rest = name.split(' ').skip(skip).collect::<Vec<_>>().join(" ");
        if !rest.is_empty() && !names.iter().any(|(n, _)| *n == rest) {
            names.push((rest, description));
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The last word is the one being typed, maybe empty
    fn words(line: &str) -> Vec<String> {
        line.split(' ').map(String::from).collect()
    }

    fn first(found: Vec<(String, String)>) -> Vec<String> {
        found.into_iter().map(|(word, _)| word).collect()
    }

    #[test]
    fn completes_commands_and_lists() {
        assert_eq!(first(candidates(&words("pl"))), ["play"]);
        assert_eq!(first(candidates(&words(""))).len(), COMMANDS.len());
        assert_eq!(first(candidates(&words("list n"))), ["next-up"]);
        assert_eq!(first(candidates(&words("list latest "))), ["--json"]);
        assert_eq!(first(candidates(&words("search dune --"))), ["--json"]);
        assert!(candidates(&words("search dune ")).is_empty());
        assert_eq!(first(candidates(&words("completions z"))), ["zsh"]);
        assert!(candidates(&words("version ")).is_empty());
    }

    #[test]
    fn completes_names_from_where_the_word_starts() {
        let found = || {
            vec![
                ("La La Land".to_string(), "2016".to_string()),
                ("Labyrinth".to_string(), "1986".to_string()),
                (
                    "Frieren: Beyond Journey's End".to_string(),
                    "show".to_string(),
                ),
            ]
        };
        assert_eq!(
            first(complete_names(found(), 0, "la")),
            ["La La Land", "Labyrinth"]
        );
        assert_eq!(first(complete_names(found(), 1, "la la")), ["La Land"]);
        assert_eq!(first(complete_names(found(), 2, "la la ")), ["Land"]);
        assert_eq!(
            first(complete_names(found(), 0, "FRI")),
            ["Frieren: Beyond Journey's End"]
        );
        assert!(complete_names(found(), 0, "dune").is_empty());
    }
}
