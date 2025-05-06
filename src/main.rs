use clap::Parser;
use exitcode;
use std::env;
use axum::{
    routing::{post},
    Router
};
use axum::routing::get;
use bb8::{Pool};
use bb8_postgres::PostgresConnectionManager;
use tokio_postgres::{NoTls};
use std::sync::Arc;
use moka::future::Cache;
use faster_elevation::types::{Cli, CoordinateResult, AppState};
use faster_elevation::repos::geo_repo::{PgGeoRepo};
use faster_elevation::handlers::{post_lookup_coordinates, get_lookup_coordinates};
use faster_elevation::utils::parse::{parse_data_create_database};


/// Main function to start the server and handle incoming requests.
#[tokio::main]
async fn main() {
    //Bekomme Lat Long übergeben
    //Frage PostGIS Server welches Polygon den Punkt enthält
    //PostGIS Datensatz: ID, Pfad unterhalb Dataroot, Auflösung, Projektion, Polygon(Boundary)
    //Wähle Layer mit höchster Auflösung
    //Öffne GeoTIFF und lese Höhe aus
    //Bei gesetzter regenerate Flag wird Datenbank gelöscht und neu geschrieben
    let args = Cli::parse();
    let dbuser = match env::var("DBUSER") {
        Ok(dbuser) => dbuser,
        Err(_) => {println!("$DBUSER is not set"); std::process::exit(exitcode::CONFIG)}
    };
    let dbpass = match env::var("DBPASS") {
        Ok(dbpass) => dbpass,
        Err(_) => {println!("$DBPASS is not set"); std::process::exit(exitcode::CONFIG)}
    };
    let dbhost = match env::var("DBHOST") {
        Ok(dbhost) => dbhost,
        Err(_) => {println!("$DBHOST is not set"); std::process::exit(exitcode::CONFIG)}
    };
    let dbdatabase = match env::var("DBDATABASE") {
        Ok(dbdatabase) => dbdatabase,
        Err(_) => {println!("$DBDATABSE is not set"); std::process::exit(exitcode::CONFIG)}
    };
    let config_datadir = match env::var("DATADIR") {
        Ok(config_datadir) => config_datadir,
        Err(_) => {println!("$DATADIR is not set"); std::process::exit(exitcode::CONFIG)}
    };
    let api_url = match env::var("APIURL") {
        Ok(api_url) => api_url,
        Err(_) => {println!("APIURL is not set"); std::process::exit(exitcode::CONFIG)}
    };
    let cache_size = match env::var("CACHESIZE") {
        Ok(cache_size) => cache_size,
        Err(_) => {println!("CACHESIZE is not set"); std::process::exit(exitcode::CONFIG)}
    };
    let db_config_string = format!("postgres://{}?dbname={}&user={}&password={}", dbhost, dbdatabase, dbuser, dbpass);
    let manager = match
        PostgresConnectionManager::new_from_stringlike(db_config_string, NoTls) {
        Ok(manager) => manager,
        Err(e) => {println!("DB Connection not sucessfull: {}", e); std::process::exit(exitcode::UNAVAILABLE)}
    };
    let pool = Pool::builder().build(manager).await.unwrap();
    let geo = Arc::new(PgGeoRepo { pool: pool.clone(), config_datadir: config_datadir.clone() });
    if args.regenerate == true {
        match parse_data_create_database(config_datadir, pool.clone()).await {
            Ok(_) => {std::process::exit(exitcode::OK);}
            Err(e) => {println!("Database Regeneration unsucessfull: {}", e); std::process::exit(exitcode::SOFTWARE);}
        };

    }
    let cache_size_u64 = match cache_size.parse::<u64>(){
        Ok(cache_size_u64) => cache_size_u64,
        Err(_) => {println!("Invalid value for CACHESIZE"); std::process::exit(exitcode::CONFIG)}
    };
    let cache:Cache<String, CoordinateResult> = Cache::new(cache_size_u64);
    let app = Router::new()
        // `POST /users` goes to `create_user`
        .route(&*api_url, post(post_lookup_coordinates)).route(&*api_url, get(get_lookup_coordinates)).with_state(AppState{geo: geo, cache: cache});
    let listener = match tokio::net::TcpListener::bind("0.0.0.0:3000").await {
        Ok(listener) => listener,
        Err(e) => {println!("Setting up TCP Listener unsucessfull: {}", e); std::process::exit(exitcode::SOFTWARE)}
    };
    let _ = match axum::serve(listener, app).await {
        Ok(_) => {},
        Err(e) => {println!("Starting Server unsucessfull: {}", e); std::process::exit(exitcode::SOFTWARE)}
    };
}

#[cfg(test)]
mod unit_tests {
    use faster_elevation::repos::geo_repo::{GeoRepository};
    use super::*;
    use axum::{extract::State, http::StatusCode, Json};
    struct DummyGeo;
    #[async_trait::async_trait]
    impl GeoRepository for DummyGeo {
        async fn lookup_coordinates(&self, lat: f64, lon: f64) -> CoordinateResult {
            CoordinateResult {
                latitude: lat,
                longitude: lon,
                error: None,
                elevation: 0,
            }
        }
    }
    fn dummy_state() -> AppState {
        AppState {
            geo: Arc::new(DummyGeo),
            cache: Cache::new(10u64),
        }
    }
    #[tokio::test]
    async fn split_missing_comma() {
        // Simulate the "location" param without comma:
        let mut params = std::collections::HashMap::new();
        params.insert("locations".into(), "51.5;0.1".into());

        let (status, Json(body)) = get_lookup_coordinates(
            State(dummy_state()),
            axum::extract::Query(params),
        ).await;

        assert_eq!(status, StatusCode::OK);
        // Should return one result with an error on "Bad parameter format"
        assert_eq!(body.results.len(), 1);
        assert!(body.results[0].error
            .as_ref().unwrap()
            .contains("Bad parameter format"));
    }
    #[tokio::test]
    async fn parse_non_numeric_latlon() {
        let mut params = std::collections::HashMap::new();
        params.insert("locations".into(), "abc,def".into());
        let (_status, Json(body)) = get_lookup_coordinates(
            State(dummy_state()),
            axum::extract::Query(params),
        ).await;

        assert_eq!(body.results.len(), 1);
        // latitude and longitude fields should echo the numeric parse failing
        assert!(body.results[0].error
            .as_ref().unwrap()
            .starts_with("Bad parameter format"));
    }
}