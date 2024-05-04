let socket;
let encoder;
let isStarted = false;
let isChannelReady = false;
let isInitiator = false;
let localStream;

document.addEventListener('DOMContentLoaded', function () {
    const activeUsersElement = document.getElementById('activeUsers');
    const wsScheme = window.location.protocol === 'https:' ? 'ws' : 'ws';
    const wsPath = `${wsScheme}://${window.location.host}/ws`;
    socket = new WebSocket(wsPath);

    socket.onerror = function (event) {
        console.log('WebSocket Error:', event);
    };

    // You can also handle socket.onopen and socket.onclose as needed

    const videoElement = document.getElementById('videoElement');

    document.getElementById('startButton').addEventListener('click', function () {
        navigator.mediaDevices.getUserMedia({ video: true, audio: true })
            .then(async stream => {
                console.log('Accessing the user webcam:', stream);
                let videoTrack = stream.getVideoTracks()[0];
                let audioTrack = stream.getAudioTracks()[0];

                let encoder = new VideoEncoder({
                    output: (chunk) => {
                        console.log('Encoded video chunk:', chunk);
                        socket.send(chunk);
                    },

                    error: (error) => {
                        console.error('Error encoding video:', error);
                    },
                });

                encoder.configure({
                    codec: 'vp8',
                    width: 640,
                    height: 480,
                    framerate: 30,
                });

                videoElement.srcObject = stream;
                videoElement.play()
                    .catch(error => {
                        console.error('Error playing video:', error);
                    });

                encoder.encode(videoTrack).then(() => {
                    console.log('Video encoding complete');
                }
                ).catch(error => {
                    console.error('Error encoding video:', error);
                });
            })
            .catch(function (error) {
                console.error('Error accessing the user webcam:', error);
            });
    });

    let queuedCandidates = [];

    socket.onmessage = function (message) {

        console.log('Received message from server:', message.data);
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
                case 'message':
                    if (message.data.startsWith('User count: ')) {
                        console.log('User count:', message.replace('User count: ', ''));
                        activeUsersElement.textContent = message.replace('User count: ', '');
                    }

                    if (message.data === "active_users") {
                        const activeUsersElement = document.getElementById('activeUsers');
                        activeUsersElement.textContent = message.data;
                        console.log('Active users:', message.data);
                    }

                    if (message.data === "Text data: [object EncodedVideoChunk]") {
                        let data = message.data.replace("Text data: ", "");
                        // CONVERT THE RECEIVED STRING TO A EncodedVideoChunk OBJECT
                        console.log('Received message:', data);
                        const encodedVideoChunk = new EncodedVideoChunk(data);

                        console.log('Received message:', encodedVideoChunk);

                        const decoder = new VideoDecoder({
                            output: (frame, metadata) => {
                                console.log('Decoded video frame:', frame);
                            },
                            error: error => {
                                console.error('Error decoding video frame:', error);
                            },
                        });

                        decoder.configure({
                            codec: 'vp8',
                            codedWidth: 640,
                            codedHeight: 480,
                            displayWidth: 640,
                            displayHeight: 480,
                        });

                        decoder.decode(encodedVideoChunk).then(decodedFrame => {
                            const canvas = document.createElement('canvas');
                            const context = canvas.getContext('2d');
                            canvas.width = decodedFrame.displayWidth;
                            canvas.height = decodedFrame.displayHeight;
                            context.drawImage(decodedFrame, 0, 0, decodedFrame.displayWidth, decodedFrame.displayHeight);

                            // Display the decoded frame on a canvas
                            document.body.appendChild(canvas);
                        }).catch(error => {
                            if (error.message.includes('key frame')) {
                                console.warn('Key frame required. Flushing decoder and requesting a key frame.');
                                decoder.flush().then(() => {
                                    // Request a key frame from the server
                                    socket.send('request_key_frame');
                                });
                            } else {
                                console.error('Error decoding video frame:', error);
                            }
                        });
                    }

                    console.log('Received message:', message.data);
                    break;

                case '':
                    // add the Blob to the media source buffer
                    mediaSourceBuffer.appendBuffer(message);
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


function checkAndStart() {
    if(!isStarted && typeof localStream != 'undefined' && isChannelReady) {
        createPeerConnection();
        isStarted = true;
        if(isInitiator) {
            doCall();
        }
    }
}