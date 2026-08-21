use echo_core::History;

pub fn print_history(store: &History) {
    if store.rows().is_empty() {
        println!("(empty)");
        return;
    }
    for row in store.rows() {
        println!("{}  {}  {}  {}", row.id, row.engine, row.infer_ms, row.text);
    }
}
