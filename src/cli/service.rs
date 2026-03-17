use crate::api::{client::DockerClient, service};
use crate::models::service::ServiceRow;
use crate::utils::printer::print_table;

pub async fn list(client: &DockerClient) -> anyhow::Result<()> {
    let services = service::list_services(client.inner()).await?;

    let rows: Vec<ServiceRow> = services
        .into_iter()
        .map(|s| {
            let spec = s.spec.unwrap_or_default();

            let name = spec.name.unwrap_or_default();
            let image = spec
                .task_template
                .and_then(|t| t.container_spec)
                .and_then(|c| c.image)
                .unwrap_or_default();

            let (mode, replicas) = match spec.mode {
                Some(m) if m.replicated.is_some() => {
                    let r = m.replicated.unwrap().replicas.unwrap_or(0);
                    ("replicated".to_string(), format!("{}/{}", r, r))
                }
                Some(_) => ("global".to_string(), "N/A".to_string()),
                None => ("unknown".to_string(), "N/A".to_string()),
            };

            ServiceRow {
                id: s.id.unwrap_or_default(),
                name,
                mode,
                replicas,
                image,
            }
        })
        .collect();

    print_table(rows);
    Ok(())
}