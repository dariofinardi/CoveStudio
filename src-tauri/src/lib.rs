use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use tokio::sync::{mpsc, oneshot};

/// State managed by Tauri: the actual port the axum server bound to.
/// Filled in once the server has started up (via the `port_tx` oneshot).
/// Read by the `api_base_url` invoke handler so the frontend can
/// discover where the backend is without a build-time constant.
///
/// Stored as `Arc<Mutex<Option<String>>>` so the same handle can be
/// cloned into the background port-watch task AND managed by Tauri
/// for invoke-handler reads, without resorting to raw pointers.
#[derive(Clone)]
struct ApiBaseUrl(Arc<Mutex<Option<String>>>);

impl ApiBaseUrl {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
    fn set(&self, url: String) {
        if let Ok(mut g) = self.0.lock() {
            *g = Some(url);
        }
    }
    fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|g| g.clone())
    }
}

/// Resolve `<home>/cove-studio-data/` and ensure it exists. Used for
/// both the SQLite DB (`cove-studio.db`, owned by the `cove_studio` crate) and the
/// release-build log file (`cove-studio.log`, set up below). Returning
/// `None` is non-fatal — callers fall back to "no extra logging" so a
/// missing HOME env doesn't bring the shell down.
fn ensure_data_dir() -> Option<std::path::PathBuf> {
    cove_studio::product::home_dir()?;
    let dir = cove_studio::product::data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Pick a free port in the ephemeral range to bind the embedded axum
/// server on. Walks up to 20 random picks in `49152..=65535`,
/// pre-binding each via a synchronous `std::net::TcpListener` (and
/// immediately dropping the listener) to confirm the port is free
/// right now. On 20 consecutive collisions — practically unreachable
/// since the search space is ~16k ports — falls back to `0` so the
/// OS still gets to pick something rather than killing startup.
///
/// Why not just keep `port = 0`: with several Tauri / Electron desktop
/// apps that all bind axum on localhost, the OS ephemeral pool can
/// hand us a port that *another* desktop app freed milliseconds ago
/// and is about to rebind. Choosing our own random port and verifying
/// it's free decouples our bind from the OS reuse window and makes
/// "did axum start on a sensible port" easier to debug from the log.
///
/// There is still a benign TOCTOU window between this pre-bind check
/// and the real `tokio::net::TcpListener::bind` inside
/// `cove_studio::run_server_with_channels`. In the rare collision case the
/// real bind returns AddrInUse, the spawn logs the error, and the
/// frontend's invoke handler returns an empty URL — the user can
/// relaunch the app and a fresh random pick will almost certainly
/// land on a different free port.
fn pick_free_random_port() -> u16 {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
    use rand::Rng;

    let mut rng = rand::rng();
    for _ in 0..20 {
        let port: u16 = rng.random_range(49152..=65535);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        if TcpListener::bind(addr).is_ok() {
            return port;
        }
    }
    // Vanishingly unlikely 20-in-a-row collision — let the OS pick.
    0
}

/// Entry point called by main.rs.
/// Starts the axum server as a background tokio task, then launches Tauri.
pub fn run() {
    dotenvy::dotenv().ok();

    // Init tracing once. Two sinks:
    //   - stdout/stderr (`fmt::layer()`)  — useful in dev (`tauri dev`,
    //     `cargo run`); detached in MSI installs because main.rs sets
    //     `windows_subsystem = "windows"`.
    //   - file (`tracing-appender::rolling::never`) — writes to
    //     `<home>/cove-studio-data/cove-studio.log`, always on. This is
    //     the only sink that survives the windowed release build, so
    //     "the backend died silently" can finally be triaged by
    //     opening the log file. We keep the worker guard alive for
    //     the lifetime of the process so the non-blocking writer
    //     keeps flushing. The guard *must* outlive every span/event
    //     it might receive, hence the `Box::leak` — the process ends
    //     when Tauri ends, so leaking until exit is harmless.
    let log_dir = ensure_data_dir();
    let file_appender = log_dir
        .as_ref()
        .map(|d| tracing_appender::rolling::never(d, cove_studio::product::SHELL_LOG_FILE_NAME));
    let (file_writer, file_guard) = match file_appender {
        Some(a) => {
            let (w, g) = tracing_appender::non_blocking(a);
            (Some(w), Some(g))
        }
        None => (None, None),
    };
    if let Some(g) = file_guard {
        Box::leak(Box::new(g));
    }
    let file_layer = file_writer.map(|w| {
        tracing_subscriber::fmt::layer()
            .with_writer(w)
            .with_ansi(false)
            .with_target(true)
    });

    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| {
                    "cove_studio=debug,cove_studio_desktop_lib=debug,tower_http=info".into()
                }),
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(file_layer)
        .try_init();

    if let Some(d) = log_dir.as_ref() {
        tracing::info!(
            "[tauri] tracing → {}",
            d.join(cove_studio::product::SHELL_LOG_FILE_NAME).display()
        );
    } else {
        tracing::warn!(
            "[tauri] HOME/USERPROFILE not set — file logging disabled"
        );
    }


    // Biometric channel: axum sends requests, Tauri processes them with HWND
    let (bio_tx, mut bio_rx) = mpsc::channel::<cove_studio::BiometricRequest>(4);

    // Port discovery channel: axum reports the OS-assigned port back so
    // the Tauri shell can hand it to the frontend on demand. Default
    // mode is "OS picks a free high port" (PORT env unset). PORT can
    // still pin a specific port for the standalone-backend dev story
    // (running the frontend in a regular browser at :3000 with
    // NEXT_PUBLIC_API_BASE_URL=http://localhost:<port>).
    let (port_tx, port_rx) = oneshot::channel::<u16>();
    let api_base = ApiBaseUrl::new();
    let api_base_for_task = api_base.clone();

    // Spawn the axum server on a background tokio runtime. Errors
    // returned from `run_server_with_channels` were previously logged
    // and otherwise silently dropped — `port_tx` got freed without
    // sending, the Tauri shell's `api_base_url` invoke handler stayed
    // empty forever, and the frontend gave up with "Failed to fetch".
    // Now we ALSO write the error chain to stderr so a developer
    // running the installed exe from a console (or in dev) sees the
    // failure without having to hunt for the log file.
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("[cove-studio:fatal] failed to build tokio runtime: {e}");
                tracing::error!("[tauri] failed to build tokio runtime: {e}");
                return;
            }
        };
        rt.block_on(async {
            let port: u16 = std::env::var("PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(pick_free_random_port);
            tracing::info!("[tauri] embedded axum will bind on 127.0.0.1:{port}");
            if let Err(e) = cove_studio::run_server_with_channels(
                port,
                Some(bio_tx),
                Some(port_tx),
            ).await {
                // Walk the error chain so the *root cause* lands in
                // the log — not just the topmost "axum server error".
                let mut chain = vec![format!("{e}")];
                let mut src: Option<&dyn std::error::Error> = e.source();
                while let Some(inner) = src {
                    chain.push(format!("{inner}"));
                    src = inner.source();
                }
                let joined = chain.join(" -> ");
                tracing::error!("[tauri] axum server failed: {joined}");
                eprintln!("[cove-studio:fatal] axum server failed: {joined}");
            }
        });
    });

    // Background task: wait for the axum server to report its bound
    // port, then stash it in the shared `ApiBaseUrl` handle so the
    // `api_base_url` invoke handler can read it. Done out of Tauri's
    // `setup` so the main thread isn't blocked — the IPC roundtrip
    // from the frontend's first `invoke("api_base_url")` races against
    // bind, but axum bind is sub-millisecond so the race resolves
    // before the user can trigger any user-driven fetch.
    tauri::async_runtime::spawn(async move {
        if let Ok(port) = port_rx.await {
            let url = format!("http://127.0.0.1:{port}");
            tracing::info!("[tauri] api_base_url resolved to {url}");
            api_base_for_task.set(url);
        } else {
            tracing::warn!(
                "[tauri] axum server never reported a port — \
                 api_base_url will stay None and the frontend will \
                 fall back to NEXT_PUBLIC_API_BASE_URL"
            );
        }
    });

    tauri::Builder::default()
        .manage(api_base)
        .invoke_handler(tauri::generate_handler![
            open_external_url,
            open_external_path,
            api_base_url,
            pick_folder
        ])
        .setup(move |app| {

            #[cfg(debug_assertions)]
            app.get_webview_window("main")
                .expect("main window")
                .open_devtools();

            // Spawn task that handles biometric requests using the Tauri window HWND
            let window = app.get_webview_window("main").expect("main window");
            tauri::async_runtime::spawn(async move {
                tracing::info!("[tauri-bio] biometric channel listener started");
                while let Some((reason, reply)) = bio_rx.recv().await {
                    tracing::info!("[tauri-bio] received request: '{reason}'");
                    let result = verify_with_window(&window, &reason);
                    tracing::info!("[tauri-bio] verify_with_window result: {:?}", result);
                    let _ = reply.send(result);
                }
                tracing::warn!("[tauri-bio] biometric channel closed");
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Call Windows Hello from the Tauri task context.
///
/// Uses the COM interop API `IUserConsentVerifierInterop::RequestVerificationForWindowAsync`
/// so the OS dialog is parented to our Tauri window — without this, the dialog
/// can appear behind the app window and the user may miss it.
#[cfg(target_os = "windows")]
fn verify_with_window(
    window: &tauri::WebviewWindow,
    reason: &str,
) -> Result<bool, String> {
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier,
        UserConsentVerifierAvailability,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    use windows::core::HSTRING;
    use windows_future::IAsyncOperation;

    tracing::info!("[tauri-bio] verify_with_window: checking availability");
    let avail = UserConsentVerifier::CheckAvailabilityAsync()
        .map_err(|e: windows::core::Error| { tracing::error!("[tauri-bio] CheckAvailabilityAsync error: {e}"); e.to_string() })?
        .get()
        .map_err(|e: windows::core::Error| { tracing::error!("[tauri-bio] availability .get() error: {e}"); e.to_string() })?;

    tracing::info!("[tauri-bio] availability value: {}", avail.0);
    if !matches!(avail, UserConsentVerifierAvailability::Available) {
        return Err(format!("Windows Hello not available (code {})", avail.0));
    }

    // Bring our Tauri window to the foreground so the OS-level dialog inherits
    // focus from a visible parent. Best-effort — failure isn't fatal.
    let _ = window.set_focus();
    let _ = window.unminimize();

    let raw_hwnd = window
        .hwnd()
        .map_err(|e| { tracing::error!("[tauri-bio] window.hwnd() error: {e}"); format!("hwnd error: {e}") })?;
    let hwnd = HWND(raw_hwnd.0 as *mut core::ffi::c_void);
    tracing::info!("[tauri-bio] obtained HWND: {:?}", hwnd.0);

    let interop: IUserConsentVerifierInterop =
        windows::core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
            .map_err(|e: windows::core::Error| {
                tracing::error!("[tauri-bio] interop factory error: {e}");
                format!("interop factory error: {e}")
            })?;

    let message = HSTRING::from(reason);
    tracing::info!("[tauri-bio] calling RequestVerificationForWindowAsync('{reason}')");
    let op: IAsyncOperation<UserConsentVerificationResult> = unsafe {
        interop
            .RequestVerificationForWindowAsync(hwnd, &message)
            .map_err(|e: windows::core::Error| { tracing::error!("[tauri-bio] interop call error: {e}"); e.to_string() })?
    };
    let result: UserConsentVerificationResult = op
        .get()
        .map_err(|e: windows::core::Error| { tracing::error!("[tauri-bio] .get() error: {e}"); e.to_string() })?;

    tracing::info!("[tauri-bio] verification result code: {}", result.0);
    Ok(matches!(result, UserConsentVerificationResult::Verified))
}

#[cfg(not(target_os = "windows"))]
fn verify_with_window(
    _window: &tauri::WebviewWindow,
    _reason: &str,
) -> Result<bool, String> {
    Err("Biometric not supported on this platform".into())
}

/// Open a URL in the system default browser.
///
/// Tauri's WebView intercepts plain `<a target="_blank">` clicks and
/// opens them inside the same WebView, which makes the in-app shell
/// behave like a mini-browser. Routing through the OS launcher (via
/// the `open` crate) hands the URL to whatever the user's default
/// browser is, which is what they expect when clicking "Apri su
/// EUR-Lex".
///
/// Validates the scheme — only `http://` and `https://` URLs are
/// accepted, so a malicious payload from a tool result can't
/// trigger a `file://` or `mailto:` action through this command.
/// Return the actual base URL of the embedded axum HTTP server.
///
/// The shell launches axum with `port = 0` so the OS picks a free
/// high port; we then store the resulting `http://127.0.0.1:<port>`
/// here. The frontend calls this once at boot to discover where to
/// `fetch()`. Returns an empty string before the server has reported
/// its port (so the frontend can fall back to NEXT_PUBLIC_API_BASE_URL).
#[tauri::command]
fn api_base_url(state: State<'_, ApiBaseUrl>) -> String {
    state.get().unwrap_or_default()
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(format!("rejected non-http(s) URL: {url}"));
    }
    open::that(&url).map_err(|e| e.to_string())
}

/// Open a *file path* with the OS's default associated application
/// — typically Microsoft Word / LibreOffice for `.docx`. Used by the
/// DocViewerPanel "Apri in Word" toolbar action so the user can run
/// Word's native Track Changes accept/reject workflow on a docx the
/// model generated.
///
/// Security model: the path is validated against the user's storage
/// root (`<home>/cove-studio-data/storage/`, the same base
/// `LocalStorage` uses) plus the OS temp dir as a permitted prefix.
/// Anything pointing elsewhere — a network share, `C:\Windows`, a
/// crafted path with `..` segments that escape the base — is
/// rejected before `open::that` is called. The frontend never
/// fabricates the path; it asks the authenticated backend endpoint
/// `GET /document/:id/file_path` for the absolute path and hands the
/// returned string straight to this command.
#[tauri::command]
fn open_external_path(path: String) -> Result<(), String> {
    use std::path::PathBuf;

    let raw = PathBuf::from(&path);
    let canonical = std::fs::canonicalize(&raw)
        .map_err(|e| format!("canonicalize {path}: {e}"))?;

    // Build the allowlist of acceptable prefixes. Canonicalize each
    // so the prefix comparison below is symmetric with the input.
    cove_studio::product::home_dir().ok_or_else(|| "USERPROFILE/HOME not set".to_string())?;
    let storage_default = cove_studio::product::data_subdir("storage");
    let storage_override = std::env::var("STORAGE_PATH").ok().map(PathBuf::from);

    let mut allowed: Vec<PathBuf> = vec![storage_default];
    if let Some(p) = storage_override {
        allowed.push(p);
    }
    allowed.push(std::env::temp_dir());

    let allowed_canonical: Vec<PathBuf> = allowed
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();

    let ok = allowed_canonical.iter().any(|root| canonical.starts_with(root));
    if !ok {
        return Err(format!(
            "rejected: path {} is outside the allowed storage roots",
            canonical.display()
        ));
    }

    open::that(&canonical).map_err(|e| e.to_string())
}

/// Open the native folder picker. Returns the selected absolute path,
/// or `None` if the user cancelled. Used by the Settings sync-folder
/// field so a path can be chosen instead of typed.
#[tauri::command]
async fn pick_folder() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|h| h.path().to_string_lossy().into_owned())
}
