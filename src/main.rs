use gdal::{Dataset};
use gdal::raster::RasterBand;
use gdal::spatial_ref::{SpatialRef, CoordTransform};
use std::path::Path;
use geozero::wkb;
use geo::{Geometry, Polygon};
use walkdir::WalkDir;
use clap::Parser;
use exitcode;
use std::env;
use axum::{
    routing::{post},
    http::StatusCode,
    Json, Router,
    extract::{State},
};
use axum::routing::get;
use axum_macros::debug_handler;
use bb8::{Pool};
use bb8_postgres::PostgresConnectionManager;
use tokio_postgres::{NoTls};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use moka::future::Cache;
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    regenerate: bool,
}
/// Structure representing a coordinate result.
#[derive(Clone, Serialize)]
struct CoordinateResult {
    /// Longitude of the coordinate.
    longitude: f64,
    /// Latitude of the coordinate.
    latitude: f64,
    /// Elevation of the coordinate.
    elevation: i32,
    /// Optional error message.
    error: Option<String>,
}

#[derive(Serialize)]
struct CoordinateResultList {
    results: Vec<CoordinateResult>,
}
#[derive(Deserialize)]
struct PostCoordinates {
    locations: Vec<CoordinateRequests>,
}
#[derive(Deserialize)]
struct CoordinateRequests {
    latitude: f64,
    longitude: f64,
}
/// Application state structure shared across handlers.
#[derive(Clone)]
struct AppState {
    /// Connection pool to the PostgreSQL database.
    db_connection: ConnectionPool,
    /// Directory containing geospatial data.
    datadir: String,
    /// Cache for storing previously looked-up coordinates.
    cache: Cache<String, CoordinateResult>,
}

type ConnectionPool = Pool<PostgresConnectionManager<NoTls>>;


/// Parses geospatial data from the specified directory and creates a database.
///
/// # Arguments
/// * `datadir_path_string` - The path to the directory containing the geospatial data.
/// * `pool` - The connection pool to the PostgreSQL database.
///
/// # Returns
/// * `Ok(true)` if the process is successful.
/// * `Err(&str)` if the database could not be created or data could not be processed.
async fn parse_data_create_database(datadir_path_string: String, pool: ConnectionPool) -> Result<bool, &'static str>{
    let datadir = Path::new(datadir_path_string.as_str());
    let conn = match pool.get().await {
        Ok(conn) => conn,
        Err(_) => {return Err("Database Connection could not be established")}
    };

    let _ = match conn.batch_execute("DROP TABLE geo_data").await {
        Ok(_) => {},
        Err(_) => {println!("Old Database could not be dropped. Continuing...")}
    };
    let _ = match conn.batch_execute("
        CREATE TABLE IF NOT EXISTS geo_data (
            id              SERIAL PRIMARY KEY,
            path            VARCHAR,
            resolution      INTEGER,
            object          GEOMETRY
            )
    ").await {
        Ok(_) => {},
        Err(_) => {return Err("New Database could not be created")}
    };
    println!("Walking Directory....");
    for entry in WalkDir::new(datadir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.file_type().is_dir()) {
        let filepath = match entry.path().strip_prefix(datadir){
            Ok(filepath) => filepath,
            Err(e) => {println!("Filepath could not be formated: {}", e); continue}
        };
        println!("{}", filepath.to_str().unwrap());
        let dataset = match Dataset::open(datadir.join(filepath)){
            Ok(dataset) => dataset,
            Err(e) => {println!("Dataset could not be opened: {}", e); continue}
        };
        println!("{:?}", dataset.projection());
        let projection_string = dataset.projection();
        let collection = projection_string.split("EPSG\",").collect::<Vec<&str>>();
        let epsg_string: &&str = match collection.last(){
            Some(epsg_string) => epsg_string,
            None => {println!("EPSG String could not be parsed"); continue}
        };
        let epsg_number = match (&epsg_string[1..epsg_string.len() - 3]).parse::<i32>() {
            Ok(epsg_number) => epsg_number,
            Err(e) => {println!("EPSG Number could not be parsed: {}", e); continue}
        };
        println!("{:?}", epsg_number);
        let spat = match SpatialRef::from_esri(&*dataset.projection()) {
            Ok(spatial_ref) => spatial_ref,
            Err(e) => {println!("Source SpatialRef could not be parsed: {}", e); continue}
        };
        let spat_target = match SpatialRef::from_epsg(4326) {
            Ok(spat_target_ref) => spat_target_ref,
            Err(e) => {println!("Target SpatialRef could not be parsed: {}", e); continue}
        };
        let geo = match CoordTransform::new(&spat, &spat_target) {
            Ok(geo) => geo,
            Err(e) => {println!("CoordTransform could not be created: {}", e); continue}
        };
        let (width, height) = dataset.raster_size();
        let geotransform = match dataset.geo_transform() {
            Ok(geotransform) => geotransform,
            Err(e) => {println!("Geo transform could not be created: {}", e); continue}
        };
        let mut x_coord = [geotransform[0], geotransform[0] + width as f64 * geotransform[1] + height as f64 * geotransform[2]];
        let mut y_coord = [geotransform[3] + width as f64 * geotransform[4] + height as f64 * geotransform[5],  geotransform[3]];
        match geo.transform_coords(&mut x_coord, &mut y_coord, &mut [0.0, 0.0]) {
            Ok(_) => {},
            Err(e) => {println!("Transform coords could not be converted: {}", e); continue}
        };
        println!("{:?}, {:?}, {:?}, {:?}",geotransform[0], geotransform[3] + width as f64 * geotransform[4] + height as f64 * geotransform[5], geotransform[0] + width as f64 * geotransform[1] + height as f64 * geotransform[2], geotransform[3]);
        if epsg_number == 25832 {
            //We need to make a special exception for EPSG 25832 because Lat/Lon is switched in the conversion
            (x_coord[0], x_coord[1], y_coord[0], y_coord[1]) = (y_coord[0], y_coord[1], x_coord[0], x_coord[1]);
        }
        println!("{:?}, {:?} | {:?}, {:?} | {:?}, {:?} | {:?}, {:?}", x_coord[0], y_coord[0], x_coord[0], y_coord[1], x_coord[1], y_coord[1],  x_coord[1], y_coord[0]);
        let resolution = (width as f64 / ((500f64 + x_coord[0]) - (500f64 + x_coord[1])).abs()) as i32;
        let coord_1 = geo::Coord::from((x_coord[0], y_coord[0]));
        let coord_2 = geo::Coord::from((x_coord[0], y_coord[1]));
        let coord_3 = geo::Coord::from((x_coord[1], y_coord[1]));
        let coord_4 = geo::Coord::from((x_coord[1], y_coord[0]));
        let geom: Geometry<f64> = Polygon::new(geo::LineString(vec![coord_1, coord_2, coord_3, coord_4]), vec![]).into();
        let _ = match conn.execute("INSERT INTO geo_data (path,resolution,object) VALUES($1, $2, ST_SetSRID(CAST ($3 AS geometry),4326))",
                               &[&filepath.to_str().unwrap(), &resolution, &wkb::Encode(geom)]).await {
            Ok(_) => {},
            Err(e) => {println!("Failed to insert geo_data: {}", e); continue}
        };
    }
    Ok(true)
}


/// Looks up elevation data based on latitude and longitude.
///
/// # Arguments
/// * `lat` - Latitude of the point.
/// * `lon` - Longitude of the point.
/// * `pool` - PostgreSQL connection pool.
/// * `config_datadir` - Directory containing geospatial data.
///
/// # Returns
/// A `CoordinateResult` containing the elevation or an error message.
async fn lookup_coordinats(lat: f64, lon: f64, pool: &ConnectionPool, config_datadir: &str) -> CoordinateResult {
    let conn = match pool.get().await {
        Ok(conn) => conn,
        Err(_e) => {
            eprintln!("{:?}", _e);
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from("Internal Server Error".to_string())};
        }
    };
    let datadir = Path::new(config_datadir);
    let row = match conn.query(
        &format!("SELECT * FROM geo_data WHERE ST_Contains(object, ST_GeomFromText('POINT({} {})', 4326)) ORDER BY resolution DESC;", lon, lat),
        &[],
    ).await {
        Ok(row) => row,
        Err(_e) => {
            eprintln!("{:?}", _e);
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("No such coordinate {} {}.", lat, lon))};
        }
    };
    if row.len() == 0 {
        return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("No such coordinate {} {}.", lat, lon))};
    }
    let value: String = row[0].get("path");
    let dataset = match Dataset::open(datadir.join(Path::new(&value))) {
        Ok(dataset) => dataset,
        Err(_e) => {
            eprintln!("{:?}", _e);
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("Internal Server Error {} {}.", lat, lon))};
        }
    };
    let spat_point = match SpatialRef::from_epsg(4326) {
        Ok(spatial_ref) => spatial_ref,
        Err(_e) => {
            eprintln!("{:?}", _e);
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("Internal Server Error {} {}.", lat, lon))};
        }
    };
    let spat_data = match SpatialRef::from_esri(&*dataset.projection()) {
        Ok(spatial_ref) => spatial_ref,
        Err(_e) => {
            eprintln!("{:?}", _e);
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("Internal Server Error {} {}.", lat, lon))};
        }
    };
    let geo = match CoordTransform::new(&spat_point, &spat_data) {
        Ok(geo) => geo,
        Err(_e) => {
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("Internal Server Error {} {}.", lat, lon))};
        }
    };
    let geotransform = match dataset.geo_transform() {
        Ok(geotransform) => geotransform,
        Err(_e) => {
            eprintln!("{:?}", _e);
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("Internal Server Error {} {}.", lat, lon))};
        }
    };
    let (width, height) = dataset.raster_size();
    let projection_string = dataset.projection();
    let collection = projection_string.split("EPSG\",").collect::<Vec<&str>>();
    let epsg_string = match collection.last() {
        Some(epsg_string) => epsg_string,
        None => {
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("Internal Server Error {} {}.", lat, lon))};
        }
    };
    let epsg_number = match (&epsg_string[1..epsg_string.len() - 3]).parse::<i32>() {
        Ok(epsg_number) => epsg_number,
        Err(_e) => {
            eprintln!("{:?}", _e);
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("Internal Server Error {} {}.", lat, lon))};
        }
    };
    let mut x_coord = [lon];
    let mut y_coord = [lat];
    if epsg_number == 25832 {
        //Also in the back conversion we must make the same exception
        (x_coord, y_coord) = (y_coord, x_coord);
    }
    match geo.transform_coords(&mut x_coord, &mut y_coord, &mut [0.0]) {
        Ok(_) => {},
        Err(_e) => {
            eprintln!("{:?}", _e);
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("Internal Server Error {} {}.", lat, lon))};
        }
    };
    let resolution_x: f64 = width as f64 / ((500f64 + geotransform[0]) - (500f64 + geotransform[0] + width as f64 * geotransform[1] + height as f64 * geotransform[2])).abs();
    let resolution_y: f64 = height as f64 / ((500f64 + geotransform[3] + width as f64 * geotransform[4] + height as f64 * geotransform[5]) - (500f64 + geotransform[3])).abs();
    let pixel_x = ((x_coord[0]-geotransform[0]).round() * resolution_x).round();
    let pixel_y = ((y_coord[0]-(geotransform[3] + width as f64 * geotransform[4] + height as f64 * geotransform[5])).round() * resolution_y).round();
    let rasterband: RasterBand = match dataset.rasterband(1) {
        Ok(rasterband) => rasterband,
        Err(_e) => {
            eprintln!("{:?}", _e);
            return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("Internal Server Error {} {}.", lat, lon))};
        }
    };
    if let Ok(rv) = rasterband.read_as::<u8>((pixel_x as isize, pixel_y as isize), (1, 1), (1, 1), None) {
        return CoordinateResult {latitude: lat, longitude: lon, elevation: rv.data()[0] as i32, error: None };
    }
    else {
        return CoordinateResult {latitude: lat, longitude: lon, elevation: 0i32, error: Option::from(format!("Internal Server Error {} {}.", lat, lon))};
    }
}

/// Handles POST requests to lookup coordinates.
///
/// # Arguments
/// * `appstate` - Application state containing the database connection and cache.
/// * `payload` - JSON payload containing the coordinates to look up.
///
/// # Returns
/// A tuple containing the status code and the JSON result.
#[debug_handler]
async fn post_lookup_coordinates(
    State(appstate): State<AppState>, Json(payload): Json<PostCoordinates>) -> (StatusCode, Json<CoordinateResultList>) {
    let pool = appstate.db_connection;
    let config_datadir = appstate.datadir;
    let cache = appstate.cache;
    let mut result_list: Vec<CoordinateResult> = Vec::new();
    let locations = payload.locations;
    for location in locations {
        let lat = location.latitude;
        let lon = location.longitude;
        let coordinate_result = match cache.get(&format!("{},{}", lat, lon)).await {
            Some(coordinate_result) => coordinate_result,
            None => {
                let lookup_result = lookup_coordinats(lat, lon, &pool, &config_datadir).await;
                cache.insert(format!("{},{}", lat, lon), lookup_result.clone()).await;
                lookup_result
            }
        };
        result_list.push(coordinate_result);
    }
    (StatusCode::OK, Json(CoordinateResultList {results: result_list}))
}

/// Handles GET requests to lookup coordinates.
///
/// # Arguments
/// * `appstate` - Application state containing the database connection and cache.
/// * `params` - Query parameters containing the locations to look up.
///
/// # Returns
/// A tuple containing the status code and the JSON result.
#[debug_handler]
async fn get_lookup_coordinates(
    State(appstate): State<AppState>, axum::extract::Query(params):
    axum::extract::Query<HashMap<String, String>>) -> (StatusCode, Json<CoordinateResultList>) {
    let pool = appstate.db_connection;
    let config_datadir = appstate.datadir;
    let cache = appstate.cache;

    let mut result_list: Vec<CoordinateResult> = Vec::new();
    let location_string = match params.get("locations"){
        Some(locations) => locations,
        None => {
            result_list.push(CoordinateResult {latitude: 0f64, longitude: 0f64, elevation: 0i32, error: Option::from("locations is a required parameter".to_string())});
            return (StatusCode::OK, Json(CoordinateResultList {results: result_list}));
        }
    };
    let locations = location_string.split("|").collect::<Vec<&str>>();
    for location in locations {
        let latlon = location.split(",").collect::<Vec<&str>>();
        let lat_string = match latlon.first() {
            Some(lat_string) => lat_string,
            None => {
                result_list.push(CoordinateResult {latitude: 0f64, longitude: 0f64, elevation: 0i32, error: Option::from(format!("Bad parameter format {}.", location))});
                continue;
            }
        };
        let lon_string = match latlon.last() {
            Some(lon_string) => lon_string,
            None => {
                result_list.push(CoordinateResult {latitude: 0f64, longitude: 0f64, elevation: 0i32, error: Option::from(format!("Bad parameter format {}.", location))});
                continue;
            }
        };
        let lon = match lon_string.parse::<f64>() {
            Ok(lon) => lon,
            Err(_) => {
                result_list.push(CoordinateResult {latitude: 0f64, longitude: 0f64, elevation: 0i32, error: Option::from(format!("Bad parameter format {}.", location))});
                continue;
            }
        };
        let lat = match lat_string.parse::<f64>() {
            Ok(lat) => lat,
            Err(_) => {
                result_list.push(CoordinateResult {latitude: 0f64, longitude: 0f64, elevation: 0i32, error: Option::from(format!("Bad parameter format {}.", location))});
                continue;
            }
        };
        let coordinate_result = match cache.get(&format!("{},{}", lat, lon)).await {
            Some(coordinate_result) => coordinate_result,
            None => {
                let lookup_result = lookup_coordinats(lat, lon, &pool, &config_datadir).await;
                cache.insert(format!("{},{}", lat, lon), lookup_result.clone()).await;
                lookup_result
            }
        };
        result_list.push(coordinate_result);
    }
    (StatusCode::OK, Json(CoordinateResultList {results: result_list}))
}

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
        .route(&*api_url, post(post_lookup_coordinates)).route(&*api_url, get(get_lookup_coordinates)).with_state(AppState{db_connection: pool, datadir: config_datadir, cache: cache});
    let listener = match tokio::net::TcpListener::bind("0.0.0.0:3000").await {
        Ok(listener) => listener,
        Err(e) => {println!("Setting up TCP Listener unsucessfull: {}", e); std::process::exit(exitcode::SOFTWARE)}
    };
    let _ = match axum::serve(listener, app).await {
        Ok(_) => {},
        Err(e) => {println!("Starting Server unsucessfull: {}", e); std::process::exit(exitcode::SOFTWARE)}
    };


}
