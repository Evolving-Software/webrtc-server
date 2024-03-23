

use std::sync::atomic::Ordering;
use std::io::Error;

use actix_web::web;
use actix_web_actors::ws::{self};
use actix::{Actor, Handler, StreamHandler};
use actix::AsyncContext; 

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_H264, MIME_TYPE_OPUS};
use webrtc::api::APIBuilder;
use webrtc::media::io::Writer;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::media::io::h264_writer::H264Writer;
use webrtc::media::io::ogg_writer::OggWriter;
use webrtc::peer_connection::configuration::RTCConfiguration;


use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType,
};



use webrtc::util::sync::Mutex;
use std::fs::File;
use std::sync::Arc;



use crate::{AppState, JsonMessage, SignalingMessage, UpdatePeerConnection, VideoData};

use super::broadcast::{self, handle_media_stream}; 

#[derive(Serialize, Deserialize)]
pub struct UserSocket {
    pub app_state: web::Data<AppState>,
    #[serde(skip_serializing, skip_deserializing)]
    pub peer_connection: Option<Arc<RTCPeerConnection>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub notify: Arc<Notify>,
    #[serde(skip_serializing, skip_deserializing)]
    pub h264_writer: Option<H264Writer<File>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub ogg_writer: Option<OggWriter<File>>,
    pub video_data: Vec<u8>,
}


impl Default for UserSocket {
    fn default() -> Self {
        UserSocket {
            app_state: web::Data::new(AppState {
                active_users: Default::default(),
                active_sockets: Default::default(),
                peer_connections: Default::default(),
            }),
            peer_connection: None,
            notify: Arc::new(Notify::new()),
            h264_writer: Some(H264Writer::new(File::create("output.h264").unwrap())),
            ogg_writer: Some(OggWriter::new(File::create("output.opus").unwrap(), 48000, 2).unwrap()),
            video_data: Vec::new(),
        }
    }
}

impl Clone for UserSocket {
    fn clone(&self) -> Self {
        UserSocket {
            app_state: self.app_state.clone(),
            peer_connection: self.peer_connection.clone(),
            notify: self.notify.clone(),
            h264_writer: None, // Or however you choose to handle this
            ogg_writer: None,  // Or however you choose to handle this
            video_data: self.video_data.clone(),
        }
    }
}



 

impl Actor for UserSocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.app_state.active_users.fetch_add(1, Ordering::SeqCst);
        self.app_state.active_sockets.lock().unwrap().push(ctx.address());
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        self.app_state.active_users.fetch_sub(1, Ordering::SeqCst);
        let socket_index = self.app_state.active_sockets.lock().unwrap().iter().position(|x| *x == ctx.address()).unwrap();
        self.app_state.active_sockets.lock().unwrap().remove(socket_index);

        for _socket in self.app_state.active_sockets.lock().unwrap().iter() {
            let _active_users = self.app_state.active_users.load(Ordering::SeqCst);
        }
    }    
}


impl Handler<UpdatePeerConnection> for UserSocket {
    type Result = ();

    fn handle(&mut self, msg: UpdatePeerConnection, ctx: &mut Self::Context) -> Self::Result {
        self.peer_connection = Some(msg.0.clone());
        // Update the AppState with the new or updated peer connection
        let mut peer_connections = self.app_state.peer_connections.lock().unwrap();
        peer_connections.insert(ctx.address(), msg.0);
    }    
}
// impl AppState {
//     // Retrieve all client peer connections except for the broadcaster's
//     pub fn get_all_viewer_peer_connections(&self, broadcaster: Addr<UserSocket>) -> Vec<Arc<RTCPeerConnection>> {
//         let peer_connections = self.peer_connections.lock().unwrap();
//         peer_connections.iter()
//             .filter(|(addr, _)| **addr != broadcaster)
//             .map(|(_, pc)| pc.clone())
//             .collect()
//     }
// }



impl Handler<VideoData> for UserSocket {
    type Result = ();

   
    fn handle(&mut self, _msg: VideoData, _ctx: &mut Self::Context) {
        // Clone necessary data to be used inside the async block
        let peer_connection_clone = self.peer_connection.clone();
        // Spawn an async block to handle the asynchronous part
        actix::spawn(async move {
             handle_media_stream(peer_connection_clone.unwrap()).await.unwrap();

            // If you need to send a message back to your actor, you can do so
            // For example, to update state based on the result of the async operation
            // addr_clone.do_send(UpdateStateMessage { ... });
        });
    }
}


struct MutexWriter(Mutex<H264Writer<File>>);

impl Writer for MutexWriter {
    fn write_rtp(&mut self, pkt: &webrtc::rtp::packet::Packet) -> Result<(), webrtc::media::Error> {
        let mut writer = self.0.lock();
        writer.write_rtp(pkt)
    }
    
    fn close(&mut self) -> Result<(), webrtc::media::Error> {
        let mut writer = self.0.lock();
        writer.close()
    }
}

struct MutexOggWriter(Arc<Mutex<OggWriter<File>>>);
impl Writer for MutexOggWriter {
    fn write_rtp(&mut self, pkt: &webrtc::rtp::packet::Packet) -> Result<(), webrtc::media::Error> {
        let mut writer = self.0.lock();
        writer.write_rtp(pkt)
    }
    
    fn close(&mut self) -> Result<(), webrtc::media::Error> {
        let mut writer = self.0.lock();
        writer.close()
    }
}




/// # Name: StreamHandler
/// ## Description
/// Stream handler for `UserSocket` actor
/// ## Methods
/// ### handle
/// Handles the incoming messages from the websocket
/// ### started
/// Called when the actor is started
/// ### finished
/// Called when the actor is finished
/// ## Traits
/// - `StreamHandler<Result<ws::Message, ws::ProtocolError>>`
/// - `Actor`
/// - `Handler<JsonMessage>`
/// - `Send`
/// - `Sync`
/// - `unsafe Send`
/// - `Message`
/// - `Handler<UpdatePeerConnection>`
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for UserSocket {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => {
                // Log the received text to the console
                //println!("Received text: {:?}", text);

                // Broadcast the received text to all connected clients
                for socket in self.app_state.active_sockets.lock().unwrap().iter() {
                    socket.do_send(JsonMessage(text.to_string().clone()));
                }



                
            }
            Ok(ws::Message::Binary(data)) => {

                println!("Received Binary: {:?}", data);

                                // Clone necessary parts of `self` and other needed data before moving into the async block.
                // Assuming `broadcast_video_data` only needs `self.peer_connection`.
                // Adjust according to your actual needs.
                if let Some(peer_connection) = self.peer_connection.clone() {
                    let _data_clone = data.to_vec(); // Clone the data to be sent
                    let _addr_clone = ctx.address(); // Clone the address
                    
                    actix::spawn(async move {
                        // Now use the cloned data within the async block
                        if let Err(e) = broadcast::handle_media_stream(peer_connection).await {
                            eprintln!("Error handling media stream: {:?}", e);
                        }

                        // Here you might loop through active sockets and send them the video_data
                        // Example (pseudo-code):
                        // for socket in active_sockets {
                        //     socket.do_send(VideoData(data_clone.clone())).await;
                        // }
                    });
                }
                 
                 

                if let Ok(signaling_message) = serde_json::from_str::<SignalingMessage>(&data.iter().map(|&byte| byte as char).collect::<String>()) {
                    match signaling_message {
                        SignalingMessage::Offer(offer) => {
                            if let Some(peer_connection) = &self.peer_connection {
                                let peer_connection = Arc::clone(peer_connection);
                                let _notify = Arc::clone(&self.notify);
                                let _h264_writer= H264Writer::new(File::create("output.h264").unwrap());
                                let _ogg_writer= OggWriter::new(File::create("output.opus").unwrap(), 48000, 2).unwrap();
                                let ctx_addr = ctx.address();

                                let _addr = ctx.address().clone(); 
                                actix::spawn(async move {
                                    peer_connection.set_remote_description(offer).await.unwrap();
                                    let answer = peer_connection.create_answer(None).await.unwrap();
                                    peer_connection.set_local_description(answer).await.unwrap();

                                    if let Some(local_desc) = peer_connection.local_description().await {
                                        let json_str = serde_json::to_string(&local_desc).unwrap();
                                        ctx_addr.do_send(JsonMessage(json_str));
                                    }         
                                });

 
                            }
                        }
                        SignalingMessage::Answer(_) => {


                        }
                        SignalingMessage::Candidate(candidate) => {
                            if let Some(peer_connection) = self.peer_connection.clone() {
                                match candidate.to_json() {
                                    Ok(candidate_init) => {
                                        actix::spawn(async move {
                                            peer_connection.add_ice_candidate(candidate_init).await.unwrap();
                                        });
                                    },
                                    Err(e) => {
                                        println!("Error converting ICE candidate to JSON: {:?}", e);
                                    }
                                }                            }
                        }
                    }
                }
            }
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
            }
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Ok(ws::Message::Pong(_)) => {
                println!("Received pong");
            }      
            Ok(ws::Message::Continuation(_)) => {
                println!("Received continuation");
            }
            Ok(ws::Message::Nop) => {
                println!("Received nop");
            }
            Err(e) => {
                println!("Error: {:?}", e);
            }
        }
    }

    fn started(&mut self, ctx: &mut Self::Context) {
        self.app_state.active_users.fetch_add(1, Ordering::SeqCst);
        self.app_state.active_sockets.lock().unwrap().push(ctx.address());

        self.video_data = Vec::new();

        let mut m = MediaEngine::default();
        m.register_codec(
            RTCRtpCodecParameters {
                capability: RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_H264.to_owned(),
                    clock_rate: 90000,
                    channels: 0,
                    sdp_fmtp_line: "".to_owned(),
                    rtcp_feedback: vec![],
                },
                payload_type: 102,
                ..Default::default()
            },
            RTPCodecType::Video,
        ).unwrap();

        m.register_codec(
            RTCRtpCodecParameters {
                capability: RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_OPUS.to_owned(),
                    clock_rate: 48000,
                    channels: 2,
                    sdp_fmtp_line: "".to_owned(),
                    rtcp_feedback: vec![],
                },
                payload_type: 111,
                ..Default::default()
            },
            RTPCodecType::Audio,
        ).unwrap();

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut m).unwrap();

        let api = APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let addr = ctx.address();
        actix::spawn(async move {
            let peer_connection = Arc::new(api.new_peer_connection(config).await.unwrap());
            peer_connection.add_transceiver_from_kind(RTPCodecType::Audio, None).await.unwrap();
            peer_connection.add_transceiver_from_kind(RTPCodecType::Video, None).await.unwrap();

            addr.do_send(UpdatePeerConnection(peer_connection));
        });
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        self.app_state.active_users.fetch_sub(1, Ordering::SeqCst);
        let socket_index = self.app_state.active_sockets.lock().unwrap().iter().position(|x| *x == ctx.address()).unwrap();
        self.app_state.active_sockets.lock().unwrap().remove(socket_index);

        for _socket in self.app_state.active_sockets.lock().unwrap().iter() {
            let _active_users = self.app_state.active_users.load(Ordering::SeqCst);
        }
    }
}

impl Handler<JsonMessage> for UserSocket {
    type Result = Result<(), Error>;

    fn handle(&mut self, msg: JsonMessage, ctx: &mut Self::Context) ->  Result<(), Error> {
        ctx.text(msg.0);
        Ok(())
    }
}
