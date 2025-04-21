use std::{env, process::Command};

use ginger_shared_rs::rocket_models::MessageResponse;
use rocket::{serde::json::Json, tokio::task};
use rocket_okapi::openapi;

use crate::models::request::KubectlRequest;

/// This is a description. <br />You can do simple html <br /> like <b>this<b/>
#[openapi()]
#[get("/")]
pub fn index() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "Ok".to_string(),
    })
}


/// Run a basic kubectl get command
#[openapi(tag = "Kubernetes")]
#[post("/kubectl", format = "json", data = "<params>")]
pub async fn kubectl_command(params: Json<KubectlRequest>) -> Json<MessageResponse> {
    let command = params.command.clone();
    let namespace = params.namespace.clone().unwrap_or_else(|| "default".to_string());

    let output_result = task::spawn_blocking(move || {
        let mut cmd = Command::new("kubectl");
        cmd.arg("get")
            .arg(&command)
            .arg("-n")
            .arg(&namespace);

        cmd.env("KUBECONFIG", env::var("DATABASE_URL").expect("DATABASE_URL must be set"));

        cmd.output()
    })
    .await;

    match output_result {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            Json(MessageResponse { message: stdout })
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Json(MessageResponse { message: format!("kubectl error: {}", stderr) })
        }
        Ok(Err(e)) => Json(MessageResponse { message: format!("Failed to run kubectl: {}", e) }),
        Err(e) => Json(MessageResponse { message: format!("Blocking task failed: {}", e) }),
    }
}
