// SPDX-License-Identifier: Apache-2.0
//! The start page (`br2-start`, B06): eight shortcuts in a 4 × 2 grid, then Recently visited.

/// A shortcut tile: its letters, label, tile colour (0xRRGGBB) and address.
#[derive(Debug, Clone, Copy)]
pub struct Shortcut {
    pub letters: &'static str,
    pub label: &'static str,
    pub color: u32,
    pub url: &'static str,
}

/// The board's eight. shortcut: fixed until Edit shortcuts (UI4 keeps them in settings).
pub const SHORTCUTS: [Shortcut; 8] = [
    Shortcut {
        letters: "N",
        label: "NeoSymbian",
        color: 0x2f80ed,
        url: "https://github.com/NeoSymbian",
    },
    Shortcut {
        letters: "Nw",
        label: "News",
        color: 0xe5484d,
        url: "https://news.example.org/",
    },
    Shortcut {
        letters: "M",
        label: "Maps",
        color: 0x22b46a,
        url: "https://maps.example.org/",
    },
    Shortcut {
        letters: "W",
        label: "Wiki",
        color: 0x5b6472,
        url: "https://en.wikipedia.org/wiki/Symbian",
    },
    Shortcut {
        letters: "Wx",
        label: "Weather",
        color: 0x1fa9c4,
        url: "https://weather.example.org/",
    },
    Shortcut {
        letters: "@",
        label: "Mail",
        color: 0xf2994a,
        url: "https://mail.example.org/",
    },
    Shortcut {
        letters: "F",
        label: "Forum",
        color: 0x8656e8,
        url: "https://forum.example.org/",
    },
    Shortcut {
        letters: "S",
        label: "Store",
        color: 0xe2a92a,
        url: "https://store.notallow.cc/",
    },
];

/// Original addresses for the recent rows, so selecting one preserves the scheme and path.
pub fn recent_urls<'a>(
    visited: impl DoubleEndedIterator<Item = &'a str>,
) -> alloc::vec::Vec<&'a str> {
    let mut out: alloc::vec::Vec<&str> = alloc::vec::Vec::new();
    for url in visited.rev() {
        if url != crate::START && !out.iter().any(|old| recent_label(old) == recent_label(url)) {
            out.push(url);
        }
        if out.len() == 3 {
            break;
        }
    }
    out
}

/// Recently visited as the start page lists it: the address without its web scheme, newest first, at most 3.
pub fn recent<'a>(visited: impl DoubleEndedIterator<Item = &'a str>) -> alloc::vec::Vec<&'a str> {
    recent_urls(visited).into_iter().map(recent_label).collect()
}

fn recent_label(url: &str) -> &str {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    #[test]
    fn recent_is_newest_first_unique_and_short() {
        let v = [
            "neo:start",
            "https://a.org/",
            "https://b.org/x",
            "https://a.org/",
            "http://c.org",
            "https://d.org",
        ];
        assert_eq!(
            super::recent(v.iter().copied()),
            ["d.org", "c.org", "a.org"]
        );
        assert_eq!(
            super::recent_urls(v.iter().copied()),
            ["https://d.org", "http://c.org", "https://a.org/"]
        );
    }
}
