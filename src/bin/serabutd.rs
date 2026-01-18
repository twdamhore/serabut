use axum::{
    extract::Query,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use clap::Parser;
use nix::ifaddrs::getifaddrs;
use serabut::{disarm, init_db, is_armed, open_db};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(name = "serabutd")]
#[command(about = "PXE boot management daemon")]
struct Cli {
    /// Network interface to bind to (e.g., enp1s0)
    #[arg(short, long)]
    interface: Option<String>,

    /// Port to listen on
    #[arg(short, long, default_value = "4123")]
    port: u16,
}

struct AppState {
    conn: Mutex<rusqlite::Connection>,
    data_dir: PathBuf,
}

fn get_interface_ip(interface: &str) -> Option<Ipv4Addr> {
    let addrs = getifaddrs().ok()?;
    for addr in addrs {
        if addr.interface_name == interface {
            if let Some(storage) = addr.address {
                if let Some(sockaddr) = storage.as_sockaddr_in() {
                    return Some(Ipv4Addr::from(sockaddr.ip()));
                }
            }
        }
    }
    None
}

fn create_app(conn: rusqlite::Connection, data_dir: PathBuf) -> Router {
    let state = Arc::new(AppState {
        conn: Mutex::new(conn),
        data_dir,
    });

    Router::new()
        .route("/boot", get(boot_handler))
        .route("/done", get(done_handler))
        .with_state(state)
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let bind_addr: IpAddr = match &cli.interface {
        Some(iface) => match get_interface_ip(iface) {
            Some(ip) => IpAddr::V4(ip),
            None => {
                eprintln!("error: no IPv4 address configured for interface {}", iface);
                return ExitCode::FAILURE;
            }
        },
        None => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
    };

    let conn = match open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: failed to open database: {}", e);
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = init_db(&conn) {
        eprintln!("error: failed to initialize database: {}", e);
        return ExitCode::FAILURE;
    }

    let app = create_app(conn, PathBuf::from(serabut::DATA_DIR));

    let bind = format!("{}:{}", bind_addr, cli.port);
    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: failed to bind to {}: {}", bind, e);
            return ExitCode::FAILURE;
        }
    };

    println!("serabutd listening on {}", bind);
    axum::serve(listener, app).await.unwrap();
    ExitCode::SUCCESS
}

async fn boot_handler(
    Query(params): Query<HashMap<String, String>>,
    state: axum::extract::State<Arc<AppState>>,
) -> impl IntoResponse {
    let Some(mac) = params.get("mac") else {
        return (StatusCode::BAD_REQUEST, "missing mac parameter".to_string());
    };

    let conn = state.conn.lock().unwrap();

    match is_armed(&conn, mac) {
        Ok(true) => {
            let path = state.data_dir.join(mac).join("boot.ipxe");
            match std::fs::read_to_string(&path) {
                Ok(content) => (StatusCode::OK, content),
                Err(_) => (StatusCode::NOT_FOUND, "boot.ipxe not found".to_string()),
            }
        }
        Ok(false) => (StatusCode::NOT_FOUND, "not armed".to_string()),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn done_handler(
    Query(params): Query<HashMap<String, String>>,
    state: axum::extract::State<Arc<AppState>>,
) -> impl IntoResponse {
    let Some(mac) = params.get("mac") else {
        return (StatusCode::BAD_REQUEST, "missing mac parameter".to_string());
    };

    let conn = state.conn.lock().unwrap();

    match disarm(&conn, mac, true) {
        Ok(_) => (StatusCode::OK, "done".to_string()),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use rusqlite::Connection;
    use std::fs;
    use tempfile::TempDir;
    use tower::ServiceExt;

    fn setup_test_app() -> (Router, TempDir) {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let temp_dir = TempDir::new().unwrap();
        let app = create_app(conn, temp_dir.path().to_path_buf());
        (app, temp_dir)
    }

    fn setup_test_app_with_armed_mac(mac: &str) -> (Router, TempDir) {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        serabut::arm(&conn, mac).unwrap();
        let temp_dir = TempDir::new().unwrap();
        let app = create_app(conn, temp_dir.path().to_path_buf());
        (app, temp_dir)
    }

    #[tokio::test]
    async fn test_boot_missing_mac_parameter() {
        let (app, _temp) = setup_test_app();

        let response = app
            .oneshot(Request::builder().uri("/boot").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"missing mac parameter");
    }

    #[tokio::test]
    async fn test_boot_not_armed() {
        let (app, _temp) = setup_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/boot?mac=aa-bb-cc-dd-ee-ff")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"not armed");
    }

    #[tokio::test]
    async fn test_boot_armed_but_file_missing() {
        let (app, _temp) = setup_test_app_with_armed_mac("aa-bb-cc-dd-ee-ff");

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/boot?mac=aa-bb-cc-dd-ee-ff")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"boot.ipxe not found");
    }

    #[tokio::test]
    async fn test_boot_armed_with_file() {
        let (app, temp_dir) = setup_test_app_with_armed_mac("aa-bb-cc-dd-ee-ff");

        // Create the boot.ipxe file
        let mac_dir = temp_dir.path().join("aa-bb-cc-dd-ee-ff");
        fs::create_dir_all(&mac_dir).unwrap();
        fs::write(mac_dir.join("boot.ipxe"), "#!ipxe\nboot").unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/boot?mac=aa-bb-cc-dd-ee-ff")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"#!ipxe\nboot");
    }

    #[tokio::test]
    async fn test_done_missing_mac_parameter() {
        let (app, _temp) = setup_test_app();

        let response = app
            .oneshot(Request::builder().uri("/done").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"missing mac parameter");
    }

    #[tokio::test]
    async fn test_done_success() {
        let (app, _temp) = setup_test_app_with_armed_mac("aa-bb-cc-dd-ee-ff");

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/done?mac=aa-bb-cc-dd-ee-ff")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"done");
    }

    #[tokio::test]
    async fn test_done_not_armed_still_succeeds() {
        let (app, _temp) = setup_test_app();

        // done with force=true always succeeds
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/done?mac=aa-bb-cc-dd-ee-ff")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"done");
    }

    #[tokio::test]
    async fn test_done_actually_disarms() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        serabut::arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap();

        let temp_dir = TempDir::new().unwrap();
        let state = Arc::new(AppState {
            conn: Mutex::new(conn),
            data_dir: temp_dir.path().to_path_buf(),
        });

        let app = Router::new()
            .route("/boot", get(boot_handler))
            .route("/done", get(done_handler))
            .with_state(state.clone());

        // First verify it's armed by checking boot returns "boot.ipxe not found" (not "not armed")
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/boot?mac=aa-bb-cc-dd-ee-ff")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"boot.ipxe not found"); // armed but no file

        // Call done
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/done?mac=aa-bb-cc-dd-ee-ff")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Now verify it's disarmed
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/boot?mac=aa-bb-cc-dd-ee-ff")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"not armed"); // now shows "not armed"
    }

    #[test]
    fn test_get_interface_ip_loopback() {
        // lo should always have 127.0.0.1
        let ip = get_interface_ip("lo");
        assert_eq!(ip, Some(std::net::Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn test_get_interface_ip_nonexistent() {
        let ip = get_interface_ip("nonexistent_interface_xyz");
        assert_eq!(ip, None);
    }
}
