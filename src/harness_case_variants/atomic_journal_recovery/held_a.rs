pub fn recover(bytes: &[u8]) -> Result<Vec<String>, String> {
    let complete_len = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let complete_len = if !bytes.ends_with(b"\n") && complete_len > 0 {
        bytes[..complete_len - 1]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1)
    } else {
        complete_len
    };
    let complete = &bytes[..complete_len];
    let text = std::str::from_utf8(complete).map_err(|error| error.to_string())?;
    let mut records = Vec::new();
    for frame in text.split_terminator('\n') {
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
