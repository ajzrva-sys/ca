pub fn recover(bytes: &[u8]) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let mut records = Vec::new();
    for frame in text.split_inclusive('\n') {
        if !frame.ends_with('\n') {
            return Err("truncated journal frame".to_string());
        }
        let frame = &frame[..frame.len() - 1];
        let (length, payload) = frame
            .split_once(':')
            .ok_or_else(|| "missing frame separator".to_string())?;
        let length = length
            .parse::<usize>()
            .map_err(|_| "invalid frame length".to_string())?;
        if payload.len() != length {
            return Err("frame length mismatch".to_string());
        }
        records.push(payload.to_string());
    }
    Ok(records)
}
