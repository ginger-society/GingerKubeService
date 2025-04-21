use rocket_okapi::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Debug, Deserialize, JsonSchema)]
pub struct KubectlRequest {
    pub models_py_content: String,
}


#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogRequest {
    pub taskrun_name: String,
    pub step_name: String,
}