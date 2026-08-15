fn main() {
    if let Err(error) = p2p_desk_lib::run() {
        p2p_desk_lib::report_startup_error(&error);
        eprintln!("{error}");
        std::process::exit(1);
    }
}
