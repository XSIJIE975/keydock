use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Two-origin local fixture server. `http` is the site origin and `idp` is a
/// distinct origin used for cross-site iframes and SSO redirects.
pub struct FixtureServer {
    pub http_port: u16,
    pub idp_port: u16,
    http_base: String,
    shutdown: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl FixtureServer {
    pub fn start() -> std::io::Result<Self> {
        let listener_http = TcpListener::bind("127.0.0.1:0")?;
        let listener_idp = TcpListener::bind("127.0.0.1:0")?;
        listener_http.set_nonblocking(true)?;
        listener_idp.set_nonblocking(true)?;
        let http_port = listener_http.local_addr()?.port();
        let idp_port = listener_idp.local_addr()?.port();
        let http_base = format!("http://127.0.0.1:{http_port}");
        let idp_base = format!("http://127.0.0.1:{idp_port}");

        let bundle_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/react-form/dist/bundle.js");
        let bundle = std::fs::read(&bundle_path).unwrap_or_else(|error| {
            format!(
                "console.error('fixture bundle missing at {}: {error}');",
                bundle_path.display()
            )
            .into_bytes()
        });

        let state = Arc::new(State {
            http_base: http_base.clone(),
            idp_base: idp_base.clone(),
            bundle,
        });
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        for (listener, is_idp) in [(listener_http, false), (listener_idp, true)] {
            let state = state.clone();
            let shutdown = shutdown.clone();
            threads.push(thread::spawn(move || {
                while !shutdown.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let state = state.clone();
                            thread::spawn(move || {
                                if let Err(error) = handle_connection(stream, &state, is_idp) {
                                    eprintln!("fixture connection error: {error}");
                                }
                            });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(error) => {
                            eprintln!("fixture accept error: {error}");
                            thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
            }));
        }

        Ok(Self {
            http_port,
            idp_port,
            http_base,
            shutdown,
            threads,
        })
    }

    pub fn normal_url(&self) -> String {
        format!("{}/normal", self.http_base)
    }

    pub fn react_url(&self) -> String {
        format!("{}/react", self.http_base)
    }

    pub fn iframe_parent_url(&self) -> String {
        format!("{}/iframe-parent", self.http_base)
    }

    pub fn spa_url(&self) -> String {
        format!("{}/spa", self.http_base)
    }

    pub fn download_url(&self) -> String {
        format!("{}/download", self.http_base)
    }

    pub fn sso_start_url(&self) -> String {
        format!("{}/sso-start", self.http_base)
    }

    pub fn site_host(&self) -> String {
        format!("127.0.0.1:{}", self.http_port)
    }

    pub fn idp_host(&self) -> String {
        format!("127.0.0.1:{}", self.idp_port)
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

struct State {
    http_base: String,
    idp_base: String,
    bundle: Vec<u8>,
}

fn handle_connection(mut stream: TcpStream, state: &State, is_idp: bool) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 8192];
    let header_end = loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        raw.extend_from_slice(&buffer[..count]);
        if let Some(index) = find_header_end(&raw) {
            break index;
        }
        if raw.len() > 1_048_576 {
            return Ok(());
        }
    };

    let header = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let mut lines = header.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_owned();
    let target = parts.next().unwrap_or("/").to_owned();

    let mut content_length = 0_usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let body_start = header_end + 4;
    while raw.len() < body_start + content_length {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..count]);
    }
    let body = raw
        .get(body_start..body_start + content_length)
        .unwrap_or_default();

    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));

    let response = route(method.as_str(), path, query, body, state, is_idp);
    write_response(&mut stream, response)
}

fn route(
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    state: &State,
    is_idp: bool,
) -> Response {
    if method == "HEAD" {
        return Response::new(200, "text/plain; charset=utf-8", b"".to_vec());
    }

    match (is_idp, path) {
        (_, "/") => html(index_page(state)),
        (false, "/normal") => html(normal_page()),
        (false, "/react") => html(react_page()),
        (_, "/bundle.js") => {
            Response::new(200, "text/javascript; charset=utf-8", state.bundle.clone())
        }
        (false, "/spa") => html(spa_page()),
        (false, "/iframe-parent") => html(iframe_parent_page(&state.idp_base)),
        (true, "/iframe-form") => html(iframe_form_page()),
        (false, "/download") => html(download_page()),
        (false, "/download/file") => {
            let mut payload = String::from("KeyDock CDP download fixture\n");
            for index in 0..4096 {
                payload.push_str(&format!("line {index:04}: 0123456789abcdef\n"));
            }
            Response::new(200, "text/plain", payload.into_bytes()).with_header(
                "Content-Disposition",
                "attachment; filename=\"keydock-cdp-spike.txt\"",
            )
        }
        (false, "/api/login") => {
            let text = String::from_utf8_lossy(body);
            let ok = text.contains("\"password\":\"secret123\"")
                && text.contains("\"username\":\"normal-user\"");
            let payload = if ok {
                br#"{"ok":true}"#.to_vec()
            } else {
                br#"{"ok":false}"#.to_vec()
            };
            Response::new(200, "application/json", payload)
        }
        (false, "/sso-start") => {
            let return_url = format!("{}/sso-home", state.http_base);
            let location = format!(
                "{}/sso-login?return={}",
                state.idp_base,
                url_encode(&return_url)
            );
            Response::redirect(&location)
        }
        (true, "/sso-login") => {
            let return_url = query_value(query, "return").unwrap_or_default();
            html(sso_login_page(&return_url))
        }
        (true, "/sso-submit") => {
            let form = String::from_utf8_lossy(body);
            let username = form_value(&form, "username").unwrap_or_default();
            let password = form_value(&form, "password").unwrap_or_default();
            let return_url = form_value(&form, "return").unwrap_or_default();
            if username == "sso-user" && password == "sso-pass" && return_url.starts_with("http") {
                Response::redirect(&return_url)
            } else {
                Response::new(
                    401,
                    "text/html; charset=utf-8",
                    sso_error_page().into_bytes(),
                )
            }
        }
        (false, "/sso-home") => html(sso_home_page()),
        (_, "/favicon.ico") => Response::new(204, "text/plain", Vec::new()),
        _ => Response::new(404, "text/plain; charset=utf-8", b"not found".to_vec()),
    }
}

fn html(body: String) -> Response {
    Response::new(200, "text/html; charset=utf-8", body.into_bytes())
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (url_decode(name) == key).then(|| url_decode(value))
    })
}

fn form_value(form: &str, key: &str) -> Option<String> {
    query_value(form, key)
}

fn url_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = hex(bytes[index + 1]);
                let low = hex(bytes[index + 2]);
                if let (Some(high), Some(low)) = (high, low) {
                    out.push(high * 16 + low);
                    index += 3;
                } else {
                    out.push(b'%');
                    index += 1;
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

struct Response {
    status: u16,
    content_type: &'static str,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Response {
    fn new(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            headers: Vec::new(),
            body,
        }
    }

    fn with_header(mut self, name: &'static str, value: &str) -> Self {
        self.headers.push((name, value.to_owned()));
        self
    }

    fn redirect(location: &str) -> Self {
        Self::new(302, "text/html; charset=utf-8", b"redirecting".to_vec())
            .with_header("Location", location)
    }
}

fn write_response(stream: &mut TcpStream, response: Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        204 => "No Content",
        302 => "Found",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Response",
    };
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn index_page(state: &State) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>KeyDock fixtures</title></head>
<body>
<h1>KeyDock CDP fixtures</h1>
<ul>
<li><a id="normal" href="/normal">Normal form</a></li>
<li><a id="react" href="/react">React controlled form</a></li>
<li><a id="iframe" href="/iframe-parent">Cross-origin iframe ({})</a></li>
<li><a id="spa" href="/spa">Delayed SPA</a></li>
<li><a id="download" href="/download">Download</a></li>
</ul>
</body></html>"#,
        state.idp_base
    )
}

fn normal_page() -> String {
    r#"<!doctype html><html><head><meta charset="utf-8"><title>Normal form</title>
<style>body{font-family:sans-serif;margin:40px}input,button{display:block;margin:8px 0;padding:8px;width:280px}</style></head>
<body>
<h1>Normal form</h1>
<form id="loginForm">
  <label for="username">Username</label><input id="username" name="username" autocomplete="off">
  <label for="password">Password</label><input id="password" name="password" type="password" autocomplete="off">
  <button id="submit" type="submit">Sign in</button>
  <div id="trusted">unset</div>
  <div id="result"></div>
</form>
<script>
document.getElementById('submit').addEventListener('click', function (event) {
  document.getElementById('trusted').textContent = String(event.isTrusted);
});
document.getElementById('loginForm').addEventListener('submit', function (event) {
  event.preventDefault();
  fetch('/api/login', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({
      username: document.getElementById('username').value,
      password: document.getElementById('password').value
    })
  }).then(function (response) { return response.json(); }).then(function (data) {
    var result = document.getElementById('result');
    if (data.ok) {
      result.id = 'success';
      result.textContent = 'NORMAL_OK';
    } else {
      result.id = 'error';
      result.textContent = 'NORMAL_BAD_CREDENTIALS';
    }
  });
});
</script>
</body></html>"#
        .to_owned()
}

fn react_page() -> String {
    r#"<!doctype html><html><head><meta charset="utf-8"><title>React controlled form</title>
<style>body{font-family:sans-serif;margin:40px}input,button{display:block;margin:8px 0;padding:8px;width:280px}</style></head>
<body><h1>React controlled form</h1><div id="root"></div><script src="/bundle.js"></script></body></html>"#
        .to_owned()
}

fn spa_page() -> String {
    r#"<!doctype html><html><head><meta charset="utf-8"><title>Delayed SPA</title>
<style>body{font-family:sans-serif;margin:40px}input,button{display:block;margin:8px 0;padding:8px;width:280px}</style></head>
<body><h1>Delayed SPA</h1><div id="app">loading</div>
<script>
setTimeout(function () {
  document.getElementById('app').innerHTML =
    '<form id="loginForm"><input id="username" autocomplete="off"><input id="password" type="password" autocomplete="off"><button id="submit" type="submit">Sign in</button><div id="trusted">unset</div><div id="result"></div></form>';
  document.getElementById('submit').addEventListener('click', function (event) {
    document.getElementById('trusted').textContent = String(event.isTrusted);
  });
  document.getElementById('loginForm').addEventListener('submit', function (event) {
    event.preventDefault();
    var result = document.getElementById('result');
    result.textContent = 'SPA_OK';
    result.id = 'success';
  });
}, 1200);
</script></body></html>"#
        .to_owned()
}

fn iframe_parent_page(idp_base: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Iframe parent</title>
<style>body{{font-family:sans-serif;margin:0;padding:20px}}iframe{{position:absolute;left:40px;top:120px;width:520px;height:360px;border:2px solid #2dd4bf}}</style></head>
<body><h1>Cross-origin iframe</h1><iframe id="loginFrame" src="{idp_base}/iframe-form"></iframe></body></html>"#
    )
}

fn iframe_form_page() -> String {
    r#"<!doctype html><html><head><meta charset="utf-8"><title>Iframe form</title>
<style>body{font-family:sans-serif;margin:16px;background:#ecfeff}input,button{display:block;margin:8px 0;padding:8px;width:220px}</style></head>
<body>
<form id="loginForm">
  <label for="username">Username</label><input id="username" autocomplete="off">
  <label for="password">Password</label><input id="password" type="password" autocomplete="off">
  <button id="submit" type="submit">Sign in</button>
  <div id="trusted">unset</div>
  <div id="result"></div>
</form>
<script>
document.getElementById('submit').addEventListener('click', function (event) {
  document.getElementById('trusted').textContent = String(event.isTrusted);
});
document.getElementById('loginForm').addEventListener('submit', function (event) {
  event.preventDefault();
  var result = document.getElementById('result');
  result.textContent = 'IFRAME_OK';
  result.id = 'success';
});
</script>
</body></html>"#
        .to_owned()
}

fn download_page() -> String {
    r#"<!doctype html><html><head><meta charset="utf-8"><title>Download fixture</title>
<style>body{font-family:sans-serif;margin:40px}</style></head>
<body><h1>Download fixture</h1><a id="download" href="/download/file" download="keydock-cdp-spike.txt">Download fixture file</a></body></html>"#
        .to_owned()
}

fn sso_login_page(return_url: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>SSO login</title>
<style>body{{font-family:sans-serif;margin:40px;background:#f0fdfa}}input,button{{display:block;margin:8px 0;padding:8px;width:280px}}</style></head>
<body><h1>SSO IDP</h1><p id="sso-origin">idp</p>
<form id="loginForm" method="post" action="/sso-submit">
  <input type="hidden" name="return" value="{return_url}">
  <label for="username">Username</label><input id="username" name="username" autocomplete="off">
  <label for="password">Password</label><input id="password" name="password" type="password" autocomplete="off">
  <button id="submit" type="submit">Sign in</button>
</form>
</body></html>"#
    )
}

fn sso_error_page() -> String {
    r#"<!doctype html><html><head><meta charset="utf-8"><title>SSO error</title></head>
<body><h1>SSO error</h1><div id="error">SSO_BAD_CREDENTIALS</div></body></html>"#
        .to_owned()
}

fn sso_home_page() -> String {
    r#"<!doctype html><html><head><meta charset="utf-8"><title>SSO home</title></head>
<body><h1>SSO home</h1><div id="success">SSO_OK</div><div id="host-check">site</div></body></html>"#
        .to_owned()
}
