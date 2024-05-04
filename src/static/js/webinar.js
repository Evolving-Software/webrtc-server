const videoElement = document.querySelector('video');
let mediaSource = new MediaSource();
videoElement.src = URL.createObjectURL(mediaSource);
let sourceBuffer;


// Assuming WebSocket connection is already established as 'ws'
const canvas = document.getElementById('videoCanvas');
const ctx = canvas.getContext('2d');
let videoDecoder = new VideoDecoder({
    output(frame) {
        // Handle the decoded frame, e.g., draw it on a canvas
        // Note: 'frame' needs to be closed to release system resources
        ctx.drawImage(frame, 0, 0, canvas.width, canvas.height);
        frame.close(); // It's important to close the frame when done
    },
    error(e) {
        console.error('VideoDecoder error:', e);
    }
});

// Configure the decoder to match the encoder settings
// Make sure to adjust the codec and other parameters as needed
videoDecoder.configure({
    codec: 'vp8',
    width: 640,
    height: 480
});


mediaSource.addEventListener('sourceopen', function () {
    // This will be executed once MediaSource is ready.
    console.log('MediaSource opened');
    sourceBuffer = mediaSource.addSourceBuffer('video/webm; codecs="vp8"');
    sourceBuffer.addEventListener('updateend', () => {
        if (mediaSource.readyState === 'open') {
            mediaSource.endOfStream();
        }
    });
});
// Establish the WebSocket connection
const wsScheme = window.location.protocol === 'https:' ? 'wss' : 'ws';
const wsPath = `${wsScheme}://${window.location.host}/ws`;

let decoder;

socket = new WebSocket(wsPath);
socket.binaryType = 'arraybuffer';
socket.onopen = function () {
    console.log('WebSocket connection opened');
    // Send a ping message to the server
    socket.send('ping');
};

socket.onclose = function (event) {
    console.log('WebSocket connection closed:', event);
};


socket.onmessage = async (event) => {

    // Assuming 'data' is an ArrayBuffer containing the encoded video frame
    // For simplicity, this example assumes the data is ready to be decoded without additional processing
    // In practice, you may need to handle different data formats or include additional metadata
    console.log(event);
    if (event.data !== 'ping') {
        let chunk = new EncodedVideoChunk({
            type: 'key', // or 'delta' for non-key frames; might need additional logic to determine
            timestamp: performance.now(), // Example timestamp; use actual frame timing
            data: event.data // The actual encoded frame data
        });

        // Decode the chunk
        videoDecoder.decode(chunk);
    }
};

function sourceOpen() {
    URL.revokeObjectURL(videoElement.src);
    console.log('MediaSource readyState:', mediaSource.readyState);

    try {
        const sourceBuffer = mediaSource.addSourceBuffer('video/webm; codecs="vp8"');
        sourceBuffer.addEventListener('updateend', function () {
            if (mediaSource.readyState === 'open') {
                mediaSource.endOfStream();
            }
        });
        sourceBuffer.appendBuffer(event.data);
    } catch (error) {
        console.error('Error creating or appending to sourceBuffer:', error);
    }
}

socket.onerror = function (error) {
    console.error('WebSocket error:', error);
};

