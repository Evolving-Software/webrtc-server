use actix_web::rt::signal;
use std::sync::Arc;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtcp::packet::Packet;

use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;

use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine as _,
};
use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::Error;

// pub struct Broadcast {
//     pub peer_connection: Arc<RTCPeerConnection>,
//     pub media_engine: MediaEngine,
// }

// pub struct MediaStream {
//     pub id: String,
//     pub label: String,
//     pub kind: String,
//     pub ssrc: u32,
//     pub codec: RTPCodecType,
// }

pub async fn handle_media_stream(
    peer_connection: Arc<RTCPeerConnection>,
) -> Result<(), webrtc::Error> {
    // Allow us to receive 1 video track
    peer_connection
        .add_transceiver_from_kind(RTPCodecType::Video, None)
        .await?;

    let (local_track_chan_tx, mut local_track_chan_rx) =
        tokio::sync::mpsc::channel::<Arc<TrackLocalStaticRTP>>(1);

    let local_track_chan_tx = Arc::new(local_track_chan_tx);
    // Set a handler for when a new remote track starts, this handler copies inbound RTP packets,
    // replaces the SSRC and sends them back
    let pc = Arc::downgrade(&peer_connection);

    peer_connection.on_track(Box::new(move |track, _, _| {
        let media_ssrc = track.ssrc();
        let pc2 = pc.clone();

        actix::spawn(async move {
            loop {
                // Delay for 3 seconds between PLI messages
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                if let Some(pc) = pc2.upgrade() {
                    let pli = PictureLossIndication {
                        sender_ssrc: media_ssrc,
                        media_ssrc,
                    };
                    let boxed_pli: Box<dyn Packet + Send + Sync> = Box::new(pli);
                    let rtcp_packets: &[Box<dyn Packet + Send + Sync>] = &[boxed_pli];

                    if let Err(err) = pc.write_rtcp(rtcp_packets).await {
                        println!("write_rtcp got error: {err}");
                    }
                }
            }
        });
        let local_track_chan_tx2 = Arc::clone(&local_track_chan_tx);
        actix::spawn(async move {
            // Create Track that we send video back to browser on
            let local_track = Arc::new(TrackLocalStaticRTP::new(
                track.codec().capability,
                "video".to_owned(),
                "webrtc-rs".to_owned(),
            ));
            let _ = local_track_chan_tx2.send(Arc::clone(&local_track)).await;

            // Read RTP packets being sent to webrtc-rs
            while let Ok((rtp, _)) = track.read_rtp().await {
                // save to disk
                println!("Packet received: {:?}", &rtp);
                if let Err(err) = local_track.write_rtp(&rtp).await {
                    if Error::ErrClosedPipe != err {
                        print!("output track write_rtp got error: {err} and break");
                        break;
                    } else {
                        print!("output track write_rtp got error: {err}");
                    }
                }
            }
        });

        Box::pin(async {})
    }));

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
        println!("Peer Connection State has changed: {s}");
        Box::pin(async {})
    }));

    // Set the remote SessionDescription
    peer_connection
        .set_remote_description(RTCSessionDescription::offer(format!("")).unwrap())
        .await?;

    // Create an answer
    let answer = peer_connection.create_answer(None).await?;

    // Create channel that is blocked until ICE Gathering is complete
    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    // Sets the LocalDescription, and starts our UDP listeners
    peer_connection.set_local_description(answer).await?;

    // Block until ICE Gathering is complete, disabling trickle ICE
    // we do this because we only can exchange one signaling message
    // in a production application you should exchange ICE Candidates via OnICECandidate
    let _ = gather_complete.recv().await;

    // Output the answer in base64 so we can paste it in browser
    let local_desc = peer_connection.local_description().await.unwrap();
    let json_str = serde_json::to_string(&local_desc).unwrap();

    let b64 = general_purpose::STANDARD.encode(json_str.as_bytes());
    println!("{b64}");

    let local_track = local_track_chan_rx.recv().await.unwrap();
    loop {
        println!("\nCurl an base64 SDP to start sendonly peer connection");

        // Create a MediaEngine object to configure the supported codec
        let mut m = webrtc::api::media_engine::MediaEngine::default();

        m.register_default_codecs()?;

        // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
        // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
        // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
        // for each PeerConnection.
        let mut registry = webrtc::interceptor::registry::Registry::new();

        // Use the default set of Interceptors
        registry =
            webrtc::api::interceptor_registry::register_default_interceptors(registry, &mut m)?;

        // Create the API object with the MediaEngine
        let api = webrtc::api::APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        // Prepare the configuration
        let config = webrtc::peer_connection::configuration::RTCConfiguration {
            ice_servers: vec![webrtc::ice_transport::ice_server::RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Create a new RTCPeerConnection
        let peer_connection = Arc::new(api.new_peer_connection(config).await?);

        let rtp_sender = peer_connection
            .add_track(Arc::clone(&local_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        // Read incoming RTCP packets
        // Before these packets are returned they are processed by interceptors. For things
        // like NACK this needs to be called.
        tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
            Result::<(), Error>::Ok(())
        });

        // Set the handler for Peer connection state
        // This will notify you when the peer has connected/disconnected
        peer_connection.on_peer_connection_state_change(Box::new(
            move |s: RTCPeerConnectionState| {
                println!("Peer Connection State has changed: {s}");
                Box::pin(async {})
            },
        ));

        // Set the remote SessionDescription
        peer_connection
            .set_remote_description(RTCSessionDescription::offer(format!("")).unwrap())
            .await?;

        // Create an answer
        let answer = peer_connection.create_answer(None).await?;

        // Create channel that is blocked until ICE Gathering is complete
        let mut gather_complete = peer_connection.gathering_complete_promise().await;

        // Sets the LocalDescription, and starts our UDP listeners
        peer_connection.set_local_description(answer).await?;

        // Block until ICE Gathering is complete, disabling trickle ICE
        // we do this because we only can exchange one signaling message
        // in a production application you should exchange ICE Candidates via OnICECandidate
        let _ = gather_complete.recv().await;

        if let Some(local_desc) = peer_connection.local_description().await {
            let json_str = serde_json::to_string(&local_desc).unwrap();
            let b64 = general_purpose::STANDARD.encode(json_str.as_bytes());
            println!("{b64}");
        } else {
            println!("generate local_description failed!");
        }
        return Ok(());
    }
}
