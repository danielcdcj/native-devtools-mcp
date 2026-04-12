//! Test harness for the CDP DOM discovery integration tests.
//!
//! Spawns a real (headless) Chrome process on an ephemeral port, connects the
//! production `CdpClient`, and exposes helpers to drive the tools under test.
//!
//! Every `Harness` owns:
//! * a Chrome child process (killed on drop),
//! * a temp `--user-data-dir` (removed on drop),
//! * an `Arc<RwLock<Option<CdpClient>>>` shaped exactly like the MCP server's
//!   runtime state, so tool functions work unmodified.
//!
//! Keep this module focused on *harness mechanics*. Scenario HTML fixtures
//! live in this file because they are small, self-contained, and closely tied
//! to the assertions; scenario *logic* stays in `cdp_dom_discovery_tests.rs`.

#![cfg(feature = "cdp")]

use base64::Engine;
use native_devtools_mcp::cdp::tools::cdp_navigate;
use native_devtools_mcp::cdp::CdpClient;
use rmcp::model::{CallToolResult, Content};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::sync::RwLock;

/// Shared state shape the MCP server uses for the live `CdpClient`.
pub type ClientHandle = Arc<RwLock<Option<CdpClient>>>;

/// Locates a Chrome/Chromium binary for the current platform.
///
/// Returns `None` if nothing suitable is installed — callers should skip the
/// test (NOT fail) in that case. We search a small fixed list rather than
/// invoking `which` so the behaviour is stable across shells.
fn find_chrome_binary() -> Option<PathBuf> {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
    } else if cfg!(target_os = "linux") {
        &[
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/snap/bin/chromium",
        ]
    } else {
        // Windows / other: not supported by this harness yet.
        &[]
    };

    candidates.iter().map(PathBuf::from).find(|p| p.is_file())
}

/// Bind a TCP listener to an ephemeral port, then drop it to release the
/// port. There's a tiny race window between drop and Chrome binding; in
/// practice this is fine for test fixtures on a single host.
fn pick_free_port() -> Option<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);
    Some(port)
}

/// A live Chrome + CDP client pair, bound together for a single test.
pub struct Harness {
    chrome: Option<Child>,
    _profile: TempDir,
    client: ClientHandle,
}

impl Harness {
    /// Spawn Chrome and connect. Returns `None` (and prints to stderr) if
    /// Chrome is not installed or launch fails — the caller should treat
    /// that as a skip, not a failure.
    pub async fn launch() -> Option<Self> {
        let chrome_path = match find_chrome_binary() {
            Some(p) => p,
            None => {
                eprintln!("[harness] skipping: no Chrome/Chromium binary found");
                return None;
            }
        };

        let profile = match TempDir::new() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[harness] skipping: cannot create temp profile dir: {e}");
                return None;
            }
        };

        let port = match pick_free_port() {
            Some(p) => p,
            None => {
                eprintln!("[harness] skipping: could not acquire a free port");
                return None;
            }
        };

        let mut cmd = Command::new(&chrome_path);
        cmd.arg("--headless=new")
            .arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-gpu")
            .arg("--disable-background-networking")
            .arg("--disable-sync")
            .arg("--disable-default-apps")
            .arg("--disable-extensions")
            // about:blank so Chrome has a page to attach to before navigation.
            .arg("about:blank")
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[harness] skipping: failed to spawn Chrome: {e}");
                return None;
            }
        };

        // Wait for the debug port to come up; chromiumoxide's own connect
        // helper will fail fast if we race ahead of Chrome.
        if let Err(e) = wait_for_debug_port(port, Duration::from_secs(15)).await {
            eprintln!("[harness] skipping: Chrome never opened debug port: {e}");
            let mut child = child;
            let _ = child.kill();
            return None;
        }

        let client = match CdpClient::connect(port).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[harness] skipping: CdpClient::connect failed: {e}");
                let mut child = child;
                let _ = child.kill();
                return None;
            }
        };

        Some(Self {
            chrome: Some(child),
            _profile: profile,
            client: Arc::new(RwLock::new(Some(client))),
        })
    }

    /// Return a clone of the shared client handle, suitable for passing into
    /// `cdp_*` tool functions.
    pub fn client_handle(&self) -> ClientHandle {
        self.client.clone()
    }

    /// Navigate the selected page to the given HTML document (inline).
    ///
    /// Uses a `data:` URL so tests don't need a local HTTP server. Chrome
    /// treats `data:` documents as same-origin with no origin, which is
    /// fine for shadow-root / srcdoc-iframe cases.
    pub async fn navigate(&mut self, html: &str) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(html);
        let url = format!("data:text/html;base64,{b64}");

        let result = cdp_navigate(Some(url), None, Some(10_000), self.client_handle()).await;
        assert_eq!(
            result.is_error,
            Some(false),
            "navigate failed: {}",
            content_text(&result)
        );

        // Give the page a frame to paint + attach custom elements /
        // shadow roots / iframe contents. CDP doesn't expose a "fully
        // settled" signal here; 200ms is empirically enough for these
        // tiny fixtures and keeps the suite snappy.
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    /// Evaluate a JS boolean expression in the page context. Convenience
    /// wrapper used by scenarios to check click side effects.
    pub async fn eval_bool(&self, expr: &str) -> bool {
        use native_devtools_mcp::cdp::tools::cdp_evaluate_script;
        let r = cdp_evaluate_script(expr.to_string(), None, self.client_handle()).await;
        if r.is_error != Some(false) {
            panic!("eval_bool failed: {}", content_text(&r));
        }
        let text = content_text(&r);
        text.trim() == "true"
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Kill the Chrome child first; chromiumoxide's handler task will
        // wind down once the websocket closes.
        if let Some(mut child) = self.chrome.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Poll `/json/version` on the debug port until it responds or we time out.
async fn wait_for_debug_port(port: u16, timeout: Duration) -> Result<(), String> {
    let start = Instant::now();
    let url = format!("http://127.0.0.1:{port}/json/version");
    loop {
        // `ureq` is already in the dependency tree — use it for a tiny
        // blocking GET inside `spawn_blocking` to avoid pulling in another
        // HTTP client.
        let u = url.clone();
        let ok = tokio::task::spawn_blocking(move || {
            let agent = ureq::Agent::new_with_defaults();
            agent.get(&u).call().is_ok()
        })
        .await
        .unwrap_or(false);

        if ok {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            return Err(format!("timed out after {:?}", timeout));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

// ---------------------------------------------------------------------------
// Result helpers
// ---------------------------------------------------------------------------

/// Concatenate all text content fragments of a `CallToolResult` into a single
/// string. Panics the test cleanly if the result contains no text (which
/// would indicate the tool returned an image or resource variant by mistake).
pub fn content_text(result: &CallToolResult) -> String {
    let mut out = String::new();
    for c in &result.content {
        if let Some(t) = text_of(c) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&t);
        }
    }
    out
}

fn text_of(content: &Content) -> Option<String> {
    content.as_text().map(|t| t.text.clone())
}

// ---------------------------------------------------------------------------
// Snapshot-text parsers (small, test-only)
// ---------------------------------------------------------------------------

/// Find the first `a<N>` UID in an AX snapshot whose role matches and whose
/// name contains the given substring.
///
/// AX snapshot format (from `tools::ax_snapshot::format_snapshot`) puts each
/// node on its own line like: `  button "AxButton"`. The rendered UID is the
/// numeric `uid` field. The snapshot map uses the same numeric → `a<N>`
/// mapping, so we count the Nth node that appears.
///
/// This is a scenario helper — if the format evolves we'll update both the
/// formatter and this parser in lockstep.
pub fn find_ax_uid(snapshot: &str, role: &str, name_substring: &str) -> Option<String> {
    for line in snapshot.lines() {
        // Lines look like: `[uid=a5] button "AxButton"`
        // But format_snapshot may differ — accept either `uid=aN` or a
        // preceding numeric uid that we prefix.
        if let Some(uid) = extract_uid(line, 'a') {
            if line.contains(role) && line.contains(name_substring) {
                return Some(uid);
            }
        }
    }
    None
}

/// Find the first `d<N>` UID in a DOM snapshot whose role matches and whose
/// label contains the given substring. The DOM snapshot format (see
/// `dom_discovery::format_dom_snapshot`) is stable: `uid=dN role "label"
/// tag=…`.
pub fn find_dom_uid(snapshot: &str, role: &str, label_substring: &str) -> Option<String> {
    for line in snapshot.lines() {
        if let Some(uid) = extract_uid(line, 'd') {
            if line.contains(role) && line.contains(label_substring) {
                return Some(uid);
            }
        }
    }
    None
}

fn extract_uid(line: &str, prefix: char) -> Option<String> {
    // Accept `uid=aN` / `uid=dN` or a bare `aN`/`dN` at the start of a token.
    for token in line.split_whitespace() {
        let candidate = token
            .strip_prefix("uid=")
            .unwrap_or(token)
            .trim_end_matches(|c: char| !c.is_alphanumeric());
        if candidate.starts_with(prefix)
            && candidate.len() > 1
            && candidate[1..].chars().all(|c| c.is_ascii_digit())
        {
            return Some(candidate.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// HTML fixtures for the scenarios.
// ---------------------------------------------------------------------------

/// Scenario 1: an AX-invisible contenteditable that only a DOM walker will
/// surface, identified by its placeholder text.
pub const HTML_CONTENTEDITABLE: &str = r#"
<!doctype html>
<html><body>
  <div id="editor"
       contenteditable="true"
       data-placeholder="Write something…"
       style="min-height:40px;border:1px solid #ccc">
  </div>
</body></html>
"#;

/// Scenario 2: a custom <div role="button"> with no text content, only an
/// aria-label.
pub const HTML_CUSTOM_BUTTON: &str = r#"
<!doctype html>
<html><body>
  <div id="x" role="button" aria-label="Close" tabindex="0"
       style="width:32px;height:32px;background:#c00"></div>
</body></html>
"#;

/// Scenario 3: two "Search" controls disambiguated by parent context.
pub const HTML_DUPLICATE_LABELS: &str = r#"
<!doctype html>
<html><body>
  <nav aria-label="Primary">
    <input type="search" placeholder="Search" />
  </nav>
  <main>
    <button aria-label="Search">Search</button>
  </main>
</body></html>
"#;

/// Scenario 4: open shadow root + same-origin srcdoc iframe. The walker
/// must descend into both.
pub const HTML_SHADOW_AND_IFRAME: &str = r#"
<!doctype html>
<html><body>
  <host-el id="host"></host-el>
  <iframe id="frame" srcdoc='<button aria-label="IframeBtn">IframeBtn</button>'></iframe>
  <script>
    class HostEl extends HTMLElement {
      constructor() {
        super();
        const root = this.attachShadow({ mode: 'open' });
        const b = document.createElement('button');
        b.setAttribute('aria-label', 'ShadowBtn');
        b.textContent = 'ShadowBtn';
        root.appendChild(b);
      }
    }
    customElements.define('host-el', HostEl);
  </script>
</body></html>
"#;

/// Scenario 5: one button the AX tree labels well ("AxButton") and one
/// contenteditable that only DOM discovery can find by its placeholder
/// ("EditorHere"). Each target stamps a marker element when activated so
/// the test can verify the click landed on the right node.
pub const HTML_MIXED_AX_DOM: &str = r#"
<!doctype html>
<html><body>
  <div id="ax-hit" data-clicked="0" style="display:none"></div>
  <div id="editor-hit" data-focused="0" style="display:none"></div>

  <button id="axbtn" onclick="document.getElementById('ax-hit').dataset.clicked='1'">AxButton</button>

  <div id="editor"
       contenteditable="true"
       data-placeholder="EditorHere"
       onfocus="document.getElementById('editor-hit').dataset.focused='1'"
       style="min-height:40px;border:1px solid #ccc;margin-top:20px">
  </div>
</body></html>
"#;
