import * as wasm from "rustboyadvance-wasm";

var fps_text = document.getElementById('fps');
var canvas = document.getElementById("screen");
var ctx = canvas.getContext('2d');
var intervalId = 0;
var romData = null;
var biosData = null;
let emulator = null;

document.getElementById("skipBios").checked = JSON.parse(localStorage.getItem("skipBios"));
var shouldSkipBios = document.getElementById("skipBios").checked;

console.log("Calling wasm init routine");
wasm.init();

function loadLocalFile(localFile, callback) {
    var reader = new FileReader();
    reader.onload = function(e) {
        var data = reader.result;
        var array = new Uint8Array(data);
        callback(array);
    };
    reader.readAsArrayBuffer(localFile);
}

function ensureFilesLoaded() {
    var bios = localStorage.getItem("bios");
    if (null == biosData) {
        alert("please load bios first!");
        return false;
    }

    if (null == romData) {
        alert("rom not loaded");
        return false;
    }
    return true;
}

const convertAudioBuffer = buffer => {
    let length = buffer.length;
    const floatArray = new Float32Array(length);
    for (let i = 0; i < length; i++) {
        floatArray[i] = (buffer[i] - 32767) / 32767;
    }
    return floatArray;
}

var fpsCounter = (function() {
    var lastLoop = (new Date).getMilliseconds();
    var count = 0;
    var fps = 0;

    return function() {
        var currentLoop = (new Date).getMilliseconds();
        if (lastLoop > currentLoop) {
            fps = count;
            count = 0;
        } else {
            count += 1;
        }
        lastLoop = currentLoop;
        return fps;
    }
}());

// Create our audio context
const audioContext = new (window.AudioContext || window.webkitAudioContext)();
console.log("audio context " + audioContext);

const playAudio = emulator => {
    let audioData = emulator.collect_audio_samples();

    let frameCount = audioData.length / 2;
    const audioBuffer = audioContext.createBuffer(
        2,
        frameCount,
        audioContext.sampleRate
    );

    for (let channel = 0; channel < 2; channel++) {
        let nowBuffering = audioBuffer.getChannelData(channel);
        for (let i = 0; i < frameCount; i++) {
            // audio data frames are interleaved
            nowBuffering[i] = audioData[i*2 + channel];
        }
    }

    const audioSource = audioContext.createBufferSource();
    audioSource.buffer = audioBuffer;

    audioSource.connect(audioContext.destination);
    audioSource.start();
}

const emulatorLoop = function() {
    emulator.run_frame(ctx);
    fps_text.innerHTML = fpsCounter();
    playAudio(emulator);
}

function startEmulator() {
    if (!ensureFilesLoaded()) {
        return;
    }

    if (intervalId != 0) {
        console.log("killing emulator");
        clearInterval(intervalId);
        intervalId = 0;
        emulator = null;
    }

    emulator = new wasm.Emulator(biosData, romData);

    if (shouldSkipBios) {
        emulator.skip_bios();
    }

    intervalId = setInterval(emulatorLoop, 16);
}

const biosCached = localStorage.getItem("biosCached");
if (biosCached) {
    console.log("found cached bios!");
    document.getElementById("bios-file-input").parentNode.style.display = "none";
    document.getElementById("reloadBios").classList.remove("hidden");
    biosData = new Uint8Array(JSON.parse(biosCached));
} else {
    console.log("Bios is not cached");
    var loadBios = biosFile => {
        console.log("loaded file " + biosFile)
        loadLocalFile(biosFile, result => {
            console.log("Loaded bios (" + result.length + " bytes )");
            biosData = result;

            console.log("Caching to localStorage");
            localStorage.setItem("biosCached", JSON.stringify(Array.from(biosData)));
            document.getElementById("bios-file-input").parentNode.style.display = "none";
        });
    };
    document.getElementById("bios-file-input").addEventListener('change', event => {
        loadBios(event.target.files[0])
    }, false);
}

document.getElementById("reloadBios").addEventListener('click', function() {
    this.classList.add("hidden");
    document.getElementById("bios-file-input").parentNode.style.display = "block";
    localStorage.removeItem("biosCached");
}, false);

function loadRom(romFile) {
    var promise = new Promise(function(resolve, reject) {
        loadLocalFile(romFile, result => {
            console.log('Loaded "' + romFile.name + '" ! length: ' + result.length);

            var rom_info = wasm.parse_rom_header(result);
            var rom_info2 = wasm.parse_rom_header(result);

            console.log("Game Code" + rom_info.get_game_code());
            console.log("Game Title" + rom_info.get_game_title());

            romData = result;
            resolve();
        });
    });

    return promise;
};

let dropArea = document.getElementById('screen');

['dragenter', 'dragover', 'dragleave', 'drop'].forEach(eventName => {
    dropArea.addEventListener(eventName,
        e => {
            // prevent default events
            e.preventDefault();
            e.stopPropagation();
        }, false)
});

dropArea.addEventListener('dragover', e => {
    dropArea.classList.add('hover');
}, false);

dropArea.addEventListener('dragleave', e => {
    dropArea.classList.remove('hover');
}, false);

dropArea.addEventListener('drop', e => {
    dropArea.classList.remove('hover');
    var files = e.dataTransfer.files;
    loadRom(files[0]).then(startEmulator);
}, true);

document.getElementById("skipBios").addEventListener('change', e => {
    shouldSkipBios = e.target.checked;
    localStorage.setItem("skipBios", JSON.stringify(shouldSkipBios));
});

document.getElementById("rom-file-input").addEventListener('change', e => {
    loadRom(e.target.files[0]).then(startEmulator);
}, false);

document.getElementById("startEmulator").addEventListener('click', e => {
    if (null == emulator) {
        startEmulator();
    }
}, false);

document.getElementById("maxFps").addEventListener('change', e => {
    if (intervalId != 0) {
        let checked = e.target.checked;
        clearInterval(intervalId);
        if (checked) {
            intervalId = setInterval(emulatorLoop, 0);
        } else {
            intervalId = setInterval(emulatorLoop, 16);
        }

    }
})

document.addEventListener("keydown", e => {
    if (null != emulator) {
        emulator.key_down(e.key)
    }
}, false);

document.addEventListener("keyup", e => {
    if (null != emulator) {
        emulator.key_up(e.key)
    }
}, false);