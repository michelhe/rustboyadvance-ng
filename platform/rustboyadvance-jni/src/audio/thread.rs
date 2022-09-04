use super::connector::AudioJNIConnector;

use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::thread::JoinHandle;

use rustboyadvance_core::util::audio::Consumer;

use jni::JavaVM;

#[derive(Debug)]
#[allow(dead_code)]
pub enum AudioThreadCommand {
    RenderAudio,
    Pause,
    Play,
    Terminate,
}

pub(crate) fn spawn_audio_worker_thread(
    audio_connector: AudioJNIConnector,
    jvm: JavaVM,
    mut consumer: Consumer<i16>,
) -> (JoinHandle<AudioJNIConnector>, Sender<AudioThreadCommand>) {
    let (tx, rx) = channel();

    let handle = thread::spawn(move || {
        info!("[AudioWorker] spawned!");

        info!("[AudioWorker] Attaching JVM");
        let env = jvm.attach_current_thread().unwrap();

        loop {
            let command = rx.recv().unwrap();
            match command {
                AudioThreadCommand::Pause => {
                    info!("[AudioWorker] - got {:?} command", command);
                    audio_connector.pause(&env);
                }

                AudioThreadCommand::Play => {
                    info!("[AudioWorker] - got {:?} command", command);
                    audio_connector.play(&env);
                }

                AudioThreadCommand::RenderAudio => {
                    let mut samples = [0; 4096 * 2]; // TODO is this memset expansive ?
                    let count = consumer.pop_slice(&mut samples);

                    audio_connector.write_audio_samples(&env, &samples[0..count]);
                }
                AudioThreadCommand::Terminate => {
                    info!("[AudioWorker] - got terminate command!");
                    break;
                }
            }
        }

        info!("[AudioWorker] terminating");

        // return the audio connector back
        audio_connector
    });

    (handle, tx)
}
