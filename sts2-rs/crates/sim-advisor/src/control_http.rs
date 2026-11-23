//! The control plane's HTTP server on `127.0.0.1:<control-port>` (was
//! `realgame_control.ControlPlane.start` and its handler, `StaticSite` and
//! `BugReportForward`). The state behind it is
//! [`sts2_advisor::control::ControlState`].
//!
//! Endpoints (all JSON):
//! - `GET /status`, `GET /tree`, `GET /state`, `GET /snapshots`, `GET /log`
//! - `POST /restore` (`{snapshot_id}` or `{node_id}`), `POST /search_tree`
//! - `GET|POST /api/bugreport`, `GET /api/bugreport/<id>` with a site
//! - every other GET: a file of the bundled browser workbench, when hosted
//!   (`GET /api/health` answers 200 only then: the mod's workbench button
//!   probes it).
//!
//! Browser access is opt-in per page origin (`--control-allow-origin`): an
//! allow-listed origin gets CORS and Chrome's Private Network Access opt-in,
//! anything else gets no CORS headers at all.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use sts2_advisor::control::ControlState;
use sts2_advisor::serve::{LogLevel, Logger};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

/// Content types by extension: explicit, so `.wasm` goes out as
/// `application/wasm` (`WebAssembly.instantiateStreaming` refuses anything
/// else).
const MIME: [(&str, &str); 21] = [
    (".html", "text/html; charset=utf-8"),
    (".js", "text/javascript; charset=utf-8"),
    (".mjs", "text/javascript; charset=utf-8"),
    (".css", "text/css; charset=utf-8"),
    (".json", "application/json"),
    (".webmanifest", "application/manifest+json"),
    (".wasm", "application/wasm"),
    (".svg", "image/svg+xml"),
    (".png", "image/png"),
    (".webp", "image/webp"),
    (".jpg", "image/jpeg"),
    (".gif", "image/gif"),
    (".ico", "image/x-icon"),
    (".woff", "font/woff"),
    (".woff2", "font/woff2"),
    (".ttf", "font/ttf"),
    (".mp3", "audio/mpeg"),
    (".webm", "video/webm"),
    (".ogg", "audio/ogg"),
    (".txt", "text/plain; charset=utf-8"),
    (".atlas", "text/plain; charset=utf-8"),
];

/// A static workbench build served from disk. Headers follow
/// `web/public/_headers` (the Cloudflare Pages rules the same build ships
/// with): the art is WebP under `.png` names, `wb-assets/` is immutable,
/// the shell is `no-cache`.
#[derive(Debug, Clone)]
pub struct StaticSite {
    pub root: PathBuf,
    /// Where `/ppo/<file>` is looked up when the build does not carry it:
    /// the packaged mod strips the nets from the build and serves the
    /// advisor's own copies.
    pub ppo_dir: Option<PathBuf>,
}

impl StaticSite {
    #[must_use]
    pub fn new(root: &Path, ppo_dir: Option<&Path>) -> Self {
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        Self {
            root: canon(root),
            ppo_dir: ppo_dir.map(canon),
        }
    }

    /// A browser-only build: `wb-precache.json` is written only by the
    /// static build (a server build would call `/api/*` here and fail).
    #[must_use]
    pub fn is_static_build(root: &Path) -> bool {
        root.join("index.html").is_file()
            && (root.join("wb-precache.json").is_file()
                || root.join("wb-precache.json.gz").is_file())
    }

    fn under(root: &Path, rel: &str) -> Option<PathBuf> {
        let path = std::fs::canonicalize(root.join(rel)).ok()?;
        (path.starts_with(root) && path.is_file()).then_some(path)
    }

    /// The file for a request path, or `None`. Never leaves the roots.
    #[must_use]
    // URL paths are case-sensitive, as the Python server compared them.
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    pub fn resolve(&self, url_path: &str) -> Option<PathBuf> {
        let rel = url_path.trim_start_matches('/');
        let rel = if rel.is_empty() { "index.html" } else { rel };
        let mut found = Self::under(&self.root, rel);
        if found.is_none() && rel.ends_with(".json") {
            found = Self::under(&self.root, &format!("{rel}.gz"));
        }
        if found.is_none() {
            if let (Some(ppo), Some(tail)) = (&self.ppo_dir, rel.strip_prefix("ppo/")) {
                found = Self::under(ppo, tail);
            }
        }
        if found.is_none() && !rel.rsplit('/').next().unwrap_or(rel).contains('.') {
            found = Self::under(&self.root, "index.html"); // SPA fallback
        }
        found
    }

    /// `file` is the `.json.gz` stand-in for a `.json` request: its bytes go
    /// out gzip-encoded.
    #[must_use]
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    pub fn gzipped(url_path: &str, file: &Path) -> bool {
        file.to_string_lossy().ends_with(".json.gz") && !url_path.ends_with(".gz")
    }

    #[must_use]
    pub fn headers(url_path: &str, file: &Path) -> Vec<(&'static str, String)> {
        let ext = if Self::gzipped(url_path, file) {
            ".json".to_string()
        } else {
            file.extension()
                .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
                .unwrap_or_default()
        };
        let mut ctype = MIME
            .iter()
            .find(|(e, _)| *e == ext)
            .map_or("application/octet-stream", |(_, t)| t)
            .to_string();
        if url_path.starts_with("/assets/") && ext == ".png" {
            ctype = "image/webp".into();
        }
        if url_path.starts_with("/ppo/") && ext != ".json" {
            ctype = "application/octet-stream".into();
        }
        let cache = if (url_path.starts_with("/wb-assets/") || url_path.starts_with("/ppo/"))
            && url_path != "/ppo/index.json"
        {
            "public, max-age=31536000, immutable"
        } else if ["/assets/", "/fonts/", "/icons/"]
            .iter()
            .any(|p| url_path.starts_with(p))
        {
            "public, max-age=604800"
        } else {
            "no-cache"
        };
        vec![
            ("content-type", ctype),
            ("cache-control", cache.to_string()),
            ("x-content-type-options", "nosniff".to_string()),
        ]
    }
}

/// The store's body cap (`web/functions/api/_bugreport.ts` MAX_BODY_BYTES).
pub const BUGREPORT_MAX_BYTES: usize = 20 * 1024 * 1024;
/// Cloudflare's bot check answers 403 to a generic agent.
pub const BUGREPORT_USER_AGENT: &str =
    "SpireMentor-advisor/1 (bundled workbench bug-report forward)";
/// The run envelope fields a forwarded report is stamped with (`/status`
/// minus `log_path`, which names the player's home directory).
const STATUS_FIELDS: [&str; 6] = ["connected", "character", "seed", "floor", "act", "state_id"];

fn is_report_id(id: &str) -> bool {
    id.len() == 19
        && id.starts_with("wb-")
        && id[3..]
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// 16 hex digits nobody can guess from outside (std's per-process random
/// hasher keys, the clock and a counter).
fn token_hex8() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    h.write_u64(N.fetch_add(1, Ordering::Relaxed));
    format!("{:016x}", h.finish())
}

/// The store's schema check (`_bugreport.ts` validateDump).
fn validate_dump(dump: &Value) -> Option<String> {
    let Some(m) = dump.as_object() else {
        return Some("body must be a JSON object".into());
    };
    if m.get("schema_version") != Some(&json!(1)) {
        return Some("schema_version must be 1".into());
    }
    for key in ["meta", "repro", "state", "ui"] {
        if !m.get(key).is_some_and(Value::is_object) {
            return Some(format!("{key} must be an object"));
        }
    }
    if m.get("note")
        .is_some_and(|n| !n.is_null() && !n.is_string())
    {
        return Some("note must be a string or null".into());
    }
    None
}

/// The bundled workbench's `/api/bugreport`: forwarded to the site's
/// intake, else saved beside the logs.
#[derive(Debug, Clone)]
pub struct BugReportForward {
    pub site: Option<String>,
    pub local_dir: PathBuf,
}

impl BugReportForward {
    #[must_use]
    pub fn new(site: Option<&str>, local_dir: PathBuf) -> Self {
        Self {
            site: site
                .map(|s| s.trim().trim_end_matches('/').to_string())
                .filter(|s| !s.is_empty()),
            local_dir,
        }
    }

    #[must_use]
    pub fn probe(&self) -> Value {
        json!({"ok": true, "store": if self.site.is_some() { "forward" } else { "local" }})
    }

    /// Enrich `dump` and store it: `(HTTP status, reply)`.
    pub fn submit(
        &self,
        mut dump: Value,
        live_state: Option<Value>,
        status: &Value,
        log: &Logger,
    ) -> (u16, Value) {
        if let Some(problem) = validate_dump(&dump) {
            return (422, json!({"detail": problem}));
        }
        dump["meta"]["forwarded_by"] = json!("advisor");
        let advisor: serde_json::Map<String, Value> = STATUS_FIELDS
            .iter()
            .filter_map(|k| status.get(*k).map(|v| ((*k).to_string(), v.clone())))
            .collect();
        dump["meta"]["advisor"] = Value::Object(advisor);
        if let Some(live) = live_state {
            if dump["state"].get("live_game").is_none() {
                dump["state"]["live_game"] = live;
            }
        }
        let body = dump.to_string();
        if body.len() > BUGREPORT_MAX_BYTES {
            return (
                413,
                json!({"detail": format!("report too large ({} bytes; limit {BUGREPORT_MAX_BYTES})", body.len())}),
            );
        }
        let mut why = "this build has no bug-report address".to_string();
        if let Some(site) = &self.site {
            match post(site, &body) {
                Ok(reply) => {
                    log(
                        LogLevel::Info,
                        "realgame_control",
                        &format!("bug report forwarded: {}", reply["id"]),
                    );
                    return (201, reply);
                }
                Err(e) => {
                    why = format!("could not reach the report store: {e}");
                    log(
                        LogLevel::Warning,
                        "realgame_control",
                        &format!("bug report forward to {site} failed: {e}"),
                    );
                }
            }
        }
        self.save_local(dump, &why, log)
    }

    /// `GET /api/bugreport/<id>`: a report this server saved itself.
    #[must_use]
    pub fn fetch(&self, report_id: &str) -> (u16, Vec<u8>) {
        let detail = |m: String| json!({"detail": m}).to_string().into_bytes();
        if !is_report_id(report_id) {
            return (404, detail(format!("unknown report: {report_id}")));
        }
        let path = self.local_dir.join(format!("{report_id}.json"));
        if let Ok(bytes) = std::fs::read(&path) {
            return (200, bytes);
        }
        match &self.site {
            Some(site) => (
                404,
                detail(format!(
                    "{report_id} is not saved here; a sent report opens on the hosted \
                     workbench (signed in): {site}/?report={report_id}"
                )),
            ),
            None => (404, detail(format!("unknown report: {report_id}"))),
        }
    }

    fn save_local(&self, mut dump: Value, why: &str, log: &Logger) -> (u16, Value) {
        let report_id = format!("wb-{}", token_hex8());
        let stamp = sts2_advisor::serve::utc_stamp();
        dump["meta"]["id"] = json!(report_id);
        dump["meta"]["captured_at"] = json!(format!(
            "{}-{}-{}T{}:{}:{}Z",
            &stamp[0..4],
            &stamp[4..6],
            &stamp[6..8],
            &stamp[9..11],
            &stamp[11..13],
            &stamp[13..15]
        ));
        dump["meta"]["received_via"] = json!("advisor-local");
        let path = self.local_dir.join(format!("{report_id}.json"));
        if let Err(e) = std::fs::create_dir_all(&self.local_dir)
            .and_then(|()| std::fs::write(&path, dump.to_string()))
        {
            return (
                500,
                json!({"detail": format!("{why}, and saving it failed: {e}")}),
            );
        }
        let shown = path.display().to_string();
        log(
            LogLevel::Info,
            "realgame_control",
            &format!("bug report saved locally: {shown} ({why})"),
        );
        (
            201,
            json!({
                "id": report_id,
                "url": shown,
                "path": shown,
                "detail": format!("Not sent ({why}). Saved to {shown} -- attach that file to your report."),
            }),
        )
    }
}

/// POST the dump to the store's intake, following no redirect (a redirect
/// is the site's sign-in page).
fn post(site: &str, body: &str) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(60))
        .build();
    let res = agent
        .post(&format!("{site}/intake/bugreport"))
        .set("content-type", "application/json")
        .set("User-Agent", BUGREPORT_USER_AGENT)
        .send_string(body);
    let res = match res {
        Ok(r) if (300..400).contains(&r.status()) => {
            let host = r
                .header("Location")
                .and_then(|l| l.split("://").nth(1))
                .and_then(|r| r.split('/').next())
                .unwrap_or_default()
                .to_string();
            return Err(if host.is_empty() {
                format!(
                    "HTTP {}: the report store redirected instead of answering",
                    r.status()
                )
            } else {
                format!(
                    "HTTP {}: the report store sent us to {host} (a sign-in page?) -- its \
                     /intake/bugreport is not exempt from the site's sign-in",
                    r.status()
                )
            });
        }
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("detail").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| text.chars().take(200).collect());
            return Err(format!("HTTP {code}: {detail}"));
        }
        Err(e) => return Err(e.to_string()),
    };
    let reply: Value = res.into_json().map_err(|e| e.to_string())?;
    match (reply.get("id").and_then(Value::as_str), reply.get("url")) {
        (Some(id), Some(url)) if !url.is_null() && url != &json!("") => {
            Ok(json!({"id": id, "url": url}))
        }
        _ => Err("the store's reply was not {id, url}".into()),
    }
}

/// A non-report POST body's cap (`/restore`, `/search_tree` take a few keys).
pub const BODY_MAX_BYTES: u64 = 1024 * 1024;

/// The `Host` values this server answers to: the loopback names on its own
/// port. Anything else is refused, which blocks DNS rebinding (a page whose
/// own host name resolves to 127.0.0.1 reaching this server same-origin).
fn loopback_hosts(port: u16) -> [String; 3] {
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
}

/// `POST /search_tree`: a companion tree over the current combat board, or
/// the error status and payload.
pub type SearchTreeFn = Arc<dyn Fn(Option<&Value>) -> Result<Value, (u16, Value)> + Send + Sync>;

/// What the server needs besides the shared state.
#[derive(Clone)]
pub struct Plane {
    pub control: Arc<ControlState>,
    pub allow_origins: Vec<String>,
    pub site: Option<StaticSite>,
    pub bug_reports: Option<BugReportForward>,
    pub search_tree: Option<SearchTreeFn>,
    pub log: Logger,
}

impl std::fmt::Debug for Plane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plane")
            .field("allow_origins", &self.allow_origins)
            .finish_non_exhaustive()
    }
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("ASCII header")
}

fn req_header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// `%XX` decoding of a URL path (`urllib.parse.unquote`).
fn unquote(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One `?key=value` of a request URL, `%XX`-decoded; `None` when absent.
fn query_param(url: &str, key: &str) -> Option<String> {
    let (_, q) = url.split_once('?')?;
    q.split('&')
        .filter_map(|kv| kv.split_once('=').or(Some((kv, ""))))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| unquote(&v.replace('+', " ")))
}

impl Plane {
    fn allowed(&self, origin: Option<&str>) -> bool {
        origin.is_some_and(|o| {
            self.allow_origins
                .iter()
                .any(|a| a == o.trim_end_matches('/'))
        })
    }

    /// CORS + Private Network Access, for an allow-listed Origin only.
    fn cors(&self, req: &Request) -> Vec<Header> {
        let origin = req_header(req, "Origin");
        if !self.allowed(origin) {
            return Vec::new();
        }
        vec![
            header("Access-Control-Allow-Origin", origin.unwrap_or_default()),
            header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
            header("Access-Control-Allow-Headers", "content-type"),
            header("Access-Control-Allow-Private-Network", "true"),
            header("Access-Control-Max-Age", "600"),
            header("Vary", "Origin"),
        ]
    }

    fn send_json(&self, req: Request, code: u16, payload: &Value) {
        self.send_bytes(req, code, payload.to_string().into_bytes(), false);
    }

    fn send_bytes(&self, req: Request, code: u16, body: Vec<u8>, no_store: bool) {
        let mut res = Response::from_data(body)
            .with_status_code(StatusCode(code))
            .with_header(header("content-type", "application/json"));
        if no_store {
            res.add_header(header("cache-control", "no-store"));
        }
        for h in self.cors(&req) {
            res.add_header(h);
        }
        let _ = req.respond(res);
    }

    /// Whether `origin` is this server's own (`http://<loopback>:<port>`).
    fn own_origin(origin: &str, port: u16) -> bool {
        loopback_hosts(port)
            .iter()
            .any(|h| origin.eq_ignore_ascii_case(&format!("http://{h}")))
    }

    fn handle(&self, mut req: Request, port: u16) {
        let url = req.url().to_string();
        let path = url.split('?').next().unwrap_or_default().to_string();
        let host_ok = req_header(&req, "Host").is_some_and(|h| {
            loopback_hosts(port)
                .iter()
                .any(|l| l.eq_ignore_ascii_case(h.trim()))
        });
        if !host_ok {
            self.send_json(req, 403, &json!({"error": "host not allowed"}));
            return;
        }
        if *req.method() == Method::Post {
            // A state-changing call (a rewind!) from a page that is neither
            // this one nor allow-listed: a cross-site form or fetch. Refused
            // before its body is read.
            let foreign = req_header(&req, "Origin").is_some_and(|o| {
                let o = o.trim_end_matches('/');
                !Self::own_origin(o, port) && !self.allow_origins.iter().any(|a| a == o)
            });
            if foreign {
                let detail = if path == "/api/bugreport" {
                    "bug reports are accepted from the workbench page only"
                } else {
                    "origin not allowed"
                };
                self.send_json(
                    req,
                    403,
                    &json!({"error": "origin not allowed", "detail": detail}),
                );
                return;
            }
        }
        match req.method().clone() {
            Method::Options => {
                if self.allowed(req_header(&req, "Origin")) {
                    let mut res = Response::empty(204);
                    for h in self.cors(&req) {
                        res.add_header(h);
                    }
                    let _ = req.respond(res);
                } else {
                    self.send_json(req, 403, &json!({"error": "origin not allowed"}));
                }
            }
            Method::Get => self.get(req, &path, &url),
            Method::Head => {
                if let Some((res, _)) = self.file(&req, &path, true) {
                    let _ = req.respond(res);
                } else {
                    let _ = req.respond(Response::empty(404));
                }
            }
            Method::Post => {
                if path == "/api/bugreport" {
                    if let Some(fwd) = self.bug_reports.clone() {
                        self.post_bug_report(req, &fwd);
                        return;
                    }
                }
                if req.body_length().is_some_and(|n| n as u64 > BODY_MAX_BYTES) {
                    self.send_json(req, 413, &json!({"error": "body too large"}));
                    return;
                }
                let mut raw = String::new();
                let _ = req
                    .as_reader()
                    .take(BODY_MAX_BYTES)
                    .read_to_string(&mut raw);
                let body: Value = serde_json::from_str(&raw)
                    .ok()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({}));
                match path.as_str() {
                    "/restore" => self.restore(req, &body),
                    "/search_tree" => match &self.search_tree {
                        None => self.send_json(req, 503, &json!({"error": "search unavailable"})),
                        Some(f) => match f(body.get("sims")) {
                            Ok(tree) => self.send_json(req, 200, &tree),
                            Err((code, payload)) => self.send_json(req, code, &payload),
                        },
                    },
                    _ => {
                        self.send_json(req, 404, &json!({"error": format!("unknown path {path}")}));
                    }
                }
            }
            _ => self.send_json(req, 405, &json!({"error": "method not allowed"})),
        }
    }

    fn warn(&self) -> impl Fn(&str) + '_ {
        move |m: &str| (self.log)(LogLevel::Warning, "realgame_control", m)
    }

    fn get(&self, req: Request, path: &str, url: &str) {
        let c = &self.control;
        match path {
            "/status" => self.send_json(req, 200, &c.status()),
            "/tree" => self.send_json(req, 200, &c.tree.timeline()),
            "/state" => match c.last_state() {
                Some(state) => self.send_json(req, 200, &state),
                None => self.send_json(
                    req,
                    404,
                    &json!({"error": "no_state", "detail": "no decision from the game yet \
                        (the mod sends one at each combat turn or choice screen)"}),
                ),
            },
            "/snapshots" => match c.list_snapshots(&self.warn()) {
                Some(reply) => self.send_json(req, 200, &reply),
                None => self.send_json(req, 503, &json!({"error": "bridge not connected"})),
            },
            "/log" => {
                let stat_only = query_param(url, "stat").as_deref() == Some("1");
                let (code, payload) = match query_param(url, "name") {
                    Some(name) => c.run_log(&name, stat_only),
                    None => c.live_log(stat_only),
                };
                self.send_json(req, code, &payload);
            }
            "/logs" => self.send_json(req, 200, &c.run_logs()),
            "/api/bugreport" if self.bug_reports.is_some() => {
                let probe = self.bug_reports.as_ref().map(BugReportForward::probe);
                self.send_json(req, 200, &probe.unwrap_or_default());
            }
            p if p.starts_with("/api/bugreport/") && self.bug_reports.is_some() => {
                let id = unquote(&p["/api/bugreport/".len()..]);
                let (code, body) = self
                    .bug_reports
                    .as_ref()
                    .map_or((404, Vec::new()), |f| f.fetch(&id));
                self.send_bytes(req, code, body, true);
            }
            "/api/health" if self.site.is_some() => {
                self.send_json(req, 200, &json!({"ok": true, "workbench": "static"}));
            }
            _ => {
                if let Some((res, _)) = self.file(&req, path, false) {
                    let _ = req.respond(res);
                } else {
                    self.send_json(req, 404, &json!({"error": format!("unknown path {path}")}));
                }
            }
        }
    }

    /// A file of the bundled workbench, or `None`.
    fn file(
        &self,
        req: &Request,
        path: &str,
        head: bool,
    ) -> Option<(Response<Box<dyn std::io::Read + Send>>, u64)> {
        let site = self.site.as_ref()?;
        if path.starts_with("/api/") {
            return None;
        }
        let file = site.resolve(&unquote(path))?;
        let gz = StaticSite::gzipped(path, &file);
        let accepts_gzip =
            req_header(req, "accept-encoding").is_some_and(|a| a.to_lowercase().contains("gzip"));
        let (reader, size, encoding): (Box<dyn std::io::Read + Send>, u64, Option<&str>) =
            if gz && !accepts_gzip {
                let mut out = Vec::new();
                flate2::read::GzDecoder::new(std::fs::File::open(&file).ok()?)
                    .read_to_end(&mut out)
                    .ok()?;
                let n = out.len() as u64;
                (Box::new(std::io::Cursor::new(out)), n, None)
            } else {
                let f = std::fs::File::open(&file).ok()?;
                let n = f.metadata().ok()?.len();
                (Box::new(f), n, gz.then_some("gzip"))
            };
        let mut headers: Vec<Header> = StaticSite::headers(path, &file)
            .iter()
            .map(|(k, v)| header(k, v))
            .collect();
        if gz {
            headers.push(header("vary", "accept-encoding"));
        }
        if let Some(e) = encoding {
            headers.push(header("content-encoding", e));
        }
        let reader: Box<dyn std::io::Read + Send> = if head {
            Box::new(std::io::empty())
        } else {
            reader
        };
        let res = Response::new(
            StatusCode(200),
            headers,
            reader,
            usize::try_from(size).ok(),
            None,
        );
        Some((res, size))
    }

    fn post_bug_report(&self, mut req: Request, fwd: &BugReportForward) {
        // The origin was checked with every POST (`handle`).
        let ctype = req_header(&req, "content-type")
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if ctype != "application/json" {
            self.send_json(
                req,
                415,
                &json!({"detail": "a bug report is application/json"}),
            );
            return;
        }
        let length = req.body_length().unwrap_or(0);
        if length > BUGREPORT_MAX_BYTES {
            self.send_json(
                req,
                413,
                &json!({"detail": format!("report too large ({length} bytes)")}),
            );
            return;
        }
        let mut raw = Vec::new();
        let _ = req
            .as_reader()
            .take(BUGREPORT_MAX_BYTES as u64 + 1)
            .read_to_end(&mut raw);
        let dump = if raw.is_empty() {
            Value::Null
        } else {
            let Ok(v) = serde_json::from_slice(&raw) else {
                self.send_json(req, 400, &json!({"detail": "body is not JSON"}));
                return;
            };
            v
        };
        let (code, reply) = fwd.submit(
            dump,
            self.control.last_state(),
            &self.control.status(),
            &self.log,
        );
        self.send_json(req, code, &reply);
    }

    fn restore(&self, req: Request, body: &Value) {
        let c = &self.control;
        let mut snapshot_id = body.get("snapshot_id").filter(|v| !v.is_null()).cloned();
        let node_id = body.get("node_id").filter(|v| !v.is_null());
        if snapshot_id.is_none() {
            if let Some(node) = node_id {
                let plan =
                    sts2_advisor::livetree::node_index(node).and_then(|n| c.tree.seek_plan(n));
                let Some(plan) = plan else {
                    self.send_json(
                        req,
                        409,
                        &json!({"error": "not_seekable", "detail": "that node carries no game save \
                            point (the game only saves at room entry and post-combat rewards)"}),
                    );
                    return;
                };
                if plan["action"] == "undo_turn" {
                    let turn = plan["turn"].as_i64().unwrap_or(0);
                    let tape = plan.get("tape_id").and_then(Value::as_str);
                    match c.undo_turn(turn, tape, &self.warn()) {
                        None => self.send_json(req, 503, &json!({"error": "bridge not connected"})),
                        Some(r) if r["type"] == "combat_undone" => self.send_json(req, 200, &r),
                        Some(r) => self.send_json(req, 409, &r),
                    }
                    return;
                }
                snapshot_id = Some(plan["snapshot_id"].clone());
            }
        }
        let id = snapshot_id
            .filter(sts2_advisor::py::truthy)
            .map(|v| sts2_advisor::py::str_of(&v));
        let Some(id) = id else {
            self.send_json(req, 400, &json!({"error": "missing snapshot_id/node_id"}));
            return;
        };
        match c.restore(&id, &self.warn()) {
            None => self.send_json(req, 503, &json!({"error": "bridge not connected"})),
            Some(r) if r["type"] == "snapshot_restored" => self.send_json(req, 200, &r),
            Some(r) => self.send_json(req, 409, &r),
        }
    }
}

/// Serve on `127.0.0.1:port` (0 = ephemeral) from a background thread; the
/// bound port, or `None` when it could not bind (logged).
pub fn start(plane: Plane, port: u16) -> Option<u16> {
    let server = match Server::http(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => {
            (plane.log)(
                LogLevel::Warning,
                "realgame_control",
                &format!("control plane could not bind 127.0.0.1:{port}: {e}"),
            );
            return None;
        }
    };
    let bound = server.server_addr().to_ip().map_or(port, |a| a.port());
    (plane.log)(
        LogLevel::Info,
        "realgame_control",
        &format!(
            "control plane listening on 127.0.0.1:{bound} (browser origins: {})",
            if plane.allow_origins.is_empty() {
                "none".to_string()
            } else {
                plane.allow_origins.join(", ")
            }
        ),
    );
    let plane = Arc::new(plane);
    let _ = std::thread::Builder::new()
        .name("advisor-control".into())
        .spawn(move || {
            for req in server.incoming_requests() {
                let p = plane.clone();
                let _ = std::thread::Builder::new()
                    .name("advisor-control-req".into())
                    .spawn(move || p.handle(req, bound));
            }
        });
    Some(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use sts2_advisor::livetree::LiveRunTree;

    struct Reply {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl Reply {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        }

        fn json(&self) -> Value {
            serde_json::from_slice(&self.body).unwrap()
        }
    }

    fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Reply {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut req = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("Host")) {
            req.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
        }
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
        s.write_all(req.as_bytes()).unwrap();
        s.write_all(body).unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .unwrap()
            .split(' ')
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let headers = lines
            .filter_map(|l| {
                l.split_once(": ")
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect();
        Reply {
            status,
            headers,
            body: raw[split + 4..].to_vec(),
        }
    }

    fn plane(
        site: Option<StaticSite>,
        reports: Option<BugReportForward>,
    ) -> (u16, Arc<ControlState>) {
        let control = Arc::new(ControlState::new(Arc::new(LiveRunTree::new())));
        let port = start(
            Plane {
                control: control.clone(),
                allow_origins: vec!["https://wb.example".into()],
                site,
                bug_reports: reports,
                search_tree: None,
                log: Arc::new(|_, _, _| {}),
            },
            0,
        )
        .unwrap();
        (port, control)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "simadv-{name}-{}-{}",
            std::process::id(),
            token_hex8()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_bundled_workbench_is_served_without_leaving_its_root() {
        let root = temp_dir("site");
        std::fs::write(root.join("index.html"), "<html>wb</html>").unwrap();
        std::fs::write(root.join("wb-precache.json"), "[]").unwrap();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/a.png"), b"RIFFwebp").unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"{\"k\": 1}").unwrap();
        std::fs::write(root.join("data.json.gz"), gz.finish().unwrap()).unwrap();
        std::fs::write(root.parent().unwrap().join("secret.txt"), "no").unwrap();
        assert!(StaticSite::is_static_build(&root));
        let (port, _) = plane(Some(StaticSite::new(&root, None)), None);

        let r = http(port, "GET", "/", &[], b"");
        assert_eq!(
            (r.status, r.body.as_slice()),
            (200, b"<html>wb</html>".as_slice())
        );
        assert_eq!(r.header("cache-control"), Some("no-cache"));
        let spa = http(port, "GET", "/runs/abc", &[], b"");
        assert_eq!(spa.body, b"<html>wb</html>", "SPA fallback");
        let art = http(port, "GET", "/assets/a.png", &[], b"");
        assert_eq!(art.header("content-type"), Some("image/webp"));
        let plain = http(port, "GET", "/data.json", &[], b"");
        assert_eq!(
            plain.json(),
            json!({"k": 1}),
            "decompressed for a client without gzip"
        );
        let zipped = http(
            port,
            "GET",
            "/data.json",
            &[("Accept-Encoding", "gzip")],
            b"",
        );
        assert_eq!(zipped.header("content-encoding"), Some("gzip"));
        let escape = http(port, "GET", "/../secret.txt", &[], b"");
        assert_ne!(escape.body, b"no");
        let escape2 = http(port, "GET", "/%2e%2e/secret.txt", &[], b"");
        assert_ne!(escape2.body, b"no");
        assert_eq!(http(port, "GET", "/api/health", &[], b"").status, 200);
        assert_eq!(http(port, "HEAD", "/", &[], b"").status, 200);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn earlier_run_logs_are_listed_and_served_by_name() {
        let dir = temp_dir("run-logs");
        std::fs::write(dir.join("old_run.jsonl"), "{\"x\":1}\n").unwrap();
        let (port, control) = plane(None, None);
        control.set_logs_dir(Some(&dir));
        let list = http(port, "GET", "/logs", &[], b"").json();
        assert_eq!(list["logs"][0]["name"], "old_run.jsonl");
        let one = http(port, "GET", "/log?name=old_run.jsonl", &[], b"");
        assert_eq!(one.status, 200);
        assert_eq!(one.json()["text"], "{\"x\":1}\n");
        let stat = http(port, "GET", "/log?name=old_run.jsonl&stat=1", &[], b"").json();
        assert_eq!(stat["size"], 8);
        assert!(stat.get("text").is_none());
        assert_eq!(
            http(port, "GET", "/log?name=..%2Fold_run.jsonl", &[], b"").status,
            400
        );
        assert_eq!(
            http(port, "GET", "/log", &[], b"").status,
            404,
            "no live log yet"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_json_endpoints_answer_without_a_game() {
        let (port, control) = plane(None, None);
        assert_eq!(
            http(port, "GET", "/status", &[], b"").json()["connected"],
            false
        );
        assert_eq!(http(port, "GET", "/state", &[], b"").status, 404);
        assert_eq!(http(port, "GET", "/log", &[], b"").status, 404);
        assert_eq!(http(port, "GET", "/snapshots", &[], b"").status, 503);
        assert_eq!(
            http(port, "GET", "/api/health", &[], b"").status,
            404,
            "no site hosted"
        );
        assert_eq!(
            http(port, "GET", "/tree", &[], b"").json()["nodes"][0]["decision"],
            "root"
        );
        control.note_message(&json!({"type": "advice_request", "seed": "S"}));
        assert_eq!(http(port, "GET", "/state", &[], b"").json()["seed"], "S");
        let missing = http(
            port,
            "POST",
            "/restore",
            &[("Content-Type", "application/json")],
            b"{}",
        );
        assert_eq!(missing.status, 400);
        let node = http(port, "POST", "/restore", &[], br#"{"node_id": 0}"#);
        assert_eq!(node.status, 409);
        assert_eq!(
            http(port, "POST", "/restore", &[], br#"{"snapshot_id": "s1"}"#).status,
            503
        );
        assert_eq!(http(port, "POST", "/search_tree", &[], b"{}").status, 503);
    }

    #[test]
    fn only_an_allowed_origin_gets_cors() {
        let (port, _) = plane(None, None);
        let ok = http(
            port,
            "OPTIONS",
            "/status",
            &[("Origin", "https://wb.example/")],
            b"",
        );
        assert_eq!(ok.status, 204);
        assert_eq!(
            ok.header("Access-Control-Allow-Private-Network"),
            Some("true")
        );
        let other = http(
            port,
            "OPTIONS",
            "/status",
            &[("Origin", "https://evil.example")],
            b"",
        );
        assert_eq!(other.status, 403);
        let get = http(
            port,
            "GET",
            "/status",
            &[("Origin", "https://evil.example")],
            b"",
        );
        assert_eq!(get.header("Access-Control-Allow-Origin"), None);
        let get = http(
            port,
            "GET",
            "/status",
            &[("Origin", "https://wb.example")],
            b"",
        );
        assert_eq!(
            get.header("Access-Control-Allow-Origin"),
            Some("https://wb.example")
        );
    }

    #[test]
    fn a_bug_report_without_a_site_is_saved_beside_the_logs() {
        let dir = temp_dir("reports");
        let root = temp_dir("site2");
        std::fs::write(root.join("index.html"), "x").unwrap();
        std::fs::write(root.join("wb-precache.json"), "[]").unwrap();
        let (port, control) = plane(
            Some(StaticSite::new(&root, None)),
            Some(BugReportForward::new(None, dir.clone())),
        );
        control.note_message(&json!({"type": "advice_request", "seed": "S", "floor": 3}));
        assert_eq!(
            http(port, "GET", "/api/bugreport", &[], b"").json()["store"],
            "local"
        );
        let dump = json!({"schema_version": 1, "meta": {}, "repro": {}, "state": {}, "ui": {}});
        let body = dump.to_string();
        let json_ct = ("Content-Type", "application/json");
        assert_eq!(
            http(
                port,
                "POST",
                "/api/bugreport",
                &[("Content-Type", "text/plain")],
                body.as_bytes()
            )
            .status,
            415
        );
        let foreign = http(
            port,
            "POST",
            "/api/bugreport",
            &[json_ct, ("Origin", "https://evil.example")],
            body.as_bytes(),
        );
        assert_eq!(foreign.status, 403);
        let bad = http(
            port,
            "POST",
            "/api/bugreport",
            &[json_ct],
            br#"{"schema_version": 2}"#,
        );
        assert_eq!(bad.status, 422);
        let saved = http(port, "POST", "/api/bugreport", &[json_ct], body.as_bytes());
        assert_eq!(saved.status, 201);
        let id = saved.json()["id"].as_str().unwrap().to_string();
        assert!(is_report_id(&id), "{id}");
        let back = http(port, "GET", &format!("/api/bugreport/{id}"), &[], b"").json();
        assert_eq!(back["state"]["live_game"]["seed"], "S");
        assert_eq!(back["meta"]["advisor"]["floor"], 3);
        assert_eq!(back["meta"]["received_via"], "advisor-local");
        assert_eq!(
            http(port, "GET", "/api/bugreport/../x", &[], b"").status,
            404
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A fake report store: answers its intake with `status` and `body`
    /// (and `location`), once.
    fn store(status: u16, body: &'static str, location: Option<&'static str>) -> String {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || {
            if let Ok(req) = server.recv() {
                assert_eq!(req.url(), "/intake/bugreport");
                let mut res = Response::from_string(body).with_status_code(StatusCode(status));
                if let Some(l) = location {
                    res.add_header(header("Location", l));
                }
                let _ = req.respond(res);
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    #[test]
    fn a_bug_report_is_forwarded_to_the_store_and_a_sign_in_redirect_is_named() {
        let log: Logger = Arc::new(|_, _, _| {});
        let dump = json!({"schema_version": 1, "meta": {}, "repro": {}, "state": {}, "ui": {}});
        let site = store(
            201,
            r#"{"id": "wb-0123456789abcdef", "url": "https://wb/?report=x"}"#,
            None,
        );
        let fwd = BugReportForward::new(Some(&site), temp_dir("fwd"));
        let (code, reply) = fwd.submit(dump.clone(), None, &json!({}), &log);
        assert_eq!(
            (code, reply["id"].as_str()),
            (201, Some("wb-0123456789abcdef"))
        );

        let site = store(302, "", Some("https://login.example.com/auth"));
        let dir = temp_dir("fwd2");
        let fwd = BugReportForward::new(Some(&site), dir.clone());
        let (code, reply) = fwd.submit(dump, None, &json!({}), &log);
        assert_eq!(code, 201, "saved locally instead");
        let detail = reply["detail"].as_str().unwrap();
        assert!(detail.contains("login.example.com"), "{detail}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_foreign_host_or_origin_is_refused() {
        let (port, _) = plane(None, None);
        // DNS rebinding: the page's own name resolved to loopback.
        let rebound = http(port, "GET", "/status", &[("Host", "evil.example")], b"");
        assert_eq!(rebound.status, 403);
        // A cross-site page rewinding the game.
        let csrf = http(
            port,
            "POST",
            "/restore",
            &[("Origin", "https://evil.example")],
            br#"{"snapshot_id": "s1"}"#,
        );
        assert_eq!(csrf.status, 403);
        // The page this server hosts, and an allow-listed one, get through
        // (to "not connected": there is no game).
        let own = format!("http://localhost:{port}");
        let ok = http(
            port,
            "POST",
            "/restore",
            &[("Origin", own.as_str())],
            br#"{"snapshot_id": "s1"}"#,
        );
        assert_eq!(ok.status, 503);
        let listed = http(
            port,
            "POST",
            "/restore",
            &[("Origin", "https://wb.example")],
            br#"{"snapshot_id": "s1"}"#,
        );
        assert_eq!(listed.status, 503);
        let big = vec![b' '; usize::try_from(BODY_MAX_BYTES).unwrap() + 1];
        assert_eq!(http(port, "POST", "/restore", &[], &big).status, 413);
    }
}
