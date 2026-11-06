use serde_json::{json, Map, Value};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};
use url::Url;

const VERSION: &str = "0.4.1";
const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

pub fn run() -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = BufReader::new(stdin.lock());
    let mut output = BufWriter::new(stdout.lock());
    match serve(&mut input, &mut output) {
        Ok(code) => code,
        Err(_) => {
            let _ = writeln!(io::stderr().lock(), "rydantic TSP transport failed");
            4
        }
    }
}

fn serve<R: BufRead, W: Write>(input: &mut R, output: &mut W) -> io::Result<i32> {
    let mut server = Server::default();
    loop {
        let Some(frame) = read_frame(input)? else {
            return Ok(if server.shutdown { 0 } else { 1 });
        };
        let message = match serde_json::from_slice::<Value>(&frame) {
            Ok(message) => message,
            Err(_) => {
                write_frame(output, &rpc_error(Value::Null, -32700, "Parse error."))?;
                continue;
            }
        };
        let outcome = server.dispatch(&message);
        for notification in outcome.notifications {
            write_frame(output, &notification)?;
        }
        if let Some(response) = outcome.response {
            write_frame(output, &response)?;
        }
        if let Some(code) = outcome.exit {
            return Ok(code);
        }
    }
}

fn read_frame<R: BufRead>(input: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut content_length = None;
    let mut header_bytes = 0;
    let mut line = Vec::new();
    loop {
        line.clear();
        if !read_header_line(input, &mut line, MAX_HEADER_BYTES - header_bytes)? {
            if header_bytes == 0 {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete JSON-RPC header",
            ));
        }
        header_bytes += line.len();
        let line = std::str::from_utf8(&line)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid JSON-RPC header"))?;
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (name, value) = line
            .trim_end_matches(['\r', '\n'])
            .split_once(':')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid JSON-RPC header"))?;
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate Content-Length",
                ));
            }
            content_length = Some(value.trim().parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length")
            })?);
        }
    }
    let length = content_length
        .filter(|length| *length <= MAX_FRAME_BYTES)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length"))?;
    let mut frame = vec![0; length];
    input.read_exact(&mut frame)?;
    Ok(Some(frame))
}

fn read_header_line<R: BufRead>(
    input: &mut R,
    line: &mut Vec<u8>,
    limit: usize,
) -> io::Result<bool> {
    loop {
        let (read, newline) = {
            let available = input.fill_buf()?;
            if available.is_empty() {
                return Ok(!line.is_empty());
            }
            let read = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |index| index + 1);
            if line.len().saturating_add(read) > limit {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "JSON-RPC header is too large",
                ));
            }
            line.extend_from_slice(&available[..read]);
            (read, available[read - 1] == b'\n')
        };
        input.consume(read);
        if newline {
            return Ok(true);
        }
    }
}

fn write_frame<W: Write>(output: &mut W, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid JSON-RPC response"))?;
    write!(output, "Content-Length: {}\r\n\r\n", body.len())?;
    output.write_all(&body)?;
    output.flush()
}

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: &'static str,
}

struct Outcome {
    response: Option<Value>,
    notifications: Vec<Value>,
    exit: Option<i32>,
}

#[derive(Default)]
struct Server {
    workspace_roots: Vec<PathBuf>,
    documents: HashMap<String, i64>,
    snapshot: u64,
    initialized: bool,
    shutdown: bool,
}

impl Server {
    fn dispatch(&mut self, message: &Value) -> Outcome {
        let id = message.get("id").cloned();
        let mut notifications = Vec::new();
        let method = message.get("method").and_then(Value::as_str);
        let valid_id = id
            .as_ref()
            .is_none_or(|id| id.is_string() || id.is_number() || id.is_null());
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || method.is_none()
            || !valid_id
        {
            return Outcome {
                response: Some(rpc_error(
                    if valid_id {
                        id.unwrap_or(Value::Null)
                    } else {
                        Value::Null
                    },
                    -32600,
                    "Invalid request.",
                )),
                notifications,
                exit: None,
            };
        }
        let method = method.unwrap_or_default();
        let params = message.get("params").unwrap_or(&Value::Null);
        if method == "exit" {
            return Outcome {
                response: None,
                notifications,
                exit: Some(if self.shutdown { 0 } else { 1 }),
            };
        }
        let result = self.handle(method, params, &mut notifications);
        let response = id.map(|id| match result {
            Ok(Some(result)) => rpc_result(id, result),
            Ok(None) => rpc_error(id, -32601, "Method not found."),
            Err(error) => rpc_error(id, error.code, error.message),
        });
        Outcome {
            response,
            notifications,
            exit: None,
        }
    }

    fn handle(
        &mut self,
        method: &str,
        params: &Value,
        notifications: &mut Vec<Value>,
    ) -> Result<Option<Value>, RpcError> {
        match method {
            "initialize" => {
                let params = object(params)?;
                if self.initialized {
                    return Err(RpcError {
                        code: -32600,
                        message: "Server is already initialized.",
                    });
                }
                self.workspace_roots = workspace_roots_from_initialize(params);
                if self.workspace_roots.is_empty() {
                    self.workspace_roots.extend(env::current_dir());
                }
                self.initialized = true;
                Ok(Some(json!({
                    "capabilities": {
                        "textDocumentSync": {"openClose": true, "change": 1}
                    },
                    "serverInfo": {
                        "name": "rydantic",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                })))
            }
            "initialized" | "$/cancelRequest" => Ok(None),
            "shutdown" => {
                self.shutdown = true;
                Ok(Some(Value::Null))
            }
            "textDocument/didOpen" => {
                let document = params
                    .get("textDocument")
                    .ok_or(RpcError::invalid_params())?;
                let document = object(document)?;
                let uri = required_string(document, "uri")?;
                if document.get("text").and_then(Value::as_str).is_none() {
                    return Err(RpcError::invalid_params());
                }
                let version = document
                    .get("version")
                    .and_then(Value::as_i64)
                    .ok_or(RpcError::invalid_params())?;
                self.documents.insert(uri.to_owned(), version);
                self.advance_snapshot(notifications)?;
                Ok(None)
            }
            "textDocument/didChange" => {
                let document = params
                    .get("textDocument")
                    .ok_or(RpcError::invalid_params())?;
                let document = object(document)?;
                let uri = required_string(document, "uri")?;
                let changes = params
                    .get("contentChanges")
                    .and_then(Value::as_array)
                    .ok_or(RpcError::invalid_params())?;
                if changes
                    .last()
                    .and_then(|change| change.get("text"))
                    .and_then(Value::as_str)
                    .is_none()
                {
                    return Err(RpcError::invalid_params());
                }
                let version = document
                    .get("version")
                    .and_then(Value::as_i64)
                    .ok_or(RpcError::invalid_params())?;
                self.documents.insert(uri.to_owned(), version);
                self.advance_snapshot(notifications)?;
                Ok(None)
            }
            "textDocument/didClose" => {
                let document = params
                    .get("textDocument")
                    .ok_or(RpcError::invalid_params())?;
                let document = object(document)?;
                let uri = required_string(document, "uri")?;
                if self.documents.remove(uri).is_some() {
                    self.advance_snapshot(notifications)?;
                }
                Ok(None)
            }
            "workspace/didChangeConfiguration" | "workspace/didChangeWatchedFiles" => {
                object(params)?;
                self.advance_snapshot(notifications)?;
                Ok(None)
            }
            "workspace/didChangeWorkspaceFolders" => {
                let params = object(params)?;
                let event = object(params.get("event").ok_or(RpcError::invalid_params())?)?;
                if let Some(folders) = event.get("removed").and_then(Value::as_array) {
                    for root in folders
                        .iter()
                        .filter_map(|folder| folder.get("uri"))
                        .filter_map(Value::as_str)
                        .filter_map(file_uri_path)
                    {
                        self.workspace_roots
                            .retain(|existing| !same_path(existing, &root));
                    }
                }
                if let Some(folders) = event.get("added").and_then(Value::as_array) {
                    for root in folders
                        .iter()
                        .filter_map(|folder| folder.get("uri"))
                        .filter_map(Value::as_str)
                        .filter_map(file_uri_path)
                    {
                        if !self
                            .workspace_roots
                            .iter()
                            .any(|existing| same_path(existing, &root))
                        {
                            self.workspace_roots.push(root);
                        }
                    }
                }
                self.advance_snapshot(notifications)?;
                Ok(None)
            }
            "typeServer/getSupportedProtocolVersion" => Ok(Some(json!(VERSION))),
            "typeServer/getSnapshot" => Ok(Some(json!(self.snapshot))),
            "typeServer/getPythonSearchPaths" => {
                let params = object(params)?;
                let from_uri = required_string(params, "fromUri")?;
                self.check_snapshot(params)?;
                Ok(Some(json!(self.search_paths(from_uri))))
            }
            "typeServer/resolveImport" => {
                let params = object(params)?;
                let source_uri = required_string(params, "sourceUri")?;
                self.check_snapshot(params)?;
                let descriptor = params
                    .get("moduleDescriptor")
                    .ok_or(RpcError::invalid_params())?;
                let leading_dots = descriptor
                    .get("leadingDots")
                    .and_then(Value::as_u64)
                    .ok_or(RpcError::invalid_params())?;
                let name_parts = descriptor
                    .get("nameParts")
                    .and_then(Value::as_array)
                    .ok_or(RpcError::invalid_params())?;
                let mut parts = Vec::with_capacity(name_parts.len());
                for part in name_parts {
                    let part = part.as_str().ok_or(RpcError::invalid_params())?;
                    if part.is_empty()
                        || part
                            .bytes()
                            .any(|byte| matches!(byte, 0 | b'.' | b'/' | b'\\' | b':'))
                    {
                        return Err(RpcError::invalid_params());
                    }
                    parts.push(part.to_owned());
                }
                let options = params.get("options").map(object).transpose()?;
                let skip_file_check =
                    match options.and_then(|options| options.get("skipFileNeededCheck")) {
                        Some(value) => value.as_bool().ok_or(RpcError::invalid_params())?,
                        None => false,
                    };
                if leading_dots > 128 {
                    return Err(RpcError::invalid_params());
                }
                let resolved = self
                    .resolve_import(source_uri, leading_dots as usize, &parts, skip_file_check)
                    .and_then(|path| file_path_uri(&path));
                Ok(Some(resolved.map_or(Value::Null, Value::String)))
            }
            "typeServer/getComputedType"
            | "typeServer/getDeclaredType"
            | "typeServer/getExpectedType" => {
                let params = object(params)?;
                self.check_snapshot(params)?;
                if params.get("arg").and_then(Value::as_object).is_none() {
                    return Err(RpcError::invalid_params());
                }
                // shortcut: type queries return no result, until a Python type analyzer is added.
                Ok(Some(Value::Null))
            }
            "typeServer/connection" => {
                let params = object(params)?;
                let kind = required_string(params, "kind")?;
                let operation = required_string(params, "type")?;
                if !["open", "close"].contains(&operation) || kind != "ipc" {
                    return Err(RpcError::invalid_params());
                }
                if let Some(args) = params.get("args") {
                    let Some(args) = args.as_array() else {
                        return Err(RpcError::invalid_params());
                    };
                    if ![1, 2].contains(&args.len()) || args.iter().any(|arg| !arg.is_string()) {
                        return Err(RpcError::invalid_params());
                    }
                }
                Ok(Some(json!({
                    "success": false,
                    "message": "Multi-connection is not supported."
                })))
            }
            _ => Err(RpcError {
                code: -32601,
                message: "Method not found.",
            }),
        }
    }

    fn check_snapshot(&self, params: &Map<String, Value>) -> Result<(), RpcError> {
        let snapshot = params
            .get("snapshot")
            .and_then(Value::as_u64)
            .ok_or(RpcError::invalid_params())?;
        if snapshot != self.snapshot {
            return Err(RpcError {
                code: -32802,
                message: "Snapshot is stale.",
            });
        }
        Ok(())
    }

    fn advance_snapshot(&mut self, notifications: &mut Vec<Value>) -> Result<(), RpcError> {
        let old = self.snapshot;
        let new = old.checked_add(1).ok_or(RpcError {
            code: -32603,
            message: "Snapshot counter exhausted.",
        })?;
        self.snapshot = new;
        notifications.push(json!({
            "jsonrpc": "2.0",
            "method": "typeServer/snapshotChanged",
            "params": {"old": old, "new": new}
        }));
        Ok(())
    }

    fn search_paths(&self, from_uri: &str) -> Vec<String> {
        let context = file_uri_path(from_uri);
        let context = context.map(|path| {
            if path.is_dir() {
                path
            } else {
                path.parent().unwrap_or(&path).to_path_buf()
            }
        });
        let root = context
            .as_ref()
            .and_then(|context| {
                self.workspace_roots
                    .iter()
                    .filter(|root| context.starts_with(root))
                    .max_by_key(|root| root.components().count())
            })
            .cloned()
            .or(context)
            .or_else(|| self.workspace_roots.first().cloned());
        let mut paths = Vec::new();
        let mut seen = HashSet::new();
        if let Some(root) = root {
            push_directory(&mut paths, &mut seen, root.clone());
            push_directory(&mut paths, &mut seen, root.join("src"));
            for environment in [root.join(".venv"), root.join("venv")] {
                add_environment_paths(&mut paths, &mut seen, &environment);
            }
        }
        if let Some(python_path) = env::var_os("PYTHONPATH") {
            for path in env::split_paths(&python_path) {
                push_directory(&mut paths, &mut seen, path);
            }
        }
        if let Some(environment) = env::var_os("VIRTUAL_ENV") {
            add_environment_paths(&mut paths, &mut seen, &PathBuf::from(environment));
        }
        paths
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }

    fn resolve_import(
        &self,
        source_uri: &str,
        leading_dots: usize,
        parts: &[String],
        skip_file_check: bool,
    ) -> Option<PathBuf> {
        if leading_dots == 0 {
            return self
                .search_paths(source_uri)
                .into_iter()
                .map(PathBuf::from)
                .find_map(|root| resolve_module(&root, parts, skip_file_check));
        }
        let source_path = file_uri_path(source_uri)?;
        let mut root = source_path.parent()?.to_path_buf();
        for _ in 1..leading_dots {
            root = root.parent()?.to_path_buf();
        }
        resolve_module(&root, parts, skip_file_check)
    }
}

impl RpcError {
    fn invalid_params() -> Self {
        Self {
            code: -32602,
            message: "Invalid parameters.",
        }
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, RpcError> {
    value.as_object().ok_or_else(RpcError::invalid_params)
}

fn required_string<'a>(value: &'a Map<String, Value>, key: &str) -> Result<&'a str, RpcError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(RpcError::invalid_params)
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn rpc_error(id: Value, code: i64, message: &'static str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn workspace_roots_from_initialize(params: &Map<String, Value>) -> Vec<PathBuf> {
    let roots = params
        .get("workspaceFolders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|folder| folder.get("uri"))
        .filter_map(Value::as_str)
        .filter_map(file_uri_path)
        .collect::<Vec<_>>();
    if !roots.is_empty() {
        return roots;
    }
    params
        .get("rootUri")
        .and_then(Value::as_str)
        .and_then(file_uri_path)
        .or_else(|| {
            params
                .get("rootPath")
                .and_then(Value::as_str)
                .map(PathBuf::from)
        })
        .into_iter()
        .collect()
}

fn same_path(left: &Path, right: &Path) -> bool {
    fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf())
        == fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf())
}

fn file_uri_path(uri: &str) -> Option<PathBuf> {
    Url::parse(uri).ok()?.to_file_path().ok()
}

fn file_path_uri(path: &Path) -> Option<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().ok()?.join(path)
    };
    Url::from_file_path(absolute)
        .ok()
        .map(|uri| uri.to_string())
}

fn push_directory(paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if !path.is_dir() {
        return;
    }
    let path = if path.is_absolute() {
        path
    } else if let Ok(current_dir) = env::current_dir() {
        current_dir.join(path)
    } else {
        return;
    };
    let key = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    if seen.insert(key) {
        paths.push(path);
    }
}

fn add_environment_paths(
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    environment: &Path,
) {
    push_directory(paths, seen, environment.join("Lib").join("site-packages"));
    let lib = environment.join("lib");
    push_directory(paths, seen, lib.join("site-packages"));
    if let Ok(entries) = fs::read_dir(lib) {
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with("python") {
                push_directory(paths, seen, path.join("site-packages"));
            }
        }
    }
}

fn resolve_module(root: &Path, parts: &[String], skip_file_check: bool) -> Option<PathBuf> {
    let mut module = root.to_path_buf();
    for part in parts {
        module.push(part);
    }
    let candidates = [
        module.join("__init__.pyi"),
        module.join("__init__.py"),
        module.with_extension("pyi"),
        module.with_extension("py"),
    ];
    for candidate in &candidates {
        if candidate.is_file() {
            return Some(candidate.clone());
        }
    }
    if parts.is_empty() && module.is_dir() {
        return Some(module);
    }
    if skip_file_check {
        return Some(if parts.is_empty() {
            module.join("__init__.py")
        } else {
            module.with_extension("py")
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Cursor,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = env::temp_dir().join(format!(
                "rydantic-tsp-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn uri(&self, path: &Path) -> String {
            Url::from_file_path(path).unwrap().to_string()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn frame(message: Value) -> Vec<u8> {
        let body = serde_json::to_vec(&message).unwrap();
        format!("Content-Length: {}\r\n\r\n", body.len())
            .into_bytes()
            .into_iter()
            .chain(body)
            .collect()
    }

    #[test]
    fn bounds_header_allocation_before_newline() {
        let input = Cursor::new(vec![b'x'; MAX_HEADER_BYTES + 1]);
        let mut reader = BufReader::with_capacity(32, input);
        let mut line = Vec::new();
        let error = read_header_line(&mut reader, &mut line, MAX_HEADER_BYTES).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(line.len(), MAX_HEADER_BYTES);
    }

    #[test]
    fn duplicate_document_close_does_not_advance_snapshot() {
        let mut server = Server::default();
        let uri = "file:///project/model.py";
        server.documents.insert(uri.to_owned(), 1);
        let params = json!({"textDocument":{"uri":uri}});
        let mut notifications = Vec::new();

        server
            .handle("textDocument/didClose", &params, &mut notifications)
            .unwrap();
        let snapshot = server.snapshot;
        let notification_count = notifications.len();
        server
            .handle("textDocument/didClose", &params, &mut notifications)
            .unwrap();

        assert_eq!(snapshot, 1);
        assert_eq!(server.snapshot, snapshot);
        assert_eq!(notifications.len(), notification_count);
    }

    #[test]
    fn search_paths_and_absolute_import_use_from_uri_workspace() {
        let temp = TempDir::new();
        let first = temp.0.join("first");
        let second = temp.0.join("second");
        fs::create_dir_all(first.join("src")).unwrap();
        fs::create_dir_all(second.join("src")).unwrap();
        fs::create_dir_all(second.join(".venv/lib/python3.12/site-packages")).unwrap();
        fs::write(second.join("src/only_second.py"), "").unwrap();

        let params = json!({
            "workspaceFolders": [
                {"uri":temp.uri(&first)},
                {"uri":temp.uri(&second)}
            ]
        });
        let mut server = Server::default();
        server
            .handle("initialize", &params, &mut Vec::new())
            .unwrap();

        let source_uri = temp.uri(&second.join("src/main.py"));
        let paths = server.search_paths(&source_uri);
        assert!(paths.contains(&second.to_string_lossy().into_owned()));
        let site_packages = second.join(".venv/lib/python3.12/site-packages");
        assert!(paths
            .iter()
            .any(|path| same_path(Path::new(path), &site_packages)));
        assert!(!paths.contains(&first.to_string_lossy().into_owned()));
        assert_eq!(
            server.resolve_import(&source_uri, 0, &["only_second".into()], false),
            Some(second.join("src/only_second.py"))
        );
    }

    #[test]
    fn package_initializer_precedes_same_named_module() {
        let temp = TempDir::new();
        let root = temp.0.join("root");
        fs::create_dir_all(root.join("foo")).unwrap();
        fs::write(root.join("foo.pyi"), "").unwrap();
        fs::write(root.join("foo.py"), "").unwrap();
        fs::write(root.join("foo/__init__.py"), "").unwrap();
        fs::write(root.join("foo/__init__.pyi"), "").unwrap();

        assert_eq!(
            resolve_module(&root, &["foo".into()], false),
            Some(root.join("foo/__init__.pyi"))
        );
    }

    #[test]
    fn speaks_tsp_and_advances_stale_snapshots() {
        let input = [
            frame(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})),
            frame(json!({"jsonrpc":"2.0","method":"initialized","params":{}})),
            frame(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///tmp/model.py","version":1,"text":""}}})),
            frame(json!({"jsonrpc":"2.0","id":2,"method":"typeServer/getSupportedProtocolVersion"})),
            frame(json!({"jsonrpc":"2.0","id":3,"method":"typeServer/getPythonSearchPaths","params":{"fromUri":"file:///tmp/model.py","snapshot":0}})),
            frame(json!({"jsonrpc":"2.0","id":4,"method":"shutdown"})),
            frame(json!({"jsonrpc":"2.0","method":"exit"})),
        ]
        .concat();
        let mut reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();
        assert_eq!(serve(&mut reader, &mut output).unwrap(), 0);

        let mut reader = BufReader::new(output.as_slice());
        let mut responses = Vec::new();
        while let Some(frame) = read_frame(&mut reader).unwrap() {
            responses.push(serde_json::from_slice::<Value>(&frame).unwrap());
        }
        assert_eq!(
            responses[0]["result"]["capabilities"]["textDocumentSync"]["change"],
            1
        );
        assert_eq!(responses[1]["method"], "typeServer/snapshotChanged");
        assert_eq!(responses[1]["params"], json!({"old":0,"new":1}));
        assert_eq!(responses[2]["result"], VERSION);
        assert_eq!(responses[3]["error"]["code"], -32802);
        assert_eq!(responses[4]["result"], Value::Null);
    }
}
