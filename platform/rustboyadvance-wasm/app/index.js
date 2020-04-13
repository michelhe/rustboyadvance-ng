import * as wasm from "rustboyadvance-wasm";

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

    let fps_text = document.getElementById('fps')
    intervalId = setInterval(function() {
        emulator.run_frame(ctx);
        fps_text.innerHTML = fpsCounter();
    }, 16);
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

let dropArea = document.getElementById('canvas-container');

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

['keydown', 'keyup'].forEach(eventName => {
    window.addEventListener(eventName,
        e => {
            // prevent default events
            e.preventDefault();
            e.stopPropagation();
        }, false)
});

window.addEventListener("keydown", e => {
    if (null != emulator) {
        emulator.key_down(e.key)
    }
}, false);

window.addEventListener("keyup", e => {
    if (null != emulator) {
        emulator.key_up(e.key)
    }
}, false);