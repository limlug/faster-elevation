use std::sync::Arc;
use bb8::Pool;
use bb8_postgres::PostgresConnectionManager;
use clap::Parser;
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use tokio_postgres::NoTls;
use crate::repos::geo_repo::{GeoRepository};
#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[arg(short, long)]
    pub regenerate: bool,
}
/// Structure representing a coordinate result.
#[derive(Clone, Serialize)]
pub struct CoordinateResult {
    /// Longitude of the coordinate.
    pub longitude: f64,
    /// Latitude of the coordinate.
    pub latitude: f64,
    /// Elevation of the coordinate.
    pub elevation: i32,
    /// Optional error message.
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct CoordinateResultList {
    pub results: Vec<CoordinateResult>,
}
#[derive(Deserialize)]
pub struct PostCoordinates {
    pub(crate) locations: Vec<CoordinateRequests>,
}
#[derive(Deserialize)]
pub struct CoordinateRequests {
    pub(crate) latitude: f64,
    pub(crate) longitude: f64,
}
/// Application state structure shared across handlers.
#[derive(Clone)]
pub struct AppState {
    /// Connection pool to the PostgreSQL database.
    pub geo: Arc<dyn GeoRepository>,
    /// Cache for storing previously looked-up coordinates.
    pub cache: Cache<String, CoordinateResult>,
}
pub type ConnectionPool = Pool<PostgresConnectionManager<NoTls>>;