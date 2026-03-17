use tabled::{Table, Tabled};

pub fn print_table<T: Tabled>(rows: Vec<T>) {
    let table = Table::new(rows);
    println!("{}", table);
}