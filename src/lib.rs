use sctp::EndpointConfig;
pub use actix_web::{App, HttpServer, web};
use clap::{arg, command, Parser};
use log::error;
use opentelemetry::{/*global,*/ KeyValue};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_stdout::MetricsExporterBuilder;
use opentelemetry_sdk::{Resource, runtime};

use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use tera::Tera;
use wg::WaitGroup;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::net::{SocketAddr, UdpSocket};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use bytes::{Bytes, BytesMut};

use std::io::Write;
use actix_web::{HttpRequest, HttpResponse};

use retty::channel::{InboundPipeline, Pipeline};
use retty::transport::{TaggedBytesMut, TransportContext};
use sfu::{DtlsHandler, ExceptionHandler, GatewayHandler, InterceptorHandler, SrtpHandler, StunHandler, DataChannelHandler, DemuxerHandler, RTCSessionDescription};

pub mod util;
pub mod messages;
pub mod interceptors;
pub mod types;
pub mod metrics;

use dtls::config;
use dtls::extension::extension_use_srtp::SrtpProtectionProfile;
use sfu::{RTCCertificate, SctpHandler, ServerConfig, ServerStates};



#[derive(Default, Debug, Copy, Clone, clap::ValueEnum)]
pub enum Level {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl From<Level> for log::LevelFilter {
    fn from(level: Level) -> Self {
        match level {
            Level::Error => log::LevelFilter::Error,
            Level::Warn => log::LevelFilter::Warn,
            Level::Info => log::LevelFilter::Info,
            Level::Debug => log::LevelFilter::Debug,
            Level::Trace => log::LevelFilter::Trace,
        }
    }
}

#[derive(Parser)]
#[command(name = "SFU Server")]
#[command(author = "Rusty Rain <y@ngr.tc>")]
#[command(version = "0.1.0")]
#[command(about = "An example of SFU Server", long_about = None)]
pub struct Cli {
    #[arg(long, default_value_t = format!("127.0.0.1"))]
    pub host: String,
    #[arg(short, long, default_value_t = 8080)]
    pub signal_port: u16,
    #[arg(long, default_value_t = 3478)]
    pub media_port_min: u16,
    #[arg(long, default_value_t = 3495)]
    pub media_port_max: u16,

    #[arg(short, long)]
    pub force_local_loop: bool,
    #[arg(short, long)]
    pub debug: bool,
    #[arg(short, long, default_value_t = Level::Info)]
    #[clap(value_enum)]
    pub level: Level,
}

pub fn init_meter_provider(
    mut _stop_rx: async_broadcast::Receiver<()>,
    _wait_group: WaitGroup,
) -> SdkMeterProvider {
    let exporter = MetricsExporterBuilder::default()
        .with_encoder(|writer, data| {
            Ok(serde_json::to_writer_pretty(writer, &data).unwrap())
        })
        .build();
    let reader = PeriodicReader::builder(exporter, runtime::TokioCurrentThread)
        .with_interval(Duration::from_secs(30))
        .build();
    let meter_provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(Resource::new(vec![KeyValue::new("chat", "metrics")]))
        .build();

    meter_provider
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {

    let (_stop_tx, _stop_rx) = crossbeam_channel::bounded::<()>(1);
    let cli = Cli::parse();
    if cli.debug {
        env_logger::Builder::new()
            .format(|buf, record| {
                writeln!(
                    buf,
                    "{}:{} [{}] {} - {}",
                    record.file().unwrap_or("unknown"),
                    record.line().unwrap_or(0),
                    record.level(),
                    chrono::Local::now().format("%H:%M:%S.%6f"),
                    record.args()
                )
            })
            .filter(None, cli.level.into())
            .init();
    }
    
    // Figure out some public IP address, since Firefox will not accept 127.0.0.1 for WebRTC traffic.
    let host_addr = if cli.host == "127.0.0.1" && !cli.force_local_loop {
        util::select_host_address()
    } else {
        IpAddr::from_str(&cli.host)?
    };

    let _media_ports: Vec<u16> = (cli.media_port_min..=cli.media_port_max).collect();
    let (_stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);
    let mut media_port_thread_map = HashMap::new();


    let key_pair = rcgen::KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let certificates = vec![RTCCertificate::from_key_pair(key_pair)?];
    let dtls_handshake_config = Arc::new(
        config::ConfigBuilder::default()
            .with_certificates(
                certificates
                    .iter()
                    .map(|c| c.dtls_certificate.clone())
                    .collect(),
            )
            .with_srtp_protection_profiles(vec![SrtpProtectionProfile::Srtp_Aes128_Cm_Hmac_Sha1_80])
            .with_extended_master_secret(config::ExtendedMasterSecretType::Require)
            .build(false, None)?,
    );
    let sctp_endpoint_config = Arc::new(EndpointConfig::default());
    let sctp_server_config = Arc::new(sctp::ServerConfig::default());
    let server_config = Arc::new(
        ServerConfig::new(certificates)
            .with_dtls_handshake_config(dtls_handshake_config)
            .with_sctp_endpoint_config(sctp_endpoint_config)
            .with_sctp_server_config(sctp_server_config)
            .with_idle_timeout(Duration::from_secs(30)),
    );
    let (_stop_meter_tx, stop_meter_rx) = async_broadcast::broadcast::<()>(1);
    let wait_group = WaitGroup::new();
    let meter_provider = init_meter_provider(stop_meter_rx, wait_group.clone());
    let media_ports: Vec<u16> = (cli.media_port_min..=cli.media_port_max).collect();
    for port in media_ports {
        let worker = wait_group.add(1);
        let stop_rx = stop_rx.clone();
        let (signaling_tx, signaling_rx) = mpsc::sync_channel(1);

        media_port_thread_map.insert(port, signaling_tx);
        // Spin up a UDP socket for the RTC. All WebRTC traffic is going to be multiplexed over this single
        // server socket. Clients are identified via their respective remote (UDP) socket address.
        let socket = UdpSocket::bind(format!("{host_addr}:{port}"))
            .expect(&format!("binding to {host_addr}:{port}"));

        let server_config = server_config.clone();
        let meter_provider = meter_provider.clone();
        std::thread::spawn(move || {
            if let Err(err) = sync_run(stop_rx, socket, signaling_rx, server_config, meter_provider)
            {
                eprintln!("run_sfu got error: {}", err);
            }
            worker.done();
        });
    }

    let media_port_thread_map = Arc::new(media_port_thread_map);
    let signal_port = cli.signal_port;
    let host_addr = if cli.host == "127.0.0.1" && !cli.force_local_loop {
        util::select_host_address()
    } else {
        IpAddr::from_str(&cli.host)?
    };

    println!("Connect a browser to https://{}:{}", host_addr, signal_port);

    HttpServer::new(move || {
        let tera = Tera::new("templates/**/*").unwrap();
        App::new()
            .service(actix_files::Files::new("/static", "./src/static/"))
            .app_data(web::Data::new(tera))
            .app_data(web::Data::new(media_port_thread_map.clone()))
            .service(web::resource("/").route(web::get().to(index)))
            .service(web::resource("/{path}/{session_id}/{endpoint_id}")
                .route(web::get().to(web_request))
                .route(web::post().to(web_request))
            )
    })
    .bind(SocketAddr::new(IpAddr::from(Ipv4Addr::new(0,0,0,0)), 80))?
    .run()
    .await?;

    println!("Wait for Signaling Server and Media Server Gracefully Shutdown...");
    wait_group.wait();

    Ok(())
}

pub enum SignalingProtocolMessage {
    Ok {
        session_id: u64,
        endpoint_id: u64,
    },
    Err {
        session_id: u64,
        endpoint_id: u64,
        reason: Bytes,
    },
    Offer {
        session_id: u64,
        endpoint_id: u64,
        offer_sdp: Bytes,
    },
    Answer {
        session_id: u64,
        endpoint_id: u64,
        answer_sdp: Bytes,
    },
    Leave {
        session_id: u64,
        endpoint_id: u64,
    },
}

pub struct SignalingMessage {
    pub request: SignalingProtocolMessage,
    pub response_tx: SyncSender<SignalingProtocolMessage>,
}

pub async fn index(tera: web::Data<Tera>) -> HttpResponse {
    let rendered = tera.render("chat.html.tera", &tera::Context::new()).unwrap();
    HttpResponse::Ok().content_type("text/html").body(rendered)
}


fn build_pipeline(local_addr: SocketAddr, server_states: Rc<RefCell<ServerStates>>) -> Rc<Pipeline<TaggedBytesMut, TaggedBytesMut>> {
    let pipeline: Pipeline<TaggedBytesMut, TaggedBytesMut> = Pipeline::new();

    let demuxer_handler = DemuxerHandler::new();
    let stun_handler = StunHandler::new();
    // DTLS
    let dtls_handler = DtlsHandler::new(local_addr, Rc::clone(&server_states));
    let sctp_handler = SctpHandler::new(local_addr, Rc::clone(&server_states));
    let data_channel_handler = DataChannelHandler::new();
    // SRTP
    let srtp_handler = SrtpHandler::new(Rc::clone(&server_states));
    let interceptor_handler = InterceptorHandler::new(Rc::clone(&server_states));
    // Gateway
    let gateway_handler = GatewayHandler::new(Rc::clone(&server_states));
    let exception_handler = ExceptionHandler::new();

    pipeline.add_back(demuxer_handler);
    pipeline.add_back(stun_handler);
    // DTLS
    pipeline.add_back(dtls_handler);
    pipeline.add_back(sctp_handler);
    pipeline.add_back(data_channel_handler);
    // SRTP
    pipeline.add_back(srtp_handler);
    pipeline.add_back(interceptor_handler);
    // Gateway
    pipeline.add_back(gateway_handler);
    pipeline.add_back(exception_handler);

    pipeline.finalize()
}

fn write_socket_output(socket: &UdpSocket, pipeline: &Rc<Pipeline<TaggedBytesMut, TaggedBytesMut>>) -> anyhow::Result<()> {
    while let Some(transmit) = pipeline.poll_transmit() {
        socket.send_to(&transmit.message, transmit.transport.peer_addr)?;
    }

    Ok(())
}

fn handle_offer_message(
    server_states: &Rc<RefCell<ServerStates>>,
    session_id: u64,
    endpoint_id: u64,
    offer: Bytes,
    response_tx: SyncSender<SignalingProtocolMessage>,
) -> anyhow::Result<()> {
    let try_handle = || -> anyhow::Result<Bytes> {
        let offer_str = String::from_utf8(offer.to_vec())?;
        log::info!(
            "handle_offer_message: {}/{}/{}",
            session_id,
            endpoint_id,
            offer_str,
        );
        let mut server_states = server_states.borrow_mut();

        let offer_sdp = serde_json::from_str::<RTCSessionDescription>(&offer_str)?;
        let answer = server_states.accept_offer(session_id, endpoint_id, None, offer_sdp)?;
        let answer_str = serde_json::to_string(&answer)?;
        log::info!("generate answer sdp: {}", answer_str);
        Ok(Bytes::from(answer_str))
    };

    match try_handle() {
        Ok(answer_sdp) => Ok(response_tx
            .send(SignalingProtocolMessage::Answer {
                session_id,
                endpoint_id,
                answer_sdp,
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::Other,
                    "failed to send back signaling message response".to_string(),
                )
            })?),
        Err(err) => Ok(response_tx
            .send(SignalingProtocolMessage::Err {
                session_id,
                endpoint_id,
                reason: Bytes::from(err.to_string()),
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::Other,
                    "failed to send back signaling message response".to_string(),
                )
            })?),
    }
}

fn handle_leave_message(_server_states: &Rc<RefCell<ServerStates>>,  session_id: u64, endpoint_id: u64,  response_tx: SyncSender<SignalingProtocolMessage>) -> anyhow::Result<()> {
    let try_handle = || -> anyhow::Result<()> {
        log::info!("handle_leave_message: {}/{}", session_id, endpoint_id,);
        Ok(())
    };

    match try_handle() {
        Ok(_) => Ok(response_tx
            .send(SignalingProtocolMessage::Ok {
                session_id,
                endpoint_id,
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::Other,
                    "failed to send back signaling message response".to_string(),
                )
            })?),
        Err(err) => Ok(response_tx
            .send(SignalingProtocolMessage::Err {
                session_id,
                endpoint_id,
                reason: Bytes::from(err.to_string()),
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::Other,
                    "failed to send back signaling message response".to_string(),
                )
            })?),
    }
}

pub fn handle_signaling_message(
    server_states: &Rc<RefCell<ServerStates>>,
    signaling_msg: SignalingMessage,
) -> anyhow::Result<()> {
    match signaling_msg.request {
        SignalingProtocolMessage::Offer {
            session_id,
            endpoint_id,
            offer_sdp,
        } => handle_offer_message(
            server_states,
            session_id,
            endpoint_id,
            offer_sdp,
            signaling_msg.response_tx,
        ),
        SignalingProtocolMessage::Leave {
            session_id,
            endpoint_id,
        } => handle_leave_message(
            server_states,
            session_id,
            endpoint_id,
            signaling_msg.response_tx,
        ),
        SignalingProtocolMessage::Ok {
            session_id,
            endpoint_id,
        }
        | SignalingProtocolMessage::Err {
            session_id,
            endpoint_id,
            reason: _,
        }
        | SignalingProtocolMessage::Answer {
            session_id,
            endpoint_id,
            answer_sdp: _,
        } => Ok(signaling_msg
            .response_tx
            .send(SignalingProtocolMessage::Err {
                session_id,
                endpoint_id,
                reason: Bytes::from("Invalid Request"),
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::Other,
                    "failed to send back signaling message response".to_string(),
                )
            })?),
    }
}

pub fn sync_run(
    stop_rx: crossbeam_channel::Receiver<()>,
    socket: UdpSocket,
    rx: Receiver<SignalingMessage>,
    server_config: Arc<ServerConfig>,
    _meter_provider: SdkMeterProvider,
) -> anyhow::Result<()> {
    let server_states = Rc::new(RefCell::new(ServerStates::new(
        server_config,
        socket.local_addr()?,
    )?));

    println!("listening {}...", socket.local_addr()?);

    let pipeline = build_pipeline(socket.local_addr()?, server_states.clone());

    let mut buf = vec![0; 2000];

    pipeline.transport_active();
    loop {
        match stop_rx.try_recv() {
            Ok(_) => break,
            Err(err) => {
                if err.is_disconnected() {
                    break;
                }
            }
        };

        write_socket_output(&socket, &pipeline)?;

        // Spawn new incoming signal message from the signaling server thread.
        if let Ok(signal_message) = rx.try_recv() {
            if let Err(err) = handle_signaling_message(&server_states, signal_message) {
                error!("handle_signaling_message got error:{}", err);
                continue;
            }
        }

        // Poll clients until they return timeout
        let mut eto = Instant::now() + Duration::from_millis(100);
        pipeline.poll_timeout(&mut eto);

        let delay_from_now = eto
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::from_secs(0));
        if delay_from_now.is_zero() {
            pipeline.handle_timeout(Instant::now());
            continue;
        }

        socket
            .set_read_timeout(Some(delay_from_now))
            .expect("setting socket read timeout");

        if let Some(input) = read_socket_input(&socket, &mut buf) {
            pipeline.read(input);
        }

        // Drive time forward in all clients.
        pipeline.handle_timeout(Instant::now());
    }
    pipeline.transport_inactive();

    println!(
        "media server on {} is gracefully down",
        socket.local_addr()?
    );
    Ok(())
}



fn read_socket_input(socket: &UdpSocket, buf: &mut [u8]) -> Option<TaggedBytesMut> {
    match socket.recv_from(buf) {
        Ok((n, peer_addr)) => {
            return Some(TaggedBytesMut {
                now: Instant::now(),
                transport: TransportContext {
                    local_addr: socket.local_addr().unwrap(),
                    peer_addr,
                    ecn: None,
                },
                message: BytesMut::from(&buf[..n]),
            });
        }

        Err(e) => match e.kind() {
            // Expected error for set_read_timeout(). One for windows, one for the rest.
            ErrorKind::WouldBlock | ErrorKind::TimedOut => None,
            _ => panic!("UdpSocket read failed: {e:?}"),
        },
    }
}



pub async fn web_request(
    req: HttpRequest,
    bytes: web::Bytes,
    path: web::Path<(String, u64, u64)>,
    tera: web::Data<Tera>,
    media_port_thread_map: web::Data<Arc<HashMap<u16, SyncSender<SignalingMessage>>>>,
) -> HttpResponse {
    let (path, session_id, endpoint_id) = path.into_inner();

    if req.method() == actix_web::http::Method::GET {
        let rendered = tera.render("chat.html.tera", &tera::Context::new()).unwrap();
        HttpResponse::Ok().content_type("text/html").body(rendered)
    } else if req.method() == actix_web::http::Method::POST {
        let mut sorted_ports: Vec<u16> = media_port_thread_map.keys().copied().collect();
        sorted_ports.sort();
        assert!(!sorted_ports.is_empty());
        let port = sorted_ports[(session_id as usize) % sorted_ports.len()];
        let tx = media_port_thread_map.get(&port);

        if let Some(tx  ) = tx {
            let offer_sdp = bytes.to_vec();

            let (response_tx, response_rx) = mpsc::sync_channel(1);
            tx.send(SignalingMessage {
                request: SignalingProtocolMessage::Offer {
                    session_id,
                    endpoint_id,
                    offer_sdp: Bytes::from(offer_sdp),
                },
                response_tx,
            })
                .expect("to send SignalingMessage instance");

            let response = response_rx.recv().expect("receive answer offer");
            match response {
                SignalingProtocolMessage::Answer {
                    session_id: _,
                    endpoint_id: _,
                    answer_sdp,
                } => HttpResponse::Ok()
                    .content_type("application/json")
                    .body(answer_sdp),
                _ => HttpResponse::NotFound().finish(),
            }
        } else {
            HttpResponse::NotAcceptable().finish()
        }
    } else {
        HttpResponse::MethodNotAllowed().finish()
    }
}