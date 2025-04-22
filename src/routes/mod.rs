use std::env;
use std::process::{Command, Stdio};
use std::io::Write;

use ginger_shared_rs::rocket_models::MessageResponse;
use rocket::tokio;
use rocket::{serde::json::Json, tokio::task};
use rocket_okapi::openapi;
use serde_json::{json, Value};

use crate::models::request::{KubectlRequest, LogRequest};
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
pub async fn kubectl_command(params: Json<KubectlRequest>) -> Json<MessageResponse> {
    let models_py_content = params.models_py_content.clone();

    // First, create the ConfigMap with the models.py content and let Kubernetes generate the name
    let config_map_result = task::spawn_blocking(move || {
        // Create ConfigMap YAML with generateName instead of name
        let config_map = format!(
            r#"apiVersion: v1
kind: ConfigMap
metadata:
  generateName: models-py-
  namespace: tasks-ginger-db-compose-runtime
data:
  models.json: |
{}"#,
            // Indent each line of models_py_content with 4 spaces for YAML formatting
            models_py_content.lines().map(|line| format!("    {}", line)).collect::<Vec<_>>().join("\n")
        );

        // Create the ConfigMap
        let mut cmd = Command::new("kubectl")
            .arg("create")
            .arg("-f")
            .arg("-")
            .arg("-o=jsonpath={.metadata.name}")  // Output only the generated name
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

    // Extract the generated ConfigMap name and proceed with TaskRun creation
    match config_map_result {
        Ok(Ok(output)) if output.status.success() => {
            // Extract the generated ConfigMap name from stdout
            let config_map_name = String::from_utf8_lossy(&output.stdout).to_string();
            
            if config_map_name.is_empty() {
                return Json(MessageResponse {
                    message: "Failed to get ConfigMap name".to_string(),
                });
            }

            // Now create the TaskRun using the generated ConfigMap name
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
                                    message: format!("Created ConfigMap: {}\nCreated TaskRun: {}\nPods:\n{}", 
                                                    config_map_name, taskrun_name, pods),
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
                        message: format!("kubectl error creating TaskRun: {}", stderr),
                    })
                }
            
                Ok(Err(e)) => Json(MessageResponse {
                    message: format!("Failed to run kubectl for TaskRun: {}", e),
                }),
            
                Err(e) => Json(MessageResponse {
                    message: format!("Blocking task failed for TaskRun: {}", e),
                }),
            }
        },
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Json(MessageResponse {
                message: format!("Failed to create ConfigMap: {}", stderr),
            })
        },
        Ok(Err(e)) => Json(MessageResponse {
            message: format!("Failed to run kubectl for ConfigMap: {}", e),
        }),
        Err(e) => Json(MessageResponse {
            message: format!("Blocking task failed for ConfigMap: {}", e),
        }),
    }
}

#[openapi()]
#[post("/kubectl/logs", format = "json", data = "<params>")]
pub async fn kubectl_logs(params: Json<LogRequest>) -> Json<Value> {
    let taskrun_name = params.taskrun_name.clone();
    let step_name = params.step_name.clone();
    let namespace = "tasks-ginger-db-compose-runtime";
    let pod_name = format!("{}-pod", taskrun_name);

    // Spawn to get logs
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

    // Spawn to get step status
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
                    steps.iter().find(|step| step["name"] == step_name)
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

    Json(json!({
        "logs": logs_result,
        "status": status_result,
    }))
}
