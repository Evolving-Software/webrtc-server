let socket;

document.addEventListener('DOMContentLoaded', function () {
    const activeUsersElement = document.getElementById('activeUsers');
    const wsScheme = window.location.protocol === 'https:' ? 'ws' : 'ws';
    const wsPath = `${wsScheme}://${window.location.host}/ws`;
    socket = new WebSocket(wsPath);

    socket.onmessage = function (event) {
        const message = event.data;
        if (message.startsWith('User count: ')) {
            activeUsersElement.textContent = message.replace('User count: ', '');
        }
    };

    socket.onerror = function (event) {
        console.log('WebSocket Error:', event);
    };

    // You can also handle socket.onopen and socket.onclose as needed

    const videoElement = document.getElementById('videoElement');

    document.getElementById('startButton').addEventListener('click', function () {
        navigator.mediaDevices.getUserMedia({ video: true, audio: true})
            .then(function (stream) {
                videoElement.srcObject = stream;
                peerConnection = new RTCPeerConnection();

                stream.getTracks().forEach(track => peerConnection.addTrack(track, stream));

                peerConnection.onicecandidate = event => {
                    if (event.candidate) {
                        socket.send(JSON.stringify({ type: 'candidate', data: event.candidate }));
                    }
                };

                peerConnection.createOffer()
                    .then(offer => {
                        peerConnection.setLocalDescription(offer);
                        socket.send(JSON.stringify({ type: 'offer', data: offer }));
                    });
            })
            .catch(function (error) {
                console.error('Error accessing the user webcam:', error);
            });
    });

    let queuedCandidates = [];

    socket.onmessage = function (event) {
        const message = JSON.parse(event.data);

        if (message === "active_users") {
            const activeUsersElement = document.getElementById('activeUsers');
            activeUsersElement.textContent = message.data;
        }

        if (message.type) {

            switch (message.type) {
                case 'offer':
                    peerConnection.setRemoteDescription(new RTCSessionDescription(message.data))
                        .then(() => peerConnection.createAnswer())
                        .then(answer => {
                            peerConnection.setLocalDescription(answer);
                            socket.send(JSON.stringify({ type: 'answer', data: answer }));
                        })
                        .catch(error => console.error('Error setting remote description:', error));
                    break;
                case 'answer':
                    peerConnection.setRemoteDescription(new RTCSessionDescription(message.data))
                        .then(() => {
                            // Process any queued candidates
                            queuedCandidates.forEach(candidate => {
                                peerConnection.addIceCandidate(new RTCIceCandidate(candidate))
                                    .catch(error => console.error('Error adding queued ICE candidate:', error));
                            });
                            // Clear the queue
                            queuedCandidates = [];
                        })
                        .catch(error => console.error('Error setting remote description:', error));
                    break;
                case 'candidate':
                    const candidate = new RTCIceCandidate(message.data);
                    if (peerConnection.remoteDescription) {
                        peerConnection.addIceCandidate(candidate)
                            .catch(error => console.error('Error adding ICE candidate:', error));
                    } else {
                        // Queue the candidate if the remote description is not yet set
                        queuedCandidates.push(candidate);
                    }
                    break;
                default:
                    console.error('Unknown message type:', message.type);
                    break;
            }
        }
    };


    document.getElementById('stopButton').addEventListener('click', function () {
        // stop the video stream
        const videoElement = document.getElementById('videoElement');
        videoElement.srcObject.getTracks().forEach(track => track.stop());
        videoElement.srcObject = null;

        // Close the peer connection
        peerConnection.close();
    });
});
