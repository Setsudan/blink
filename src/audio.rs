use std::io::Cursor;
use std::sync::mpsc;
use std::thread;

use rodio::{Decoder, OutputStream, Sink};

const BLINK_WAV: &[u8] = include_bytes!("../assets/blink.wav");
const THANKS_WAV: &[u8] = include_bytes!("../assets/thanks.wav");

#[derive(Clone, Copy)]
pub enum ChimeKind {
    /// Played when the break overlay appears.
    Blink,
    /// Played when the break timer finishes.
    Thanks,
}

enum AudioCmd {
    Play(ChimeKind),
}

/// Fire-and-forget chime player. Spawns one audio thread on first use.
pub struct ChimePlayer {
    tx: Option<mpsc::Sender<AudioCmd>>,
}

impl ChimePlayer {
    pub fn new() -> Self {
        Self { tx: None }
    }

    pub fn play(&mut self, kind: ChimeKind) {
        if self.ensure_worker().is_none() {
            return;
        }
        let Some(tx) = self.tx.as_ref() else {
            return;
        };
        let _ = tx.send(AudioCmd::Play(kind));
    }

    fn ensure_worker(&mut self) -> Option<()> {
        if self.tx.is_some() {
            return Some(());
        }

        let (tx, rx) = mpsc::channel::<AudioCmd>();
        let started = thread::Builder::new()
            .name("blink-audio".into())
            .spawn(move || audio_thread(rx))
            .ok();

        if started.is_none() {
            eprintln!("blink: failed to start audio thread");
            return None;
        }

        self.tx = Some(tx);
        Some(())
    }
}

impl Default for ChimePlayer {
    fn default() -> Self {
        Self::new()
    }
}

fn audio_bytes(kind: ChimeKind) -> &'static [u8] {
    match kind {
        ChimeKind::Blink => BLINK_WAV,
        ChimeKind::Thanks => THANKS_WAV,
    }
}

fn audio_thread(rx: mpsc::Receiver<AudioCmd>) {
    let Ok((_stream, handle)) = OutputStream::try_default() else {
        eprintln!("blink: no audio output device");
        // Drain so senders do not block forever if the channel fills.
        while rx.recv().is_ok() {}
        return;
    };

    while let Ok(AudioCmd::Play(kind)) = rx.recv() {
        let cursor = Cursor::new(audio_bytes(kind));
        let Ok(source) = Decoder::new(cursor) else {
            eprintln!("blink: failed to decode wav");
            continue;
        };
        let Ok(sink) = Sink::try_new(&handle) else {
            eprintln!("blink: failed to open audio sink");
            continue;
        };
        sink.append(source);
        sink.sleep_until_end();
    }
}
