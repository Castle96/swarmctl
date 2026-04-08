use crate::api::client::DockerClient;
use anyhow::Result;

pub async fn run(client: &DockerClient) -> Result<()> {
    let swarm = crate::api::swarm::get_swarm_info(client.inner()).await?;
    
    println!();
    println!("{}", style("════════════════════════════════════════════════════════════════════════════════", 80, '─'));
    println!("  \x1b[1;36mDocker Swarm Cluster Information\x1b[0m");
    println!("{}", style("────────────────────────────────────────────────────────────────────────────────", 80, '─'));
    
    if let Some(spec) = &swarm.spec {
        println!("  \x1b[1mCluster Name:\x1b[0m   {}", spec.name.as_ref().unwrap_or(&"unknown".to_string()));
    }
    
    println!("  \x1b[1mCluster ID:\x1b[0m     {}", swarm.id.as_ref().unwrap_or(&"unknown".to_string()));
    
    println!("  \x1b[1mCreated At:\x1b[0m    {}", swarm.created_at.unwrap_or_default());
    println!("  \x1b[1mUpdated At:\x1b[0m    {}", swarm.updated_at.unwrap_or_default());
    
    println!();
    println!("{}", style("────────────────────────────────────────────────────────────────────────────────", 80, '─'));
    println!("  \x1b[1mRaft Configuration:\x1b[0m");
    if let Some(raft) = &swarm.spec.as_ref().and_then(|s| s.raft.as_ref()) {
        if let Some(snapshot_interval) = raft.snapshot_interval {
            println!("    Snapshot Interval: {}", snapshot_interval);
        }
        if let Some(log_entries_for_slow_followers) = raft.log_entries_for_slow_followers {
            println!("    Log Entries for Slow Followers: {}", log_entries_for_slow_followers);
        }
        if let Some(heartbeat_tick) = raft.heartbeat_tick {
            println!("    Heartbeat Tick: {}", heartbeat_tick);
        }
        if let Some(election_tick) = raft.election_tick {
            println!("    Election Tick: {}", election_tick);
        }
    }
    
    println!();
    println!("{}", style("════════════════════════════════════════════════════════════════════════════════", 80, '─'));
    println!();
    
    Ok(())
}

fn style(s: &str, width: usize, fill: char) -> String {
    let len = s.chars().count();
    if len >= width {
        s.chars().take(width).collect()
    } else {
        format!("{}{}", s, fill.to_string().repeat(width - len))
    }
}
