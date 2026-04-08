use bollard::Docker;
use bollard::query_parameters::ListServicesOptions;

#[allow(dead_code)]
pub async fn list_stacks(_docker: &Docker) -> anyhow::Result<Vec<StackSummary>> {
    Ok(Vec::new())
}

#[allow(dead_code)]
pub async fn get_stack_services(_docker: &Docker, _stack_name: &str) -> anyhow::Result<Vec<bollard::models::Service>> {
    Ok(Vec::new())
}

#[derive(Debug, Clone)]
pub struct StackSummary {
    pub name: String,
    pub services: usize,
    pub replicas: usize,
}
