const BLINK_WAV: &[u8] = include_bytes!("../assets/blink.wav");
const THANKS_WAV: &[u8] = include_bytes!("../assets/thanks.wav");

#[derive(Clone, Copy)]
pub enum ChimeKind {
    /// Played when the break overlay appears.
    Blink,
    /// Played when the break timer finishes.
    Thanks,
}

fn audio_bytes(kind: ChimeKind) -> &'static [u8] {
    match kind {
        ChimeKind::Blink => BLINK_WAV,
        ChimeKind::Thanks => THANKS_WAV,
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::io::Cursor;
    use std::sync::mpsc;
    use std::thread;

    use rodio::{Decoder, OutputStream, Sink};

    use super::{audio_bytes, ChimeKind};

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
}

/// Linux: play the embedded WAV through whatever sound server CLI is installed
/// (PipeWire's pw-play, PulseAudio's paplay, ALSA's aplay, or ffplay).
/// This keeps the Linux build free of ALSA dev packages while still chiming on
/// typical desktops. Falls back to silence (with a log line) when no player exists.
#[cfg(target_os = "linux")]
mod imp {
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread;

    use super::{audio_bytes, ChimeKind};

    enum AudioCmd {
        Play(ChimeKind),
    }

    /// Fire-and-forget chime player. Spawns one worker thread on first use so
    /// chimes never overlap and never block the UI thread.
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

    /// Candidate players: binary plus its fixed args.
    const PLAYERS: [&[&str]; 4] = [
        &["pw-play"],
        &["paplay"],
        &["ffplay", "-nodisp", "-autoexit", "-loglevel", "quiet"],
        &["aplay", "-q"],
    ];

    fn player_command() -> Option<&'static [&'static str]> {
        PLAYERS.iter().copied().find(|c| {
            Command::new(c[0])
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
                || which(c[0])
        })
    }

    fn which(bin: &str) -> bool {
        let Some(paths) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&paths).any(|dir| {
            let candidate = dir.join(bin);
            candidate.is_file()
        })
    }

    fn wav_path(kind: ChimeKind) -> Option<PathBuf> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(std::env::temp_dir);
        let name = match kind {
            ChimeKind::Blink => "blink-chime.wav",
            ChimeKind::Thanks => "blink-thanks.wav",
        };
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).ok()?;
        file.write_all(audio_bytes(kind)).ok()?;
        Some(path)
    }

    fn audio_thread(rx: mpsc::Receiver<AudioCmd>) {
        let Some(player) = player_command() else {
            eprintln!("blink: no audio player found (tried pw-play, paplay, ffplay, aplay)");
            while rx.recv().is_ok() {}
            return;
        };
        while let Ok(AudioCmd::Play(kind)) = rx.recv() {
            let Some(path) = wav_path(kind) else {
                continue;
            };
            let mut cmd = Command::new(player[0]);
            cmd.args(&player[1..])
                .arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            // Detached: do not wait for playback to finish.
            let _ = cmd.spawn();
        }
    }
}

pub use imp::ChimePlayer;
