use midi_file::MidiTrack;

use crate::context::Context;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PlayerConfig {
    Mute,
    Auto,
    Human,
}

#[derive(Debug, Clone)]
pub struct TrackConfig {
    pub track_id: usize,
    pub player: PlayerConfig,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Practice {
    Watch,
    RightHand,
    LeftHand,
    BothHands,
}

impl Practice {
    pub const ALL: [Self; 4] = [
        Self::Watch,
        Self::RightHand,
        Self::LeftHand,
        Self::BothHands,
    ];
}

const HAND_NAMES: [(&str, &str); 8] = [
    ("right", "left"),
    ("rh", "lh"),
    ("treble", "bass"),
    ("direita", "esquerda"),
    ("derecha", "izquierda"),
    ("droite", "gauche"),
    ("destra", "sinistra"),
    ("rechts", "links"),
];

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Hands {
    right: usize,
    left: usize,
}

impl Hands {
    fn detect(tracks: &[MidiTrack]) -> Option<Self> {
        let candidates: Vec<&MidiTrack> = tracks
            .iter()
            .filter(|track| !track.notes.is_empty() && track.has_other_than_drums)
            .collect();

        let named = |words: [&str; 8]| -> Vec<usize> {
            candidates
                .iter()
                .filter(|track| {
                    let name = track.name.as_deref().unwrap_or_default().to_lowercase();
                    name.split(|c: char| !c.is_alphanumeric())
                        .any(|word| words.contains(&word))
                })
                .map(|track| track.track_id)
                .collect()
        };

        let right = named(HAND_NAMES.map(|(right, _)| right));
        let left = named(HAND_NAMES.map(|(_, left)| left));

        if let ([right], [left]) = (right.as_slice(), left.as_slice())
            && right != left
        {
            return Some(Self {
                right: *right,
                left: *left,
            });
        }

        let [a, b] = candidates.as_slice() else {
            return None;
        };

        let (right, left) = if average_pitch(a) > average_pitch(b) {
            (a, b)
        } else {
            (b, a)
        };

        Some(Self {
            right: right.track_id,
            left: left.track_id,
        })
    }

    pub fn right(self) -> usize {
        self.right
    }

    pub fn left(self) -> usize {
        self.left
    }
}

fn average_pitch(track: &MidiTrack) -> f32 {
    track.notes.iter().map(|n| n.note as f32).sum::<f32>() / track.notes.len() as f32
}

#[derive(Default, Debug, Clone)]
pub struct SongConfig {
    pub tracks: Box<[TrackConfig]>,
    pub hands: Option<Hands>,
}

impl SongConfig {
    fn new(tracks: &[MidiTrack]) -> Self {
        let hands = Hands::detect(tracks);
        let tracks: Vec<_> = tracks
            .iter()
            .map(|t| {
                let is_drums = t.has_drums && !t.has_other_than_drums;
                TrackConfig {
                    track_id: t.track_id,
                    player: PlayerConfig::Auto,
                    visible: !is_drums,
                }
            })
            .collect();
        Self {
            tracks: tracks.into(),
            hands,
        }
    }

    pub fn practice(&self) -> Option<Practice> {
        let hands = self.hands?;

        let others_auto = self
            .tracks
            .iter()
            .filter(|track| track.track_id != hands.right && track.track_id != hands.left)
            .all(|track| track.player == PlayerConfig::Auto);

        if !others_auto {
            return None;
        }

        let right = self.tracks[hands.right].player;
        let left = self.tracks[hands.left].player;

        match (right, left) {
            (PlayerConfig::Auto, PlayerConfig::Auto) => Some(Practice::Watch),
            (PlayerConfig::Human, PlayerConfig::Human) => Some(Practice::BothHands),
            (PlayerConfig::Human, _) => Some(Practice::RightHand),
            (_, PlayerConfig::Human) => Some(Practice::LeftHand),
            _ => None,
        }
    }

    pub fn set_practice(&mut self, practice: Practice) {
        let Some(hands) = self.hands else {
            return;
        };

        let (right, left) = match practice {
            Practice::Watch => (PlayerConfig::Auto, PlayerConfig::Auto),
            Practice::RightHand => (PlayerConfig::Human, PlayerConfig::Auto),
            Practice::LeftHand => (PlayerConfig::Auto, PlayerConfig::Human),
            Practice::BothHands => (PlayerConfig::Human, PlayerConfig::Human),
        };

        for track in self.tracks.iter_mut() {
            track.player = PlayerConfig::Auto;
        }
        self.tracks[hands.right].player = right;
        self.tracks[hands.left].player = left;
    }

    pub fn other_hand(&self) -> Option<usize> {
        let hands = self.hands?;

        match self.practice()? {
            Practice::RightHand => Some(hands.left),
            Practice::LeftHand => Some(hands.right),
            Practice::Watch | Practice::BothHands => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Song {
    pub file: midi_file::MidiFile,
    pub config: SongConfig,
}

impl Song {
    pub fn new(file: midi_file::MidiFile) -> Self {
        let config = SongConfig::new(&file.tracks);
        Self { file, config }
    }

    pub fn from_env(ctx: &Context) -> Option<Self> {
        let args: Vec<String> = std::env::args().collect();
        let midi_file = if args.len() > 1 {
            midi_file::MidiFile::new(&args[1]).ok()
        } else if let Some(last) = ctx.config.last_opened_song() {
            midi_file::MidiFile::new(last).ok()
        } else {
            None
        };

        Some(Self::new(midi_file?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use midi_file::midly::{
        Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    };

    fn track(name: &'static str, channel: u8, keys: &[u8]) -> Vec<TrackEvent<'static>> {
        let mut events = vec![TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(MetaMessage::TrackName(name.as_bytes())),
        }];

        for &key in keys {
            for message in [
                MidiMessage::NoteOn {
                    key: key.into(),
                    vel: 100.into(),
                },
                MidiMessage::NoteOff {
                    key: key.into(),
                    vel: 0.into(),
                },
            ] {
                events.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: channel.into(),
                        message,
                    },
                });
            }
        }

        events
    }

    fn detect(tracks: Vec<Vec<TrackEvent<'static>>>) -> Option<Hands> {
        let smf = Smf {
            header: Header {
                format: Format::Parallel,
                timing: Timing::Metrical(480.into()),
            },
            tracks,
        };
        let file = midi_file::MidiFile::from_smf("test.mid", &smf).unwrap();
        Hands::detect(&file.tracks)
    }

    #[test]
    fn hands_are_found_by_name() {
        // The name wins over the pitch
        assert_eq!(
            detect(vec![
                track("Song", 0, &[]),
                track("M\u{e3}o direita", 0, &[38, 45]),
                track("M\u{e3}o esquerda", 1, &[62, 69]),
            ]),
            Some(Hands { right: 1, left: 2 })
        );
        assert_eq!(
            detect(vec![
                track("Piano LH", 0, &[72]),
                track("Piano RH", 0, &[48]),
                track("Strings", 0, &[60]),
            ]),
            Some(Hands { right: 1, left: 0 })
        );
    }

    #[test]
    fn hands_are_found_by_pitch_between_two_tracks() {
        assert_eq!(
            detect(vec![
                track("Piano", 0, &[48, 52]),
                track("Piano", 0, &[64, 67]),
                track("Drums", 9, &[36, 42]),
            ]),
            Some(Hands { right: 1, left: 0 })
        );
    }

    #[test]
    fn hands_are_unknown_otherwise() {
        assert_eq!(detect(vec![track("Piano", 0, &[60])]), None);
        assert_eq!(
            detect(vec![
                track("", 0, &[48]),
                track("", 0, &[60]),
                track("", 0, &[72]),
            ]),
            None
        );
        // Whole words only
        assert_eq!(
            detect(vec![
                track("Bassoon", 0, &[48]),
                track("Bright Strings", 0, &[60]),
                track("Flute", 0, &[72]),
            ]),
            None
        );
    }
}
