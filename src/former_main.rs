use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::{Error};
use actix_web::{get, web, App, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws::{self};
use actix::{Message, Addr};


use std::sync::Mutex;
use tokio::sync::Notify;



use webrtc::peer_connection::RTCPeerConnection;


use webrtc::media::io::h264_writer::H264Writer;
use webrtc::media::io::ogg_writer::OggWriter;

use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;


use std::fs::File;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;  


mod live_streaming;

use live_streaming::user_socket::UserSocket;

#[derive(Debug, Serialize, Deserialize)]
struct AppState {
    active_users: AtomicUsize,
    #[serde(skip)]
    active_sockets: Mutex<Vec<Addr<UserSocket>>>,
    // New field to track peer connections by user socket address
    #[serde(skip)]
    peer_connections: Mutex<HashMap<Addr<UserSocket>, Arc<RTCPeerConnection>>>,
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


struct UpdatePeerConnection(pub Arc<RTCPeerConnection>);

impl Message for UpdatePeerConnection {
    type Result = ();
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct VideoData(Vec<u8>);




#[derive(Message, Serialize, Deserialize, Clone, Debug)]
#[rtype(result = "Result<(), Error>")]
struct JsonMessage(String);

unsafe impl Send for JsonMessage {
    // This is safe because `JsonMessage` only contains a `String` which is `Send`

}
 

#[get("/")]
async fn index(data: web::Data<AppState>, tmpl: web::Data<tera::Tera>) -> HttpResponse {
    let active_users = data.active_users.load(Ordering::SeqCst);
    format!("Active users: {}", active_users);

    let mut ctx = tera::Context::new();
    ctx.insert("active_users", &active_users);

    let rendered = tmpl.render("index.html.tera", &ctx).unwrap();
    HttpResponse::Ok().body(rendered)
}

async fn ws_index(req: HttpRequest, stream: web::Payload, app_state: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    let h264_writer: H264Writer<File> = H264Writer::new(File::create("output.h264").unwrap());
    let ogg_writer: OggWriter<File> = OggWriter::new(File::create("output.opus").unwrap(), 48000, 2).unwrap();

    let user_socket = UserSocket {
        app_state: app_state.clone(),
        peer_connection: None,
        notify: Arc::new(Notify::new()),
        h264_writer: Some(h264_writer), // Some(h264_writer),
        ogg_writer: Some(ogg_writer), // Some(ogg_writer),
        video_data: Vec::new(),
    };

 

    let active_users = app_state.active_users.load(Ordering::SeqCst);
    for socket in app_state.active_sockets.lock().unwrap().iter() {
        socket.do_send(JsonMessage(format!("{{\"active_users\": {active_users}}}")));
        
    }



    ws::start(user_socket, &req, stream)
}

#[get("/webinar")]
async fn webinar(data: web::Data<AppState>, tmpl: web::Data<tera::Tera>) -> HttpResponse {
    let active_users = data.active_users.load(Ordering::SeqCst);
    format!("Active users: {}", active_users);

    let mut ctx = tera::Context::new();
    ctx.insert("active_users", &active_users);

    let rendered = tmpl.render("webinar.html.tera", &ctx).unwrap();
    HttpResponse::Ok().body(rendered)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app_state = web::Data::new(AppState {
        active_users: AtomicUsize::new(0),
        active_sockets: Mutex::new(Vec::new()),
        peer_connections: Mutex::new(HashMap::new()),
    });

    HttpServer::new(move || {
        let tera = tera::Tera::new("templates/**/*").unwrap();

        App::new()
            .service(actix_files::Files::new("/static", "./src/static/")) 
            .app_data(web::Data::new(tera))
            .app_data(app_state.clone())
            .service(index)
            .service(webinar)
            .route("/ws", web::get().to(ws_index))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}