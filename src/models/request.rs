use rocket_okapi::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Debug, Deserialize, JsonSchema)]
pub struct KubectlRequest {
    pub config_map_name: String,
}


#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogRequest {
    pub taskrun_name: String,
    pub step_name: String,
}