// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|arg| arg == "--apply-codex-config-headless") {
        if let Err(error) = smart_agent_lib::apply_codex_config_headless() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    smart_agent_lib::run()
}
