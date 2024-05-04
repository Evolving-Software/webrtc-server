use std::io::Error;
use std::sync::atomic::Ordering;

use actix::AsyncContext;
use actix::{Actor, Handler, StreamHandler};
use actix_web::web;
use actix_web_actors::ws::{self};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_H264, MIME_TYPE_OPUS};
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::media::io::h264_writer::H264Writer;
use webrtc::media::io::ogg_writer::OggWriter;
use webrtc::media::io::Writer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::RTCPeerConnection;

use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType,
};

use std::fs::File;
use std::sync::Arc;
use webrtc::util::sync::Mutex;

use crate::{AppState, JsonMessage, UpdatePeerConnection, VideoData};

use super::broadcast::{self, handle_media_stream};

use actix::Message;


#[derive(Message)]
#[rtype(result = "()")]
pub enum Broadcast {
    Binary(Vec<u8>),
    Text(String),
    // Add more message types as needed
}
 
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
            ogg_writer: Some(
                OggWriter::new(File::create("output.opus").unwrap(), 48000, 2).unwrap(),
            ),
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

    // fn handle(&mut self, _msg: VideoData, _ctx: &mut Self::Context) {
    //     // Clone necessary data to be used inside the async block
    //     let peer_connection_clone = self.peer_connection.clone();
    //     // Spawn an async block to handle the asynchronous part
    //     actix::spawn(async move {
    //          handle_media_stream(peer_connection_clone.unwrap()).await.unwrap();

    //         // If you need to send a message back to your actor, you can do so
    //         // For example, to update state based on the result of the async operation
    //         // addr_clone.do_send(UpdateStateMessage { ... });
    //     });
    // }

    fn handle(&mut self, msg: VideoData, ctx: &mut Self::Context) {
        // Iterate over the connected peers and send the video data
        for socket in self.app_state.active_sockets.lock().unwrap().iter() {
            socket.do_send(JsonMessage(format!(
                "{{\"videoData\": \"{}\"}}",
                base64::encode(&msg.0)
            )));
        }
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

impl Handler<JsonMessage> for UserSocket {
    type Result = Result<(), Error>;

    fn handle(&mut self, msg: JsonMessage, ctx: &mut Self::Context) -> Result<(), Error> {
        ctx.text(msg.0);
        Ok(())
    }
}
