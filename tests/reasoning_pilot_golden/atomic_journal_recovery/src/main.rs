fn main() {
    let path = std::env::args().nth(1).expect("missing journal path");
    let bytes = std::fs::read(path).expect("failed to read journal");
    for record in
        reasoning_pilot_atomic_journal_recovery::recover(&bytes).expect("journal recovery failed")
    {
        println!("{record}");
    }
}
