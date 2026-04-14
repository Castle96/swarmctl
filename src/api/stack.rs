use bollard::Docker;
use bollard::models::Service;
use bollard::query_parameters::{
    ListConfigsOptions, ListNetworksOptions, ListSecretsOptions, ListServicesOptions,
};
use std::collections::HashMap;

pub async fn list_stacks(docker: &Docker) -> anyhow::Result<Vec<StackSummary>> {
    let services = docker.list_services(None::<ListServicesOptions>).await?;

    let mut stacks: HashMap<String, StackSummary> = HashMap::new();
    for service in services {
        if let Some(labels) = service.spec.as_ref().and_then(|spec| spec.labels.as_ref()) {
            if let Some(stack_name) = labels.get("com.docker.stack.namespace") {
                let entry = stacks
                    .entry(stack_name.clone())
                    .or_insert_with(|| StackSummary {
                        name: stack_name.clone(),
                        services: 0,
                        replicas: 0,
                    });
                entry.services += 1;

                if let Some(mode) = service.spec.as_ref().and_then(|spec| spec.mode.as_ref()) {
                    if let Some(replicated) = mode.replicated.as_ref() {
                        entry.replicas += replicated.replicas.unwrap_or(0) as usize;
                    }
                }
            }
        }
    }

    let mut stacks: Vec<StackSummary> = stacks.into_values().collect();
    stacks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(stacks)
}

pub async fn get_stack_services(docker: &Docker, stack_name: &str) -> anyhow::Result<Vec<Service>> {
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![format!("com.docker.stack.namespace={}", stack_name)],
    );

    let options = ListServicesOptions {
        filters: Some(filters),
        status: None,
    };
    Ok(docker.list_services(Some(options)).await?)
}

pub async fn remove_stack(docker: &Docker, stack_name: &str) -> anyhow::Result<()> {
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![format!("com.docker.stack.namespace={}", stack_name)],
    );

    let services = docker
        .list_services(Some(ListServicesOptions {
            filters: Some(filters.clone()),
            status: None,
        }))
        .await?;
    for service in services {
        if let Some(id) = service.id {
            docker.delete_service(&id).await?;
        }
    }

    let networks = docker
        .list_networks(Some(ListNetworksOptions {
            filters: Some(filters.clone()),
        }))
        .await?;
    for network in networks {
        if let Some(id) = network.id {
            docker.remove_network(&id).await?;
        }
    }

    let configs = docker
        .list_configs(Some(ListConfigsOptions {
            filters: Some(filters.clone()),
        }))
        .await?;
    for config in configs {
        if let Some(id) = config.id {
            docker.delete_config(&id).await?;
        }
    }

    let secrets = docker
        .list_secrets(Some(ListSecretsOptions {
            filters: Some(filters),
        }))
        .await?;
    for secret in secrets {
        if let Some(id) = secret.id {
            docker.delete_secret(&id).await?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct StackSummary {
    pub name: String,
    pub services: usize,
    pub replicas: usize,
}
