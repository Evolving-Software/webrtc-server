
use actix::{Addr, AsyncContext, Handler, Message, StreamHandler};
use actix_web::{get, web, App, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws::{self, Message as WsMessage, ProtocolError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use webrtc::peer_connection::RTCPeerConnection;

mod live_streaming;

use actix::prelude::ActorContext;
use live_streaming::user_socket::{Broadcast, UserSocket};

#[derive(Debug, Serialize, Deserialize)]
struct AppState {
    active_users: AtomicUsize,
    #[serde(skip)]
    active_sockets: Mutex<Vec<Addr<StreamingWebSocket>>>,
    // New field to track peer connections by user socket address
    #[serde(skip)]
    peer_connections: Mutex<HashMap<Addr<UserSocket>, Arc<RTCPeerConnection>>>,
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

// Define the WebSocket actor
struct StreamingWebSocket {
    app_state: web::Data<AppState>,
}

impl StreamingWebSocket {
    fn new(app_state: web::Data<AppState>) -> Self {
        StreamingWebSocket { app_state }
    }

    fn register_socket(&self, socket_addr: Addr<StreamingWebSocket>) {
        self.app_state.active_sockets.lock().unwrap().push(socket_addr);
    }
}

impl actix::Actor for StreamingWebSocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        println!("WebSocket actor started");
        let socket_addr = ctx.address();
        self.register_socket(socket_addr);
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        println!("WebSocket actor stopped");
        let socket_addr = ctx.address();
        self.app_state.active_sockets.lock().unwrap().retain(|addr| addr != &socket_addr);
    }
}
impl Handler<Broadcast> for StreamingWebSocket {
    type Result = ();

    fn handle(&mut self, msg: Broadcast, ctx: &mut Self::Context) {
       
        match msg {
            Broadcast::Text(text) => {
              //  println!("Broadcasting text: {:?}", text);
                ctx.text(text);
            }
            Broadcast::Binary(data) => {
                println!("Broadcasting binary data: {:?}", &*data);
                ctx.binary(data);
            }
        }
    }
}


impl Handler<JsonMessage> for StreamingWebSocket {
    type Result = Result<(), Error>;

    fn handle(&mut self, msg: JsonMessage, ctx: &mut Self::Context) -> Self::Result {
        ctx.text(msg.0);
        Ok(())
    }
}

impl StreamHandler<Result<WsMessage, ProtocolError>> for StreamingWebSocket {
    fn handle(&mut self, msg: Result<WsMessage, ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(WsMessage::Binary(data)) => {
                // Forward the received data to the RTMP server
                //forward_to_rtmp_server(&data); 
                // Broadcast the data to all connected WebSocket clients
                //println!("Received binary data: {:?}", &*data);
                self.app_state.active_sockets.lock().unwrap().iter().for_each(|addr| {
                    addr.do_send(Broadcast::Binary(data.to_vec()));
                });
            }
            Ok(WsMessage::Text(text)) => {
                // Forward the received data to the RTMP server
                //forward_to_rtmp_server(&data);
                // Broadcast the text to all connected WebSocket clients
                self.app_state.active_sockets.lock().unwrap().iter().for_each(|addr| {
                    addr.do_send(Broadcast::Text(text.clone().to_string()));
                });
            }
            Ok(WsMessage::Ping(ping)) => {
                ctx.pong(&ping);
                print!("Received Ping");
            }
            Ok(WsMessage::Continuation(_)) => {
                // Handle continuation message
                ctx.text("Continuation message");
                println!("Continuation message");
            }
            Ok(WsMessage::Pong(_)) => {
                println!("Received Pong");
            }
            Ok(WsMessage::Close(reason)) => {
                println!("Closing connection: {:?}", reason);
                <ws::WebsocketContext<StreamingWebSocket> as ActorContext>::stop(ctx);
            }
            Ok(WsMessage::Nop) => {
                // Handle nop message
                println!("Nop message");
                ctx.text("Nop message");
            }
            Err(ProtocolError::Overflow) => {
                println!("Error: Overflow - Message too large");
                // Handle the overflow error (e.g., close the connection or send an error response)
                ctx.stop();
            }
            Err(ProtocolError::ContinuationNotStarted) => {
                println!("Error: ContinuationNotStarted - Received continuation message without a start message");
                // Handle the continuation error (e.g., close the connection or send an error response)
                ctx.stop();
            }
            Err(ProtocolError::ContinuationStarted) => {
                println!("Error: ContinuationStarted - New message started while continuation is in progress");
                // Handle the continuation error (e.g., close the connection or send an error response)
                ctx.stop();
            }
            Err(err) => {
                println!("Error: {:?}", err);
                // Handle other errors (e.g., close the connection or send an error response)
                ctx.stop();
            }
        }
    }
}

async fn ws_index(
    req: HttpRequest,
    stream: web::Payload,
    app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    println!("WebSocket connection established");
    ws::start(StreamingWebSocket::new(app_state), &req, stream)
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