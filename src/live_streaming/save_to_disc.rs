use std::sync::Arc;
use tokio::sync::Notify;
use webrtc::{media::io::Writer, track::track_remote::TrackRemote, Error}; 

pub async fn _save_to_disk(
    mut writer: Box<dyn Writer + Send>,
    track: Arc<TrackRemote>,
    notify: Arc<Notify>,
) -> Result<(), Error> {
    loop {
        tokio::select! {
            result = track.read_rtp() => {
                if let Ok((rtp_packet, _)) = result {
                   
                    writer.write_rtp(&rtp_packet).unwrap();
                } else {
                    println!("file closing begin after read_rtp error");
                    
                    if let Err(err) = writer.close() {
                        println!("file close err: {err}");
                    }
                    println!("file closing end after read_rtp error");
                    return Ok(());
                }
            }
            _ = notify.notified() => {
                println!("file closing begin after notified");
                
                if let Err(err) = writer.close() {
                    println!("file close err: {err}");
                }
                println!("file closing end after notified");
                return Ok(());
            }
        }
    }
}
