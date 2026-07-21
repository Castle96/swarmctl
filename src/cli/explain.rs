pub async fn run(resource: Option<String>) -> anyhow::Result<()> {
    match resource.as_deref() {
        None | Some("help") => {
            println!("Resource Types:");
            println!("  nodes     - Docker Swarm nodes");
            println!("  services  - Docker Swarm services");
            println!("  tasks     - Docker Swarm tasks");
            println!("  networks  - Docker Swarm networks");
            println!("  secrets   - Docker Swarm secrets");
            println!("  configs   - Docker Swarm configs");
            println!("  stacks    - Docker Swarm stacks");
            println!();
            println!("Short names:");
            println!("  no, nodes      svc, services    po, tasks");
            println!("  net, networks  sec, secrets     cm, configs");
            println!("  st, stacks");
            println!();
            println!("Use 'swarmctl explain <resource>' for field details.");
        }
        Some("services") | Some("svc") => {
            println!("SERVICE FIELDS");
            println!("  name         - Service name");
            println!("  image        - Container image");
            println!("  replicas     - Number of replicas");
            println!("  mode         - Service mode (replicated or global)");
            println!("  env          - Environment variables (KEY=VAL)");
            println!("  labels       - Service labels (KEY=VAL)");
            println!("  ports        - Published ports");
            println!("  networks     - Attached networks");
            println!("  restart      - Restart policy");
            println!("  resources    - CPU/memory limits");
            println!("  update_config- Rolling update configuration");
        }
        Some("nodes") | Some("no") => {
            println!("NODE FIELDS");
            println!("  id           - Node ID");
            println!("  hostname     - Node hostname");
            println!("  status       - Node status (ready, down)");
            println!("  availability - Node availability (active, pause, drain)");
            println!("  manager      - Manager status");
            println!("  labels       - Node labels");
            println!("  role         - Node role (manager, worker)");
        }
        Some("tasks") | Some("po") => {
            println!("TASK FIELDS");
            println!("  id           - Task ID");
            println!("  name         - Task name");
            println!("  desired_state- Desired state (running, shutdown)");
            println!("  current_state- Current state");
            println!("  image        - Container image");
            println!("  node         - Node ID");
            println!("  ports        - Port mappings");
        }
        Some("networks") | Some("net") => {
            println!("NETWORK FIELDS");
            println!("  id           - Network ID");
            println!("  name         - Network name");
            println!("  driver       - Network driver (overlay, bridge)");
            println!("  scope        - Network scope (swarm, local)");
            println!("  internal    - Whether network is internal");
            println!("  labels       - Network labels");
            println!("  subnet       - Network subnet");
        }
        Some("secrets") | Some("sec") => {
            println!("SECRET FIELDS");
            println!("  id           - Secret ID");
            println!("  name         - Secret name");
            println!("  created_at   - Creation timestamp");
            println!("  labels       - Secret labels");
        }
        Some("configs") | Some("cm") => {
            println!("CONFIG FIELDS");
            println!("  id           - Config ID");
            println!("  name         - Config name");
            println!("  created_at   - Creation timestamp");
            println!("  labels       - Config labels");
        }
        Some("stacks") | Some("st") => {
            println!("STACK FIELDS");
            println!("  name         - Stack name");
            println!("  services     - Number of services");
            println!("  replicas     - Total replicas");
        }
        _ => {
            return Err(anyhow::anyhow!("Unknown resource: {}", resource.unwrap()));
        }
    }

    Ok(())
}
