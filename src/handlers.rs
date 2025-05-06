use std::collections::HashMap;
use axum::extract::State;
use axum::Json;
use axum_macros::debug_handler;
use http::StatusCode;
use crate::types::{AppState, CoordinateResult, CoordinateResultList, PostCoordinates};
/// Handles POST requests to lookup coordinates.
///
/// # Arguments
/// * `appstate` - Application state containing the database connection and cache.
/// * `payload` - JSON payload containing the coordinates to look up.
///
/// # Returns
/// A tuple containing the status code and the JSON result.
#[debug_handler]
pub async fn post_lookup_coordinates(
    State(appstate): State<AppState>, Json(payload): Json<PostCoordinates>) -> (StatusCode, Json<CoordinateResultList>) {
    let cache = appstate.cache;
    let mut result_list: Vec<CoordinateResult> = Vec::new();
    let locations = payload.locations;
    for location in locations {
        let lat = location.latitude;
        let lon = location.longitude;
        let coordinate_result = match cache.get(&format!("{},{}", lat, lon)).await {
            Some(coordinate_result) => coordinate_result,
            None => {
                let lookup_result = appstate.geo.lookup_coordinates(lat, lon).await;
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
pub async fn get_lookup_coordinates(
    State(appstate): State<AppState>, axum::extract::Query(params):
    axum::extract::Query<HashMap<String, String>>) -> (StatusCode, Json<CoordinateResultList>) {
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
                let lookup_result = appstate.geo.lookup_coordinates(lat, lon).await;
                cache.insert(format!("{},{}", lat, lon), lookup_result.clone()).await;
                lookup_result
            }
        };
        result_list.push(coordinate_result);
    }
    (StatusCode::OK, Json(CoordinateResultList {results: result_list}))
}