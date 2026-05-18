mod audio;
mod engine;
mod map;
mod raycaster;

fn main() {
    if let Err(e) = engine::run() {
        eprintln!("pfinder error: {e}");
        std::process::exit(1);
    }
}
