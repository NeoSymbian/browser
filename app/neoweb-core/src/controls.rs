// SPDX-License-Identifier: Apache-2.0
//! The page's controls in NEOWEB (WB5a, browser-ui.md UI3): what Back does next, script dialogs as the shell's QUERY,
//! and where an uploaded file goes for the engine.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use neoweb_proto::DialogKind;

/// What Back does next, after the kit's sheets (they close first) and the shell's keyboard (the shell hides it
/// before the app sees Back): browser-ui.md's Back order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackStep {
    /// Leave the address bar.
    EndEdit,
    /// The page's previous history entry.
    History,
    /// The start page (it comes before the first page of a tab).
    StartPage,
    /// Nothing left in the app: Back goes to the shell (`can_back` false).
    Shell,
}

pub fn back_step(editing: bool, showing_web: bool, history_back: bool) -> BackStep {
    if editing {
        BackStep::EndEdit
    } else if showing_web && history_back {
        BackStep::History
    } else if showing_web {
        BackStep::StartPage
    } else {
        BackStep::Shell
    }
}

/// The shell's QUERY limits (neoui-proto `page.rs`): 160 bytes of text.
pub const QUERY_TEXT_MAX: usize = 160;

/// A script dialog as a QUERY: the site on the first line (so it can't pass for the browser's own question), then
/// the page's text, cut to fit; and its buttons, the confirming one first.
pub fn query(site: &str, kind: DialogKind, text: &str) -> (String, Vec<String>) {
    let mut t = format!("{site}\n{}", text.trim());
    if t.len() > QUERY_TEXT_MAX {
        let mut end = QUERY_TEXT_MAX - '…'.len_utf8();
        while !t.is_char_boundary(end) {
            end -= 1;
        }
        t.truncate(end);
        t.push('…');
    }
    let buttons: &[&str] = match kind {
        DialogKind::Alert => &["OK"],
        DialogKind::Confirm | DialogKind::Prompt => &["OK", "Cancel"],
        DialogKind::Unload => &["Leave", "Stay"],
    };
    (t, buttons.iter().map(|b| String::from(*b)).collect())
}

/// Where NEOWEB puts a picked file for the engine: NEOWEBE's private folder (SID 0xE0007008), which it can read.
pub const UPLOAD_DIR: &str = "C:/Private/e0007008/upload/";

/// The engine's path for an uploaded file named like `full_name` (a Symbian path from Files): its last component,
/// with characters a FAT name can't hold replaced.
pub fn upload_path(full_name: &str) -> String {
    let name = full_name.rsplit(['\\', '/']).next().unwrap_or("");
    let name: String = name
        .chars()
        .map(|c| {
            if "<>:\"|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    format!(
        "{UPLOAD_DIR}{}",
        if name.is_empty() { "file" } else { &name }
    )
}

/// The Files picker's filter for an `accept` list ("jpg,png" or "image/*"): Files takes extensions or a family name.
pub fn picker_filter(accept: &str, photos: bool) -> String {
    if photos {
        return "image".into();
    }
    accept
        .split(',')
        .map(|a| a.trim().trim_start_matches('.'))
        .map(|a| a.strip_suffix("/*").unwrap_or(a))
        .filter(|a| !a.is_empty() && !a.contains('/'))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_goes_edit_then_history_then_start_then_shell() {
        assert_eq!(back_step(true, true, true), BackStep::EndEdit);
        assert_eq!(back_step(false, true, true), BackStep::History);
        assert_eq!(back_step(false, true, false), BackStep::StartPage);
        assert_eq!(back_step(false, false, false), BackStep::Shell);
    }

    #[test]
    fn a_dialog_names_its_site_first_and_fits_the_query() {
        let (t, b) = query(
            "forum.example.org",
            DialogKind::Confirm,
            "Delete this draft?",
        );
        assert_eq!(t, "forum.example.org\nDelete this draft?");
        assert_eq!(b, ["OK", "Cancel"]);
        let (t, _) = query("a.org", DialogKind::Alert, &"ж".repeat(200));
        assert!(t.len() <= QUERY_TEXT_MAX && t.ends_with('…') && t.starts_with("a.org\n"));
        assert_eq!(query("a", DialogKind::Unload, "").1, ["Leave", "Stay"]);
    }

    #[test]
    fn uploads_keep_the_file_name_in_the_engine_folder() {
        assert_eq!(
            upload_path("C:\\Data\\Images\\sunset.jpg"),
            "C:/Private/e0007008/upload/sunset.jpg"
        );
        assert_eq!(
            upload_path("E:\\a?b.txt"),
            "C:/Private/e0007008/upload/a_b.txt"
        );
        assert_eq!(upload_path("C:\\"), "C:/Private/e0007008/upload/file");
    }

    #[test]
    fn the_picker_filter_takes_extensions_and_families() {
        assert_eq!(picker_filter(".jpg, png", false), "jpg,png");
        assert_eq!(picker_filter("image/*", false), "image");
        assert_eq!(picker_filter("application/pdf", false), "");
        assert_eq!(picker_filter("", true), "image");
    }
}
