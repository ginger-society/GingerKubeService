use rocket_okapi::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Debug, Deserialize, JsonSchema)]
pub struct KubectlRequest {
    pub models_py_content: String,
    pub commit_message: Option<String>,
    pub commit: bool,
}


#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogRequest {
    pub taskrun_name: String,
    pub step_name: String,
}