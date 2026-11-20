// SPDX-License-Identifier: Apache-2.0
//! The address bar's text (`br2-address`): an address or a search (B09: DuckDuckGo), and the suggestions under it.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Where typed text goes: an address as typed (with https:// added), else a DuckDuckGo search.
pub fn parse(text: &str) -> String {
    let t = text.trim();
    if ["https://", "http://", "file:", "neo:"]
        .iter()
        .any(|s| t.starts_with(s))
    {
        return t.into();
    }
    let host = t.split('/').next().unwrap_or("");
    if !t.contains(char::is_whitespace)
        && (host.contains('.') || host == "localhost" || host.starts_with("localhost:"))
    {
        return format!("https://{t}");
    }
    search(t)
}

/// The web search for `text` (the "Search for …" row), whatever it looks like.
pub fn search(text: &str) -> String {
    format!("https://duckduckgo.com/?q={}", encode(text.trim()))
}

/// Percent-encoding for a query value: unreserved bytes stay, a space is '+'.
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A row under the address bar while editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suggestion {
    /// a page visited: (title, address)
    History(String, String),
    /// a bookmark: (title, address)
    Bookmark(String, String),
    /// the last row: search the web for the text
    Search(String),
}

/// The rows for `text`: up to 3 visited pages (newest first), then up to 2 bookmarks, whose title or address contains
/// it (any case), then "Search for …". `visited` and `bookmarks` are (title, address), oldest first.
pub fn suggest(
    text: &str,
    visited: &[(String, String)],
    bookmarks: &[(String, String)],
) -> Vec<Suggestion> {
    let t = text.trim().to_lowercase();
    let hit = |(title, url): &&(String, String)| {
        !t.is_empty() && (title.to_lowercase().contains(&t) || url.to_lowercase().contains(&t))
    };
    let mut out: Vec<Suggestion> = Vec::new();
    for (title, url) in visited.iter().rev().filter(hit) {
        if out.len() == 3 {
            break;
        }
        if !out
            .iter()
            .any(|s| matches!(s, Suggestion::History(_, u) if u == url))
        {
            out.push(Suggestion::History(title.clone(), url.clone()));
        }
    }
    out.extend(
        bookmarks
            .iter()
            .filter(hit)
            .take(2)
            .map(|(a, b)| Suggestion::Bookmark(a.clone(), b.clone())),
    );
    if !t.is_empty() {
        out.push(Suggestion::Search(text.trim().into()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_searches() {
        assert_eq!(parse("https://a.org/x"), "https://a.org/x");
        assert_eq!(
            parse(" en.wikipedia.org/wiki/EKA2 "),
            "https://en.wikipedia.org/wiki/EKA2"
        );
        assert_eq!(parse("localhost:8080"), "https://localhost:8080");
        assert_eq!(
            parse("symbian eka2"),
            "https://duckduckgo.com/?q=symbian+eka2"
        );
        assert_eq!(
            parse("c++ & rust"),
            "https://duckduckgo.com/?q=c%2B%2B+%26+rust"
        );
        assert_eq!(parse("neo:start"), "neo:start");
        assert_eq!(parse("file:///c:/pages/a.html"), "file:///c:/pages/a.html");
    }

    #[test]
    fn suggestions_are_history_then_bookmarks_then_search() {
        let s = |a: &str, b: &str| (String::from(a), String::from(b));
        let visited = [
            s("EKA2 – Wikipedia", "https://en.wikipedia.org/wiki/EKA2"),
            s("Slint", "https://slint.dev/docs"),
            s(
                "Symbian Foundation – Wikipedia",
                "https://en.wikipedia.org/wiki/Symbian_Foundation",
            ),
            s("EKA2 – Wikipedia", "https://en.wikipedia.org/wiki/EKA2"),
        ];
        let marks = [s("NeoSymbian", "https://github.com/NeoSymbian")];
        let got = suggest("wiki", &visited, &marks);
        assert_eq!(
            got,
            [
                Suggestion::History(visited[3].0.clone(), visited[3].1.clone()),
                Suggestion::History(visited[2].0.clone(), visited[2].1.clone()),
                Suggestion::Search("wiki".into()),
            ]
        );
        assert!(matches!(
            suggest("neosym", &visited, &marks)[0],
            Suggestion::Bookmark(..)
        ));
        assert!(suggest("  ", &visited, &marks).is_empty());
    }
}
