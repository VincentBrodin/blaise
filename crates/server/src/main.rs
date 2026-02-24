mod api;
mod dto;
mod state;

use crate::state::{AllocatorPool, AppState};
use axum::routing::{get, post};
use blaise::{gtfs::GtfsReader, realtime::Realtime, repository::Repository};
use std::{env, fs::File, io::Read, path::Path, str::FromStr, sync::Arc, time::Instant};
use tokio::{net::TcpListener, sync::RwLock};
use tracing::{Level, info, warn};

const DEFAULT_PORT: u32 = 3000;
const DEFAULT_ALLOC_COUNT: usize = 32;
const DEFAULT_LOG_LEVEL: Level = Level::INFO;

#[tokio::main]
async fn main() {
    if let Some(err) = dotenv::dotenv().err() {
        println!("Failed to load .env file: {}", err)
    }

    let log_level = match env::var("LOG_LEVEL") {
        Ok(level_str) => Level::from_str(&level_str).unwrap_or(DEFAULT_LOG_LEVEL),
        Err(_) => DEFAULT_LOG_LEVEL,
    };

    tracing_subscriber::fmt()
        .with_file(false)
        .with_target(false)
        .with_max_level(log_level)
        .init();

    let start_logo = include_str!("../start_logo.txt");
    println!("{}", start_logo);

    // Load env vars
    let gtfs_data_path = env::var("GTFS_DATA_PATH")
        .map(|path_str| Path::new(&path_str).to_owned())
        .expect("Missing GTFS_DATA_PATH");

    let gtfs_rt_data_path = env::var("GTFS_RT_DATA_PATH")
        .map(|path_str| Path::new(&path_str).to_owned())
        .expect("Missing GTFS_RT_DATA_PATH");

    let alloc_count = env::var("ALLOCATOR_COUNT")
        .map(|value| {
            value
                .parse()
                .expect("Failed to parse the given allocator count")
        })
        .unwrap_or(DEFAULT_ALLOC_COUNT);

    let port = env::var("PORT")
        .map(|value| value.parse().expect("Failed to parse the given port"))
        .unwrap_or(DEFAULT_PORT);

    // Built app state
    let app_state = AppState {
        repository: RwLock::new(None),
        realtime: RwLock::new(None),
        allocator_pool: RwLock::new(None),
        allocator_count: alloc_count,
        gtfs_data_path,
        gtfs_rt_data_path,
    };

    if app_state.gtfs_data_path.exists() {
        info!("Reading GTFS data...");
        let mut now = Instant::now();
        let data = GtfsReader::new()
            .from_zip_cache(&app_state.gtfs_data_path)
            .expect("Failed to build gtfs reader")
            .par_read()
            .expect("Failed to read gtfs data");
        info!("Reading GTFS data took {:?}", now.elapsed());
        info!("Loading GTFS data...");
        now = Instant::now();
        let repo = Repository::new().load_gtfs(data);
        info!("Loading GTFS data took {:?}", now.elapsed());
        info!("Loading RT data...");
        now = Instant::now();
        let mut rt_file = File::open(&app_state.gtfs_rt_data_path).expect("Failed to open rt file");
        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        rt_file
            .read_to_end(&mut buf)
            .expect("Failed to read rt file");
        let rt = Realtime::new(&repo)
            .load(&buf, &repo)
            .expect("Failed to load rt file");
        info!("Loading RT data took {:?}", now.elapsed());
        info!("Allocating {alloc_count} pools...");
        let now = Instant::now();
        let pool = AllocatorPool::new(alloc_count, &repo);
        info!("Allocating {alloc_count} pools took {:?}", now.elapsed());
        let _ = app_state.allocator_pool.write().await.replace(pool);
        let _ = app_state.repository.write().await.replace(repo);
        let _ = app_state.realtime.write().await.replace(rt);
    } else {
        warn!("No GTFS data found.");
    }

    info!("Starting server...");
    let app = axum::Router::new()
        .route("/search/area", get(api::search_areas))
        .route("/search/stop", get(api::search_stops))
        .route("/near/area", get(api::near_areas))
        .route("/near/stop", get(api::near_stops))
        .route("/routing", get(api::routing))
        .route("/gtfs/download", post(api::download_gtfs))
        .route("/gtfs/age", get(api::age))
        .route("/gtfsrt/download", post(api::download_gtfs_rt))
        .with_state(Arc::new(app_state));
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("Failed to create listener");
    info!("Listening to port {port}");
    axum::serve(listener, app)
        .await
        .expect("Failed to serve listener");
}
