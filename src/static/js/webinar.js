document.addEventListener('DOMContentLoaded', function () {
    const videoElement = document.querySelector('video');
    let socket;
    let peerConnection;

    // Code related to <select> elements and getUserMedia goes here
    const audioSelect = document.querySelector("select#audioSource");
    const videoSelect = document.querySelector("select#videoSource");

    if (audioSelect && videoSelect) {
        audioSelect.onchange = getStream;
        videoSelect.onchange = getStream;

        getStream()
            .then(getDevices)
            .then(gotDevices);
    } else {
        console.error("Audio/Video source selectors not found!");
    }
     

    // Establish the WebSocket connection
    const wsScheme = window.location.protocol === 'https:' ? 'wss' : 'ws';
    const wsPath = `${wsScheme}://${window.location.host}/ws`;
    socket = new WebSocket(wsPath);

    // Handle WebSocket connection open
    socket.onopen = function () {
        console.log('WebSocket connection opened');
    };

    // Handle incoming messages from the server
    socket.onmessage = function (event) {
        const message = JSON.parse(event.data);
        if (message.type === 'offer') {
            console.log('Received offer');

            if (peerConnection) {
                console.warn('PeerConnection already exists. Ignoring offer.');
                return;
            }

            peerConnection = new RTCPeerConnection();

            peerConnection.setRemoteDescription(new RTCSessionDescription(message.data))
                .then(() => {
                    console.log('Remote description set successfully. Creating answer...');
                    return peerConnection.createAnswer();
                })
                .then(answer => {
                    console.log('Answer created:', answer);
                    return peerConnection.setLocalDescription(answer);
                })
                .then(() => {
                    console.log('Local description set successfully.');
                    socket.send(JSON.stringify({ type: 'answer', data: peerConnection.localDescription }));
                })
                .catch(error => {
                    console.error('Error processing offer and creating answer:', error);
                });

            peerConnection.onicecandidate = event => {
                if (event.candidate) {
                    console.log('Sending ICE candidate:', event.candidate);
                    socket.send(JSON.stringify({ type: 'candidate', data: event.candidate }));
                }
            };
            peerConnection.oniceconnectionstatechange = () => console.log(peerConnection.iceConnectionState);

            peerConnection.onconnectionstatechange = () => console.log(peerConnection.connectionState);

            peerConnection.ontrack =  gotRemoteStream;


        } else if (message.type === 'candidate') {
            console.log('Received ICE candidate:', message.data);
            if (peerConnection) {
                peerConnection.addIceCandidate(new RTCIceCandidate(message.data))
                    .catch(error => {
                        console.error('Error adding ICE candidate:', error);
                    });
            } else {
                console.warn('PeerConnection does not exist. Ignoring candidate.');
            }
        }
    };

    videoElement.addEventListener('loadedmetadata', () => {
        videoElement.play().catch(e => console.error('Error playing video after metadata load:', e));
        console.log('Video metadata loaded');
    });

    videoElement.addEventListener('error', (e) => {
        console.error('Video element error:', e);
    });


    // Handle WebSocket connection close
    socket.onclose = function () {
        console.log('WebSocket connection closed');
    };
    // WebSocket and PeerConnection code goes here

    function gotRemoteStream(event) {
        console.log('Received remote stream');
        
        // Make sure there's at least one stream
        if (event.streams && event.streams[0]) {
            console.log("Tracks in the stream:", event.streams[0].getTracks());
            
            // Check if the videoElement is already playing this stream
            if (videoElement.srcObject !== event.streams[0]) {
                console.log('Setting received stream as the source for the video element');
                videoElement.srcObject = event.streams[0];
            }
        } else {
            console.error('No stream received');
        }
    }
    
});