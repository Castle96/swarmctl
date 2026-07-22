use bollard::Docker;
use bollard::models::{Mount, MountTypeEnum};
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MigrationTask {
    pub volume_name: String,
    pub source_node: String,
    pub target_node: String,
    pub status: MigrationStatus,
    pub bytes_copied: u64,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub error: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum MigrationStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

pub struct VolumeMigration {
    docker: Docker,
}

impl VolumeMigration {
    pub fn new(docker: Docker) -> Self {
        Self { docker }
    }

    #[allow(dead_code)]
    pub async fn get_service_volumes(
        &self,
        service_name: &str,
    ) -> anyhow::Result<Vec<Mount>> {
        let services = self.docker.list_services(None).await?;
        for service in services {
            if let Some(spec) = &service.spec {
                if spec.name.as_deref() == Some(service_name) {
                    if let Some(task_template) = &spec.task_template {
                        if let Some(container_spec) = &task_template.container_spec {
                            if let Some(mounts) = &container_spec.mounts {
                                let volume_mounts: Vec<Mount> = mounts
                                    .iter()
                                    .filter(|m| {
                                        matches!(
                                            m.typ.as_ref(),
                                            Some(MountTypeEnum::VOLUME)
                                        )
                                    })
                                    .cloned()
                                    .collect();
                                return Ok(volume_mounts);
                            }
                        }
                    }
                }
            }
        }
        Ok(Vec::new())
    }

    pub async fn list_node_volumes(
        &self,
        _node_id: &str,
    ) -> anyhow::Result<Vec<String>> {
        let options = bollard::query_parameters::ListVolumesOptions {
            ..Default::default()
        };
        let result = self.docker.list_volumes(Some(options)).await?;
        Ok(result
            .volumes
            .unwrap_or_default()
            .into_iter()
            .map(|v| v.name)
            .collect())
    }

    pub async fn copy_volume_data(
        &self,
        volume_name: &str,
        source_path: &PathBuf,
        target_path: &PathBuf,
    ) -> anyhow::Result<u64> {
        let mut total_bytes: u64 = 0;

        self.create_target_directory(target_path).await?;

        let entries = self.read_directory_entries(source_path).await?;

        for entry in entries {
            let src = source_path.join(&entry);
            let dst = target_path.join(&entry);

            if src.is_dir() {
                let bytes = Box::pin(self.copy_volume_data(
                    &format!("{}/{}", volume_name, entry),
                    &src,
                    &dst,
                ))
                .await?;
                total_bytes += bytes;
            } else {
                let bytes = self.copy_file(&src, &dst).await?;
                total_bytes += bytes;
            }
        }

        Ok(total_bytes)
    }

    async fn create_target_directory(&self, path: &PathBuf) -> anyhow::Result<()> {
        if !path.exists() {
            fs::create_dir_all(path).await?;
        }
        Ok(())
    }

    async fn read_directory_entries(&self, path: &PathBuf) -> anyhow::Result<Vec<String>> {
        let mut entries = Vec::new();
        if path.exists() {
            let mut dir = fs::read_dir(path).await?;
            while let Some(entry) = dir.next_entry().await? {
                entries.push(entry.file_name().to_string_lossy().to_string());
            }
        }
        entries.sort();
        Ok(entries)
    }

    async fn copy_file(&self, src: &PathBuf, dst: &PathBuf) -> anyhow::Result<u64> {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).await?;
        }

        let data = fs::read(src).await?;
        let bytes = data.len() as u64;

        let mut file = fs::File::create(dst).await?;
        file.write_all(&data).await?;

        Ok(bytes)
    }

    pub async fn create_snapshot(
        &self,
        volume_name: &str,
        snapshot_path: &PathBuf,
    ) -> anyhow::Result<u64> {
        let volume = self.docker.inspect_volume(volume_name).await?;
        let source = PathBuf::from(&volume.mountpoint);
        if source.exists() {
            self.copy_volume_data(volume_name, &source, snapshot_path)
                .await
        } else {
            Ok(0)
        }
    }

    pub async fn restore_snapshot(
        &self,
        volume_name: &str,
        snapshot_path: &PathBuf,
    ) -> anyhow::Result<u64> {
        let volume = self.docker.inspect_volume(volume_name).await?;
        let target = PathBuf::from(&volume.mountpoint);
        self.copy_volume_data(volume_name, snapshot_path, &target)
            .await
    }
}

pub async fn migrate_volume(
    docker: &Docker,
    remote_docker: &Docker,
    volume_name: &str,
    source_node: &str,
    target_node: &str,
) -> anyhow::Result<MigrationTask> {
    let started_at = chrono_now();

    let migration = VolumeMigration::new(docker.clone());

    let snapshot_dir = std::env::temp_dir().join(format!("swarmctl_migration_{}", volume_name));

    let bytes_copied = migration.create_snapshot(volume_name, &snapshot_dir).await?;

    let target_volumes = migration.list_node_volumes(target_node).await?;

    if !target_volumes.contains(&volume_name.to_string()) {
        let request = bollard::models::VolumeCreateRequest {
            name: Some(volume_name.to_string()),
            ..Default::default()
        };
        remote_docker.create_volume(request).await?;
    }

    migration.restore_snapshot(volume_name, &snapshot_dir).await?;

    let _ = fs::remove_dir_all(&snapshot_dir).await;

    let completed_at = chrono_now();

    Ok(MigrationTask {
        volume_name: volume_name.to_string(),
        source_node: source_node.to_string(),
        target_node: target_node.to_string(),
        status: MigrationStatus::Completed,
        bytes_copied,
        started_at,
        completed_at: Some(completed_at),
        error: None,
    })
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let secs = duration.as_secs();
    let mins = secs / 60;
    let hours = mins / 60;
    format!("{:02}:{:02}:{:02}", hours % 24, mins % 60, secs % 60)
}
