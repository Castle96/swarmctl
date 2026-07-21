use crate::api::client::DockerClient;
use futures::StreamExt;

pub async fn run(client: &DockerClient) -> anyhow::Result<()> {
    let mut stream = client
        .inner()
        .events(None::<bollard::query_parameters::EventsOptions>);

    while let Some(event) = stream.next().await {
        match event {
            Ok(ev) => {
                let action = ev.action.unwrap_or_default();
                let type_ = ev.typ.map(|t| format!("{:?}", t)).unwrap_or_default();
                let id = ev
                    .actor
                    .as_ref()
                    .and_then(|a| a.id.clone())
                    .unwrap_or_default();
                let time = ev.time.unwrap_or(0);
                println!("[{}] {} {}: {}", time, type_, action, id);
            }
            Err(e) => {
                eprintln!("Event error: {}", e);
            }
        }
    }

    Ok(())
}
