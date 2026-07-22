pub async fn run() -> anyhow::Result<()> {
    println!(
        "{:<20} {:<15} {:<30} {:<20}",
        "NAME", "SHORTNAMES", "APIVERSION", "NAMESPACED"
    );
    println!("{}", "-".repeat(85));

    let resources = vec![
        ("nodes", "no", "v1.25+", "false"),
        ("services", "svc", "v1.25+", "false"),
        ("tasks", "po", "v1.25+", "false"),
        ("networks", "net", "v1.25+", "false"),
        ("secrets", "sec", "v1.25+", "false"),
        ("configs", "cm", "v1.25+", "false"),
    ];

    for (name, short, version, namespaced) in resources {
        println!("{:<20} {:<15} {:<30} {:<20}", name, short, version, namespaced);
    }

    println!();
    println!("Use 'swarmctl explain <resource>' for field details.");

    Ok(())
}
