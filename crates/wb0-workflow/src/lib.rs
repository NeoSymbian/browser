// SPDX-License-Identifier: Apache-2.0
//! WB0 site workflows: the steps `neoweb-host` runs against a page, loaded from `wb0/sites/*.json`.
//!
//! ```json
//! { "name": "wikipedia", "url": "https://en.m.wikipedia.org/wiki/Symbian",
//!   "steps": [ { "do": "wait_load" }, { "do": "scroll", "dy": 1500 }, { "do": "shot", "name": "article" } ] }
//! ```
//! Text and URLs may use `${VAR}` from the environment (credentials); `"secret": true` keeps typed text out of logs.

use serde::Deserialize;

/// Default timeout for waits: generous, the runs are emulated.
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub name: String,
    /// The first page, loaded when the webview is created.
    pub url: String,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Step {
    #[serde(flatten)]
    pub action: Action,
    /// A failing optional step is logged and skipped (consent banners, signed-out variants).
    #[serde(default)]
    pub optional: bool,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Action {
    /// Load a URL (does not wait; follow with `wait_load`).
    Nav {
        url: String,
    },
    /// Wait until the page started by the last navigation reaches load-complete.
    WaitLoad,
    /// Wait until `document.querySelector(sel)` finds an element.
    WaitSel {
        sel: String,
    },
    /// Wait until a JavaScript expression is truthy.
    WaitJs {
        js: String,
    },
    /// Touch the centre of the element (scrolled into view first).
    Tap {
        sel: String,
    },
    /// Type text as key events into the focused element.
    Type {
        text: String,
        #[serde(default)]
        secret: bool,
    },
    /// One named key (`Enter`, `Tab`, `Backspace`, `Escape`, `ArrowDown`…) or a single character.
    Key {
        key: String,
    },
    /// Scroll by `dy` CSS pixels (positive = down) and wait for frames to settle.
    Scroll {
        dy: f32,
    },
    /// Write `<name>.png`.
    Shot {
        name: String,
    },
    Sleep {
        ms: u64,
    },
    /// Write Servo's memory report to `<name>.mem.json`.
    Mem {
        name: String,
    },
}

impl Step {
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)
    }

    /// Short label for logs and CSV: the step kind and its argument, with secrets redacted.
    pub fn label(&self) -> (&'static str, String) {
        match &self.action {
            Action::Nav { url } => ("nav", url.clone()),
            Action::WaitLoad => ("wait_load", String::new()),
            Action::WaitSel { sel } => ("wait_sel", sel.clone()),
            Action::WaitJs { js } => ("wait_js", js.clone()),
            Action::Tap { sel } => ("tap", sel.clone()),
            Action::Type { secret: true, .. } => ("type", "<secret>".into()),
            Action::Type { text, .. } => ("type", text.clone()),
            Action::Key { key } => ("key", key.clone()),
            Action::Scroll { dy } => ("scroll", dy.to_string()),
            Action::Shot { name } => ("shot", name.clone()),
            Action::Sleep { ms } => ("sleep", ms.to_string()),
            Action::Mem { name } => ("mem", name.clone()),
        }
    }
}

pub fn parse(json: &str) -> Result<Workflow, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

pub fn load(path: &std::path::Path) -> Result<Workflow, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Replace `${VAR}` with `lookup(VAR)`; a missing variable is an error naming it (never its value).
pub fn expand(s: &str, lookup: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| format!("unclosed ${{ in {s:?}"))?;
        let name = &after[..end];
        out.push_str(
            &lookup(name).ok_or_else(|| format!("environment variable {name} is not set"))?,
        );
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_actions_and_flags() {
        let w = parse(
            r##"{"name":"t","url":"https://example.com/","steps":[
              {"do":"wait_load"},{"do":"wait_sel","sel":"#q","timeout_ms":5},
              {"do":"tap","sel":"#q","optional":true},{"do":"type","text":"${U}","secret":true},
              {"do":"key","key":"Enter"},{"do":"scroll","dy":1500},{"do":"shot","name":"a"},
              {"do":"sleep","ms":10},{"do":"mem","name":"m"},{"do":"nav","url":"https://e.org/"},
              {"do":"wait_js","js":"1"}]}"##,
        )
        .unwrap();
        assert_eq!(w.steps.len(), 11);
        assert_eq!(w.steps[1].timeout_ms(), 5);
        assert_eq!(w.steps[0].timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert!(w.steps[2].optional);
        assert_eq!(w.steps[3].label(), ("type", "<secret>".to_string()));
        assert_eq!(w.steps[5].action, Action::Scroll { dy: 1500.0 });
    }

    #[test]
    fn rejects_unknown_actions_and_fields() {
        assert!(parse(r#"{"name":"t","url":"u","steps":[{"do":"fly"}]}"#).is_err());
        assert!(parse(r#"{"name":"t","url":"u","extra":1}"#).is_err());
    }

    #[test]
    fn expands_variables() {
        let env = |k: &str| (k == "A").then(|| "x".to_string());
        assert_eq!(expand("a${A}b${A}", env).unwrap(), "axbx");
        assert_eq!(expand("plain", env).unwrap(), "plain");
        assert!(expand("${B}", env).unwrap_err().contains("B is not set"));
        assert!(expand("${A", env).is_err());
    }

    #[test]
    fn all_site_workflows_parse() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../wb0/sites");
        let mut n = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json") {
                let w = load(&path).unwrap();
                assert!(!w.steps.is_empty(), "{} has no steps", path.display());
                n += 1;
            }
        }
        assert!(n >= 10, "expected the 10 WB0 sites, found {n}");
    }
}
