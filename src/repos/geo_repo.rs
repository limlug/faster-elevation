use crate::types::{CoordinateResult, ConnectionPool};
use crate::utils::lookup::lookup_coordinats;
#[async_trait::async_trait]
pub trait GeoRepository: Send + Sync + 'static {
    /// Lookup a coordinate in your Postgres / spatial DB
    async fn lookup_coordinates(&self, lat: f64, lon: f64) -> CoordinateResult;
}
pub struct PgGeoRepo {
    pub pool: ConnectionPool,
    pub config_datadir: String,
}
#[async_trait::async_trait]
impl GeoRepository for PgGeoRepo {
    async fn lookup_coordinates(&self, lat: f64, lon: f64) -> CoordinateResult {
        lookup_coordinats(lat, lon, &self.pool, &self.config_datadir).await
    }
}