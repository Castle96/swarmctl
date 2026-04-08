use crate::api::client::DockerClient;
use crate::cli::root::ResourceType;

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: Option<String>,
    selector: Option<String>,
    force: bool,
) -> anyhow::Result<()> {
    match resource {
        ResourceType::Services => delete_service(client, name, selector, force).await?,
        ResourceType::Networks => delete_network(client, name, selector, force).await?,
        ResourceType::Secrets => delete_secret(client, name, selector, force).await?,
        ResourceType::Configs => delete_config(client, name, selector, force).await?,
        ResourceType::Tasks => delete_task(client, name, selector, force).await?,
        _ => return Err(anyhow::anyhow!("Deleting {} is not yet supported", format!("{:?}", resource).to_lowercase())),
    }

    Ok(())
}

async fn delete_service(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    _force: bool,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        println!("Deleting service {}...", name);
        client.inner().delete_service(&name).await?;
        println!("Service {} deleted", name);
    } else if let Some(_selector) = selector {
        // TODO: Implement selector-based deletion
        return Err(anyhow::anyhow!("Selector-based deletion not yet implemented"));
    } else {
        return Err(anyhow::anyhow!("Must specify service name or selector"));
    }

    Ok(())
}

async fn delete_network(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    _force: bool,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        println!("Deleting network {}...", name);
        client.inner().remove_network(&name).await?;
        println!("Network {} deleted", name);
    } else if let Some(_selector) = selector {
        return Err(anyhow::anyhow!("Selector-based deletion not yet implemented"));
    } else {
        return Err(anyhow::anyhow!("Must specify network name or selector"));
    }

    Ok(())
}

async fn delete_secret(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    _force: bool,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        println!("Deleting secret {}...", name);
        client.inner().delete_secret(&name).await?;
        println!("Secret {} deleted", name);
    } else if let Some(_selector) = selector {
        return Err(anyhow::anyhow!("Selector-based deletion not yet implemented"));
    } else {
        return Err(anyhow::anyhow!("Must specify secret name or selector"));
    }

    Ok(())
}

async fn delete_config(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    _force: bool,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        println!("Deleting config {}...", name);
        client.inner().delete_config(&name).await?;
        println!("Config {} deleted", name);
    } else if let Some(_selector) = selector {
        return Err(anyhow::anyhow!("Selector-based deletion not yet implemented"));
    } else {
        return Err(anyhow::anyhow!("Must specify config name or selector"));
    }

    Ok(())
}

async fn delete_task(
    _client: &DockerClient,
    name: Option<String>,
    _selector: Option<String>,
    _force: bool,
) -> anyhow::Result<()> {
    if let Some(task_id) = name {
        println!("Deleting task {}...", task_id);
        // Note: Docker API doesn't have a direct delete task endpoint
        // Tasks are managed through services
        return Err(anyhow::anyhow!("Task deletion is not supported. Tasks are managed through services."));
    } else {
        return Err(anyhow::anyhow!("Must specify task ID"));
    }
}