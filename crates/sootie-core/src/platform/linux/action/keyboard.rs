use std::process::Command;

pub fn type_text(text: &str) -> Result<(), String> {
    Command::new("xdotool")
        .arg("type")
        .arg("--clearmodifiers")
        .arg(text)
        .output()
        .map_err(|e| format!("Type failed: {}", e))?;

    Ok(())
}