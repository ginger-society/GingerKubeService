use std::process::{Command, Stdio};
use std::io::Write;

use ginger_shared_rs::rocket_models::MessageResponse;
use rocket::{serde::json::Json, tokio::task};
use rocket_okapi::openapi;

use crate::models::request::KubectlRequest;

#[openapi()]
#[get("/")]
pub fn index() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "Ok".to_string(),
    })
}



#[openapi(tag = "Kubernetes")]
#[post("/kubectl", format = "json", data = "<params>")]
pub async fn kubectl_command(params: Json<KubectlRequest>) -> Json<MessageResponse> {
    let config_map_name = params.config_map_name.clone();

    let manifest = format!(
        r#"
apiVersion: tekton.dev/v1beta1
kind: TaskRun
metadata:
  generateName: dry-run-8-
  namespace: tasks-ginger-db-compose-runtime
spec:
  serviceAccountName: tekton-registry-access
  taskRef:
    name: dry-run
  workspaces:
    - name: source
      emptyDir: {{}}
    - name: ssh-credentials
      secret:
        secretName: ssh-private-key
    - name: ssh-config
      emptyDir: {{}}
    - name: general-purpose-cache
      persistentVolumeClaim:
        claimName: general-purpose-cache
    - name: pipeline-secrets
      secret:
        secretName: pipeline-secrets-secret
    - name: src
      configMap:
        name: {}
"#,
        config_map_name
    );

    let output_result = task::spawn_blocking(move || {
        let mut cmd = Command::new("kubectl")
            .arg("create")
            .arg("-f")
            .arg("-")
            .env("KUBECONFIG", "/Users/pradeepyadav/Documents/ginger-society/prod.kubeconfig.yml")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(stdin) = cmd.stdin.as_mut() {
            stdin.write_all(manifest.as_bytes())?;
        }

        let output = cmd.wait_with_output()?;
        Ok::<_, std::io::Error>(output)
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
