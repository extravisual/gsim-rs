fn main() {
    env_logger::init();

    if let Err(e) = gsim_rs::run() {
        log::error!("{e}");
        std::process::exit(1);
    }
}
