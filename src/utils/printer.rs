use tabled::{Table, Tabled};
use serde::Serialize;

pub fn print_table<T: Tabled>(rows: Vec<T>) {
    let table = Table::new(rows);
    println!("{}", table);
}

pub fn print_json<T: Serialize>(data: &T) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(data)?;
    println!("{}", json);
    Ok(())
}

pub fn print_yaml<T: Serialize>(data: &T) -> anyhow::Result<()> {
    let yaml = serde_yaml::to_string(data)?;
    println!("{}", yaml);
    Ok(())
}