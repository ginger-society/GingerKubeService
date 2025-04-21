use std::env;
use std::process::{Command, Stdio};
use std::io::Write;

use ginger_shared_rs::rocket_models::MessageResponse;
use rocket::{serde::json::Json, tokio::task};
use rocket_okapi::openapi;

use crate::models::request::{KubectlRequest, LogRequest};

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
            .env("KUBECONFIG", env::var("KUBECONFIG_PATH").expect("KUBECONFIG_PATH must be set"))
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
    
            // Parse TaskRun name (extract from stdout: "taskrun.tekton.dev/dry-run-8-abcde created")
            let taskrun_name = stdout
                .lines()
                .find_map(|line| {
                    line.split_whitespace()
                        .find(|part| part.contains("dry-run-8-"))
                        .map(|part| part.trim_start_matches("taskrun.tekton.dev/").to_string())
                });
    
            if let Some(taskrun_name) = taskrun_name {
                // Wait a moment to allow pods to be created
                rocket::tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let taskrun_name_cloned = taskrun_name.clone();

                let pod_output_result = task::spawn_blocking(move || {
                    Command::new("kubectl")
                        .arg("get")
                        .arg("pods")
                        .arg("-n")
                        .arg("tasks-ginger-db-compose-runtime")
                        .arg("-l")
                        .arg(format!("tekton.dev/taskRun={}", taskrun_name_cloned))
                        .arg("-o")
                        .arg("name")
                        .env("KUBECONFIG", env::var("KUBECONFIG_PATH").expect("KUBECONFIG_PATH must be set"))
                        .output()
                })
                .await;
    
                match pod_output_result {
                    Ok(Ok(pod_output)) if pod_output.status.success() => {
                        let pods = String::from_utf8_lossy(&pod_output.stdout).to_string();
                        Json(MessageResponse {
                            message: format!("Created TaskRun: {}\nPods:\n{}", taskrun_name, pods),
                        })
                    }
                    Ok(Ok(pod_output)) => {
                        let stderr = String::from_utf8_lossy(&pod_output.stderr).to_string();
                        Json(MessageResponse {
                            message: format!("TaskRun created but failed to fetch pods: {}", stderr),
                        })
                    }
                    _ => Json(MessageResponse {
                        message: format!("TaskRun created but pod lookup failed"),
                    }),
                }
            } else {
                Json(MessageResponse {
                    message: format!("TaskRun created but name could not be parsed:\n{}", stdout),
                })
            }
        }
    
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Json(MessageResponse {
                message: format!("kubectl error: {}", stderr),
            })
        }
    
        Ok(Err(e)) => Json(MessageResponse {
            message: format!("Failed to run kubectl: {}", e),
        }),
    
        Err(e) => Json(MessageResponse {
            message: format!("Blocking task failed: {}", e),
        }),
    }
    
}


#[openapi(tag = "Kubernetes")]
#[post("/kubectl/logs", format = "json", data = "<params>")]
pub async fn kubectl_logs(params: Json<LogRequest>) -> Json<MessageResponse> {
    let taskrun_name = params.taskrun_name.clone();
    let step_name = params.step_name.clone();
    let namespace = "tasks-ginger-db-compose-runtime";

    let pod_name_result = task::spawn_blocking(move || {
        let output = Command::new("kubectl")
            .arg("get")
            .arg("pods")
            .arg("-n")
            .arg(namespace)
            .arg("-l")
            .arg(format!("tekton.dev/taskRun={}", taskrun_name))
            .arg("-o")
            .arg("jsonpath={.items[0].metadata.name}")
            .env("KUBECONFIG", env::var("KUBECONFIG_PATH").expect("KUBECONFIG_PATH must be set"))
            .output()?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&output.stdout).trim().to_string())
    })
    .await;

    let pod_name = match pod_name_result {
        Ok(Ok(name)) if !name.is_empty() => name,
        Ok(Ok(_)) => return Json(MessageResponse { message: "No pod found for TaskRun".to_string() }),
        Ok(Err(e)) => return Json(MessageResponse { message: format!("Error getting pod name: {}", e) }),
        Err(e) => return Json(MessageResponse { message: format!("Spawn error: {}", e) }),
    };

    let step_name_clone = step_name.clone();
    let log_output_result = task::spawn_blocking(move || {
        let output = Command::new("kubectl")
            .arg("logs")
            .arg("-n")
            .arg(namespace)
            .arg(&pod_name)
            .arg("-c")
            .arg(step_name_clone)
            .env("KUBECONFIG", env::var("KUBECONFIG_PATH").expect("KUBECONFIG_PATH must be set"))
            .output()?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&output.stdout).to_string())
    })
    .await;

    match log_output_result {
        Ok(Ok(logs)) => Json(MessageResponse { message: logs }),
        Ok(Err(e)) => Json(MessageResponse { message: format!("Error getting logs: {}", e) }),
        Err(e) => Json(MessageResponse { message: format!("Spawn error: {}", e) }),
    }
}
