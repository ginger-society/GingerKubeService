use chrono::{DateTime, NaiveDate, Utc};
use rocket_okapi::JsonSchema;
use serde::{Deserialize, Serialize};


#[derive(Serialize, JsonSchema)]
pub struct TaskRunResponse {
    pub taskrun_name: Option<String>,
    pub message: Option<String>,
}


#[derive(Serialize, JsonSchema)]
pub struct KubectlLogsResponse {
    pub logs: String,
    pub status: String,
}