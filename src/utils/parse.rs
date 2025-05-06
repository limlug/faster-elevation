use std::path::Path;
use gdal::Dataset;
use gdal::spatial_ref::{CoordTransform, SpatialRef};
use geo::{Geometry, Polygon};
use geozero::wkb;
use walkdir::WalkDir;
use crate::types::ConnectionPool;

/// Parses geospatial data from the specified directory and creates a database.
///
/// # Arguments
/// * `datadir_path_string` - The path to the directory containing the geospatial data.
/// * `pool` - The connection pool to the PostgreSQL database.
///
/// # Returns
/// * `Ok(true)` if the process is successful.
/// * `Err(&str)` if the database could not be created or data could not be processed.
pub async fn parse_data_create_database(datadir_path_string: String, pool: ConnectionPool) -> Result<bool, &'static str>{
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