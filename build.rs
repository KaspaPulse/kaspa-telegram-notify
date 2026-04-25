fn main() {
    if cfg!(target_os = "windows") {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/IM", "kaspa-pulse.exe"])
            .status();
    }
}
