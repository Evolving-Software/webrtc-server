use clap::{error::ErrorKind, CommandFactory, Parser};
use webrtc_streaming_actix::{Cli, Level};

#[test]
fn cli_reports_package_version() {
    assert_eq!(
        Cli::command().get_version(),
        Some(env!("CARGO_PKG_VERSION"))
    );

    let error = Cli::try_parse_from(["sfu-server", "--version"])
        .err()
        .expect("--version should display the version and exit");
    assert_eq!(error.kind(), ErrorKind::DisplayVersion);
    assert!(error.to_string().contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn cli_preserves_server_defaults() {
    let cli = Cli::try_parse_from(["sfu-server"]).expect("default arguments should parse");

    assert_eq!(cli.host, "127.0.0.1");
    assert_eq!(cli.signal_port, 8080);
    assert_eq!(cli.media_port_min, 3478);
    assert_eq!(cli.media_port_max, 3495);
    assert!(!cli.force_local_loop);
    assert!(!cli.debug);
    assert!(matches!(cli.level, Level::Info));
}
