use std::process::Command;

pub fn click_at(x: f64, y: f64, button: u32, count: u32) -> Result<(), String> {
    Command::new("xdotool")
        .arg("mousemove")
        .arg("--screen")
        .arg("0")
        .arg(x.to_string())
        .arg(y.to_string())
        .output()
        .map_err(|e| format!("MouseMove failed: {}", e))?;

    for _ in 0..count {
        Command::new("xdotool")
            .arg("click")
            .arg(button.to_string())
            .output()
            .map_err(|e| format!("Click failed: {}", e))?;
    }

    Ok(())
}