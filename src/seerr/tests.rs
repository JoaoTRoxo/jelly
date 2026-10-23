use super::*;

#[test]
fn finds_seerr_next_to_jellyfin() {
    assert_eq!(
        default_url("http://192.0.2.7:8096"),
        "http://192.0.2.7:5055"
    );
    assert_eq!(
        default_url("https://media.example.com/jellyfin"),
        "http://media.example.com:5055"
    );
    assert_eq!(default_url("http://[::1]:8096"), "http://[::1]:5055");
}

#[test]
fn looks_next_to_a_domain() {
    let found = candidates("https://stream.example.org");
    assert_eq!(
        found.first().map(String::as_str),
        Some("https://requests.example.org")
    );
    assert_eq!(
        found.last().map(String::as_str),
        Some("http://stream.example.org:5055")
    );
    assert_eq!(
        candidates("https://example.com")[0],
        "https://requests.example.com"
    );
    assert_eq!(
        candidates("http://192.0.2.7:8096"),
        vec!["http://192.0.2.7:5055"]
    );
}

#[test]
fn adds_up_downloads() {
    let json = r#"{"tmdbId":1,"mediaType":"tv","status":4,"downloadStatus":[
        {"size":1000,"sizeLeft":250,"status":"downloading","estimatedCompletionTime":"2026-10-22T03:10:00Z"},
        {"size":1000,"sizeLeft":1000,"status":"queued","estimatedCompletionTime":"2026-10-22T03:40:00Z"}]}"#;
    let media: MediaInfo = serde_json::from_str(json).unwrap();
    assert_eq!(media.download_progress(), Some(0.375));
    assert_eq!(media.download_done_at(), Some("2026-10-22T03:40:00Z"));
    assert_eq!(media.download_state(), Some("downloading"));
    assert_eq!(MediaInfo::default().download_progress(), None);
}

#[test]
fn says_how_far_along() {
    let underway = |awaiting_approval: bool, status: i32, downloads: Vec<Download>| Underway {
        found: Found {
            media_info: Some(MediaInfo {
                status,
                download_status: downloads,
                ..Default::default()
            }),
            ..Default::default()
        },
        awaiting_approval,
    };
    let paused = Download {
        size: 4.0,
        size_left: 3.0,
        status: "paused".into(),
        ..Default::default()
    };
    assert_eq!(
        underway(true, PENDING, Vec::new()).progress(),
        "Waiting for approval"
    );
    assert_eq!(
        underway(false, PROCESSING, Vec::new()).progress(),
        "Processing"
    );
    assert_eq!(
        underway(false, PROCESSING, vec![paused]).progress(),
        "Paused · 25%"
    );
    assert_eq!(format_minutes(12), "12 min");
    assert_eq!(format_minutes(65), "1 h 5 min");
    assert_eq!(format_minutes(3 * 24 * 60 + 120), "3 d 2 h");
}

#[test]
fn knows_whats_on_its_way() {
    let media = |status: i32, downloading: bool| MediaInfo {
        status,
        download_status: if downloading {
            vec![Download::default()]
        } else {
            Vec::new()
        },
        ..Default::default()
    };
    assert!(
        on_its_way(1, &media(PENDING, false)),
        "waiting for approval"
    );
    assert!(
        on_its_way(2, &media(PROCESSING, false)),
        "approved, not there yet"
    );
    assert!(
        !on_its_way(2, &media(PARTLY_AVAILABLE, false)),
        "airing, nothing coming now"
    );
    assert!(
        on_its_way(2, &media(PARTLY_AVAILABLE, true)),
        "airing, an episode downloading"
    );
    assert!(!on_its_way(5, &media(AVAILABLE, false)), "done");
    assert!(!on_its_way(3, &media(PENDING, false)), "declined");
}

#[test]
fn encodes_queries() {
    assert_eq!(encode("dune part two"), "dune%20part%20two");
    assert_eq!(encode("amélie & co"), "am%C3%A9lie%20%26%20co");
}

#[test]
fn reads_search_results() {
    let json = r#"{"page":1,"results":[
        {"id":693134,"mediaType":"movie","title":"Dune: Part Two","releaseDate":"2024-02-27","posterPath":"/a.jpg","mediaInfo":{"status":5}},
        {"id":1396,"mediaType":"tv","name":"Breaking Bad","firstAirDate":"2008-01-20","posterPath":null,"mediaInfo":null},
        {"id":17419,"mediaType":"person","name":"Bryan Cranston"}]}"#;
    #[derive(Deserialize)]
    struct Page {
        results: Vec<Found>,
    }
    let page: Page = serde_json::from_str(json).unwrap();
    let (movie, show) = (&page.results[0], &page.results[1]);
    assert_eq!(
        (movie.title(), movie.year(), movie.status()),
        ("Dune: Part Two", Some("2024"), AVAILABLE)
    );
    assert_eq!(
        movie.poster_url().as_deref(),
        Some("https://image.tmdb.org/t/p/w342/a.jpg")
    );
    assert_eq!(
        (show.title(), show.year(), show.status(), show.is_show()),
        ("Breaking Bad", Some("2008"), 0, true)
    );
    assert_eq!(show.poster_url(), None);
}

#[test]
fn names_seasons() {
    let season = |number: i32, name: Option<&str>| Season {
        season_number: number,
        name: name.map(String::from),
        episode_count: None,
    };
    assert_eq!(season(2, Some("2")).title(), "Season 2");
    assert_eq!(season(2, None).title(), "Season 2");
    assert_eq!(season(2, Some("")).title(), "Season 2");
    assert_eq!(
        season(4, Some("The Final Season")).title(),
        "The Final Season"
    );
}

#[test]
fn knows_which_seasons_are_taken() {
    let json = r#"{"name":"Severance","seasons":[
        {"seasonNumber":0,"name":"Specials"},{"seasonNumber":1,"episodeCount":9},
        {"seasonNumber":2,"episodeCount":10},{"seasonNumber":3,"episodeCount":10}],
        "mediaInfo":{"status":4,"seasons":[{"seasonNumber":1,"status":5}],
        "requests":[{"status":1,"seasons":[{"seasonNumber":2,"status":1}]},{"status":3,"seasons":[{"seasonNumber":3,"status":1}]}]}}"#;
    let mut show: Details = serde_json::from_str(json).unwrap();
    show.media_type = "tv".into();
    assert_eq!(show.season_taken(1), Some("Available"));
    assert_eq!(show.season_taken(2), Some("Requested"));
    // A declined request leaves it free
    assert_eq!(show.season_taken(3), None);
    assert_eq!(show.requestable(), vec![3]);
    assert!(show.can_request());
    assert_eq!(show.state(), "Waiting for approval");
}

#[test]
fn numbers_the_next_episode_only_when_it_fits() {
    let season = |n: i32, count: i32| Season {
        season_number: n,
        name: None,
        episode_count: Some(count),
    };
    let show = |number: (i32, i32)| Details {
        seasons: vec![season(1, 24), season(2, 24), season(3, 24)],
        next_episode_to_air: Some(NextEpisode {
            air_date: None,
            season_number: number.0,
            episode_number: number.1,
        }),
        ..Default::default()
    };
    assert_eq!(show((3, 4)).next_episode_number().as_deref(), Some("S3 E4"));
    // TMDB's one long season, with seasons from TheTVDB
    assert_eq!(show((1, 52)).next_episode_number(), None);
}

#[test]
fn says_where_a_movie_is_at() {
    let movie = |status: i32, requests: Vec<Request>, downloads: Vec<Download>| Details {
        media_type: "movie".into(),
        media_info: Some(MediaInfo {
            status,
            requests,
            download_status: downloads,
            ..Default::default()
        }),
        ..Default::default()
    };
    let approved = || {
        vec![Request {
            status: 2,
            seasons: Vec::new(),
        }]
    };
    let fresh = Details {
        media_type: "movie".into(),
        ..Default::default()
    };
    assert_eq!(
        (fresh.state().as_str(), fresh.can_request()),
        ("Not requested yet", true)
    );
    assert_eq!(
        movie(PROCESSING, approved(), Vec::new()).state(),
        "Processing"
    );
    let halfway = Download {
        size: 2.0,
        size_left: 1.0,
        status: "warning".into(),
        ..Default::default()
    };
    assert_eq!(
        movie(PROCESSING, approved(), vec![halfway.clone()]).state(),
        "Stalled · 50%"
    );
    assert!(!movie(PROCESSING, approved(), Vec::new()).can_request());
    assert_eq!(
        movie(AVAILABLE, approved(), Vec::new()).state(),
        "In your library"
    );
    let episode = Download {
        episode: Some(DownloadEpisode {
            season_number: 3,
            episode_number: 1,
            title: Some("The Maid".into()),
        }),
        ..halfway
    };
    assert_eq!(episode.episode_name().as_deref(), Some("S3 E1 · The Maid"));
}
