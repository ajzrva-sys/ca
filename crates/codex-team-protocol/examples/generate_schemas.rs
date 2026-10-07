use codex_team_protocol::{MechanisticActionV1, MechanisticRequestV1};
use schemars::schema_for;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: generate_schemas OUTPUT_DIRECTORY")?;
    if !output.is_dir() {
        return Err("output directory must already exist".into());
    }
    let action = serde_json::to_vec_pretty(&schema_for!(MechanisticActionV1))?;
    let request = serde_json::to_vec_pretty(&schema_for!(MechanisticRequestV1))?;
    std::fs::write(output.join("mechanistic_action.schema.json"), action)?;
    std::fs::write(output.join("mechanistic_request.schema.json"), request)?;
    Ok(())
}
