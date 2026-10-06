// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().nth(1).as_deref() == Some(ottid_core::discord_mute::HELPER_ARG) {
        if ottid_core::discord_mute::helper_main().is_err() {
            std::process::exit(1);
        }
        return;
    }
    ottid_lib::run();
}
