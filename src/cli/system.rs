use crate::api::client::DockerClient;
use crate::utils::printer::print_json;

pub async fn info(client: &DockerClient, json: bool) -> anyhow::Result<()> {
    let info = client.inner().info().await?;
    if json {
        print_json(&info)?;
    } else {
        let version = client.inner().version().await?;
        println!("Client:");
        println!("  Version:    {}", version.version.unwrap_or_default());
        println!("  API:        {}", version.api_version.unwrap_or_default());
        println!(
            "  OS/Arch:    {}/{}",
            version.os.unwrap_or_default(),
            version.arch.unwrap_or_default()
        );
        println!();
        println!("Server:");
        println!("  Version:    {}", info.server_version.unwrap_or_default());
        println!(
            "  OS/Arch:    {}/{}",
            info.operating_system.unwrap_or_default(),
            info.architecture.unwrap_or_default()
        );
        println!("  Kernel:     {}", info.kernel_version.unwrap_or_default());
        println!("  Root Dir:   {}", info.docker_root_dir.unwrap_or_default());
        println!("  CPUs:       {}", info.ncpu.unwrap_or(0));
        println!("  Total Mem:  {}", info.mem_total.unwrap_or(0));
        println!("  Name:       {}", info.name.unwrap_or_default());
        if let Some(swarm) = &info.swarm {
            println!(
                "  Swarm:      node_id={} state={:?} managers={} nodes={}",
                swarm.node_id.as_deref().unwrap_or(""),
                swarm.local_node_state.unwrap_or_default(),
                swarm.managers.unwrap_or(0),
                swarm.nodes.unwrap_or(0)
            );
        }
    }
    Ok(())
}

pub async fn df(client: &DockerClient) -> anyhow::Result<()> {
    let df = client
        .inner()
        .df(None::<bollard::query_parameters::DataUsageOptions>)
        .await?;
    if df.images_disk_usage.is_none()
        && df.containers_disk_usage.is_none()
        && df.volumes_disk_usage.is_none()
    {
        println!("Docker disk usage summary");
        println!("(No data returned by daemon)");
    }
    if let Some(images) = &df.images_disk_usage {
        println!("\nImages:");
        println!("  Active:  {}", images.active_count.unwrap_or(0));
        println!("  Total:   {}", images.total_count.unwrap_or(0));
        println!("  Size:    {} bytes", images.total_size.unwrap_or(0));
    }
    if let Some(containers) = &df.containers_disk_usage {
        println!("\nContainers:");
        println!("  Active:  {}", containers.active_count.unwrap_or(0));
        println!("  Total:   {}", containers.total_count.unwrap_or(0));
    }
    if let Some(volumes) = &df.volumes_disk_usage {
        println!("\nVolumes:");
        println!("  Active:  {}", volumes.active_count.unwrap_or(0));
        println!("  Total:   {}", volumes.total_count.unwrap_or(0));
    }
    Ok(())
}

pub async fn ping(client: &DockerClient) -> anyhow::Result<()> {
    let pong = client.inner().ping().await?;
    if pong.contains("OK") {
        println!("OK");
    } else {
        println!("{}", pong.trim());
    }
    Ok(())
}
