const byId = (id) => document.getElementById(id);
const byTag = (tag) => [].slice.call(document.getElementsByTagName(tag));
var callback = null;

byId("session").value = Math.floor(Math.random() * 1000000000);
let endpointId = Math.floor(Math.random() * 1000000000);
let streamCam;
let streamMic;
let dataChannel;
let rtc = new RTCPeerConnection();
rtc.oniceconnectionstatechange = () => {
    byId('ice_status').innerText = rtc.iceConnectionState;
    if (rtc.iceConnectionState == 'disconnected' || rtc.iceConnectionState == 'failed') {
        if (streamCam) {
            streamCam.getTracks()[0]?.stop();
        }
        if (streamMic) {
            streamMic.getTracks()[0]?.stop();
        }
        rtc.close();
        byId('join').disabled = true;
        byId('leave').disabled = true;
        byId('cam').disabled = true;
        byId('mic').disabled = true;
    }
};

async function negotiate() {
    const offer = await rtc.createOffer();
    console.log('do offer', offer.sdp.split('\r\n'));
    rtc.setLocalDescription(offer);
    await dataChannel.send(JSON.stringify(offer));
    const json = await new Promise((rs) => {
        callback = rs;
    });
    const answer = JSON.parse(json);
    console.log('received answer', answer.sdp.split('\r\n'));
    try {
        rtc.setRemoteDescription(answer);
    } catch (error) {
        console.log('rtc.setRemoteDescription(answer) with error: ', error);
    }
}

async function handleOffer(json) {
    const offer = JSON.parse(json);
    console.log('handle offer', offer.sdp.split('\r\n'));
    try {
        rtc.setRemoteDescription(offer);
    } catch (error) {
        console.log('rtc.setRemoteDescription(offer) with error: ', error);
    }
    const answer = await rtc.createAnswer();
    console.log('offer response', answer.sdp.split('\r\n'));
    rtc.setLocalDescription(answer);
    await dataChannel.send(JSON.stringify(answer));
}

async function startCam() {
    byId('cam').disabled = true;
    streamCam = await navigator.mediaDevices.getUserMedia({
        video: {
            width: 640,
            height: 360,
        },
    });
    const tr = rtc.addTransceiver(streamCam.getTracks()[0], {
        direction: "sendonly",
        streams: [streamCam],
        // This table shows the valid values for simulcast.
        //
        // https://chromium.googlesource.com/external/webrtc/+/branch-heads/49/talk/media/webrtc/simulcast.cc
        // These tables describe from which resolution we can use how many
        // simulcast layers at what bitrates (maximum, target, and minimum).
        // Important!! Keep this table from high resolution to low resolution.
        // const SimulcastFormat kSimulcastFormats[] = {
        //   {1920, 1080, 3, 5000, 4000, 800},
        //   {1280, 720, 3,  2500, 2500, 600},
        //   {960, 540, 3, 900, 900, 450},
        //   {640, 360, 2, 700, 500, 150},
        //   {480, 270, 2, 450, 350, 150},
        //   {320, 180, 1, 200, 150, 30},
        //   {0, 0, 1, 200, 150, 30}
        // };

        // Uncomment this to enable simulcast. The actual selected
        // simulcast level is hardcoded in sync_chat.
        // sendEncodings: [
        //     { rid: "h", maxBitrate: 700 * 1024 },
        //     { rid: "l", maxBitrate: 150 * 1024 }
        // ]
    });
    await negotiate();
}

async function startMic() {
    byId('mic').disabled = true;
    streamMic = await navigator.mediaDevices.getUserMedia({
        audio: true,
    });
    const tr = rtc.addTransceiver(streamMic.getTracks()[0], {
        streams: [streamMic],
        direction: "sendonly"
    });
    await negotiate();
}

rtc.ontrack = (e) => {
    console.log('ontrack', e.track);
    const track = e.track;
    const domId = `media-${track.id}`;
    const el = document.createElement('video');
    if (byId(domId)) {
        // we aleady have this track
        return;
    }
    el.id = domId;
    el.width = 500;
    byId('media').appendChild(el);
    el.controls = true;
    el.autoplay = true;
    setTimeout(() => {
        const media = new MediaStream();
        media.addTrack(track);
        el.srcObject = media;
    }, 1);
    track.addEventListener('mute', () => {
        console.log('track muted', track);
        el.parentNode.removeChild(el);
    });
    track.addEventListener('unmute', () => {
        console.log('track unmuted', track);
        byId('media').appendChild(el);
    });
};

async function startRtc() {
    let path = '/offer/' + byId("session").value + '/' + endpointId;
    byId('ice_status').innerText = 'Connecting';
    byId('chan_status').innerText = 'Joining session ' + byId("session").value + ' as endpoint ' + endpointId;
    byId("session").disabled = true;
    byId('join').disabled = true;
    byId('leave').disabled = false;
    dataChannel = rtc.createDataChannel("offer/answer");
    dataChannel.onmessage = (event) => {
        let json = JSON.parse(event.data);
        if (json.type == 'offer') {
            // no callback probably means it's an offer
            handleOffer(event.data);
        } else if (json.type == 'answer') {
            callback(event.data);
            callback = null;
        }
    };
    dataChannel.onopen = () => {
        byId('chan_status').innerText = 'Joined session ' + byId("session").value + ' as endpoint ' + endpointId;
        byId('mic').disabled = false;
        byId('cam').disabled = false;
    };
    const offer = await rtc.createOffer();
    rtc.setLocalDescription(offer);
    console.log('POST offer', offer.sdp.split('\r\n'));

    const res = await fetch(path, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json'
        },
        body: JSON.stringify(offer),
    });

    const answer = await res.json();
    rtc.setRemoteDescription(answer);
    console.log('POST answer', answer.sdp.split('\r\n'));
}

async function leaveRtc() {
    byId('ice_status').innerText = 'Waiting';
    byId('chan_status').innerText = 'Click Join Button...'
    byId("session").disabled = false;
    byId('join').disabled = false;
    byId('leave').disabled = true;
    rtc.close();

    let path = '/leave/' + byId("session").value + '/' + endpointId;
    const res = await fetch(path, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json'
        }
    });

    rtc = new RTCPeerConnection();
}