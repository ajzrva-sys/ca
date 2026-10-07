use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let scope = args.next().expect("missing scope");
    let candidate = args.next().expect("missing candidate");
    assert!(args.next().is_none(), "unexpected argument");
    let allowed = reasoning_pilot_permission_scope_matching::is_path_allowed(
        Path::new(&scope),
        Path::new(&candidate),
    );
    println!("{}", if allowed { "allow" } else { "deny" });
}
