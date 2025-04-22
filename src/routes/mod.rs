use std::env;
use std::process::{Command, Stdio};
use std::io::Write;

use ginger_shared_rs::rocket_models::MessageResponse;
use rocket::tokio;
use rocket::{serde::json::Json, tokio::task};
use rocket_okapi::openapi;
use serde_json::{json, Value};

use crate::models::request::{KubectlRequest, LogRequest};
use crate::models::response::{KubectlLogsResponse, TaskRunResponse};
use anyhow::{Result, Context};


#[openapi()]
#[get("/")]
pub fn index() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "Ok".to_string(),
    })
}
#[openapi()]
#[post("/kubectl", format = "json", data = "<params>")]
pub async fn kubectl_command(params: Json<KubectlRequest>) -> Json<TaskRunResponse> {
    let models_py_content = params.models_py_content.clone();

    let config_map_result = task::spawn_blocking(move || {
        let config_map = format!(
            r#"apiVersion: v1
kind: ConfigMap
metadata:
  generateName: models-py-
  namespace: tasks-ginger-db-compose-runtime
data:
  models.json: |
{}"#,
            models_py_content
                .lines()
                .map(|line| format!("    {}", line))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let mut cmd = Command::new("kubectl")
            .arg("create")
            .arg("-f")
            .arg("-")
            .arg("-o=jsonpath={.metadata.name}")
            .env("KUBECONFIG", env::var("KUBECONFIG_PATH").expect("KUBECONFIG_PATH must be set"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(stdin) = cmd.stdin.as_mut() {
            stdin.write_all(config_map.as_bytes())?;
        }

        let output = cmd.wait_with_output()?;
        Ok::<_, std::io::Error>(output)
    })
    .await;

    match config_map_result {
        Ok(Ok(output)) if output.status.success() => {
            let config_map_name = String::from_utf8_lossy(&output.stdout).trim().to_string();

            if config_map_name.is_empty() {
                return Json(TaskRunResponse {
                    taskrun_name: None,
                    message: Some("Failed to get ConfigMap name".into()),
                });
            }

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
                    let taskrun_name = stdout
                        .lines()
                        .find_map(|line| {
                            line.split_whitespace()
                                .find(|part| part.contains("dry-run-8-"))
                                .map(|part| part.trim_start_matches("taskrun.tekton.dev/").to_string())
                        });

                    if let Some(name) = taskrun_name {
                        Json(TaskRunResponse {
                            taskrun_name: Some(name),
                            message: None,
                        })
                    } else {
                        Json(TaskRunResponse {
                            taskrun_name: None,
                            message: Some(format!("TaskRun created but name not found: {}", stdout)),
                        })
                    }
                }

                Ok(Ok(output)) => Json(TaskRunResponse {
                    taskrun_name: None,
                    message: Some(format!(
                        "kubectl error creating TaskRun: {}",
                        String::from_utf8_lossy(&output.stderr)
                    )),
                }),

                Ok(Err(e)) => Json(TaskRunResponse {
                    taskrun_name: None,
                    message: Some(format!("Failed to run kubectl for TaskRun: {}", e)),
                }),

                Err(e) => Json(TaskRunResponse {
                    taskrun_name: None,
                    message: Some(format!("Blocking task failed for TaskRun: {}", e)),
                }),
            }
        }

        Ok(Ok(output)) => Json(TaskRunResponse {
            taskrun_name: None,
            message: Some(format!(
                "Failed to create ConfigMap: {}",
                String::from_utf8_lossy(&output.stderr)
            )),
        }),

        Ok(Err(e)) => Json(TaskRunResponse {
            taskrun_name: None,
            message: Some(format!("Failed to run kubectl for ConfigMap: {}", e)),
        }),

        Err(e) => Json(TaskRunResponse {
            taskrun_name: None,
            message: Some(format!("Blocking task failed for ConfigMap: {}", e)),
        }),
    }
}

#[openapi()]
#[post("/kubectl/logs", format = "json", data = "<params>")]
pub async fn kubectl_logs(params: Json<LogRequest>) -> Json<KubectlLogsResponse> {
    let taskrun_name = params.taskrun_name.clone();
    let step_name = params.step_name.clone();
    let namespace = "tasks-ginger-db-compose-runtime";
    let pod_name = format!("{}-pod", taskrun_name);

    let logs_task = task::spawn_blocking({
        let pod_name = pod_name.clone();
        let step_name = step_name.clone();
        move || {
            let output = Command::new("kubectl")
                .arg("logs")
                .arg("-n")
                .arg(namespace)
                .arg(&pod_name)
                .arg("-c")
                .arg(step_name)
                .env("KUBECONFIG", env::var("KUBECONFIG_PATH").expect("KUBECONFIG_PATH must be set"))
                .output()?;
            Ok::<_, std::io::Error>(String::from_utf8_lossy(&output.stdout).to_string())
        }
    });

    let status_task = task::spawn_blocking({
        let taskrun_name = taskrun_name.clone();
        move || -> Result<String> {
            let output = Command::new("kubectl")
                .arg("get")
                .arg("taskrun")
                .arg(&taskrun_name)
                .arg("-n")
                .arg(namespace)
                .arg("-o")
                .arg("json")
                .env("KUBECONFIG", env::var("KUBECONFIG_PATH")?)
                .output()
                .context("Failed to run kubectl")?;

            let json: Value = serde_json::from_slice(&output.stdout)
                .context("Failed to parse JSON")?;

            let status = json["status"]["steps"]
                .as_array()
                .and_then(|steps| {
                    steps.iter().find(|step| step["name"] == step_name.replace("step-", ""))
                })
                .and_then(|step| step["terminated"]["reason"].as_str().map(String::from))
                .unwrap_or("Unknown".into());

            Ok(status)
        }
    });

    let (logs, status) = tokio::join!(logs_task, status_task);

    let logs_result = match logs {
        Ok(Ok(logs)) => logs,
        Ok(Err(e)) => format!("Error getting logs: {}", e),
        Err(e) => format!("Spawn error (logs): {}", e),
    };

    let status_result = match status {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => format!("Error getting step status: {}", e),
        Err(e) => format!("Spawn error (status): {}", e),
    };

    Json(KubectlLogsResponse {
        logs: logs_result,
        status: status_result,
    })
}
