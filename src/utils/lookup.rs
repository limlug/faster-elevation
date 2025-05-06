use std::path::Path;
use gdal::Dataset;
use gdal::raster::RasterBand;
use gdal::spatial_ref::{CoordTransform, SpatialRef};
use crate::types::{ConnectionPool, CoordinateResult};
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
pub async fn lookup_coordinats(lat: f64, lon: f64, pool: &ConnectionPool, config_datadir: &str) -> CoordinateResult {
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