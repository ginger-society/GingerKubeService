use rocket_okapi::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Debug, Deserialize, JsonSchema)]
pub struct KubectlRequest {
    pub command: String,
    pub namespace: Option<String>,
}
