use rocket_okapi::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Debug, Deserialize, JsonSchema)]
pub struct KubectlRequest {
    pub config_map_name: String,
}
