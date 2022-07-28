pub mod data;
pub mod error;

use crate::{
    error::SourceMap,
    parsing_old::data::*,
    trust::Trust,
};

use self::{
    data::*,
    error::{ErrorType, SequencingError},
};

pub fn sequence_pieces<'a>(
    parse_tree: &ParseTree<'a>,
    _source_map: &SourceMap,
) -> Result<Vec<Piece<'a>>, SequencingError> {
    use crate::notes::lcm;

    let mut pieces = Vec::new();

    for piece_node in &parse_tree.pieces {
        // validation
        {
            for play in &piece_node.plays {
                let matched = piece_node
                    .voices
                    .iter()
                    .any(|voice| Some(voice.name) == play.voice);
                if !matched {
                    let error = match play.voice {
                        Some(voice_name) => ErrorType::UndeclaredVoice {
                            voice_name: voice_name.to_owned(),
                        },
                        None => ErrorType::VoicelessPlayBlock,
                    };

                    return Err(SequencingError {
                        loc: play.error_loc.as_ref().trust().clone(),
                        error,
                    });
                }
            }
        }

        let Piece {
            title,
            composer,
            tempo,
            beats,
            ..
        } = Piece::default();

        let title = piece_node.title.or(title);
        let composer = piece_node.composer.or(composer);
        let tempo = piece_node.tempo.unwrap_or(tempo);
        let beats = piece_node.beats.unwrap_or(beats);

        let mut voices = Vec::new();

        for voice_node in &piece_node.voices {
            let Voice {
                channel,
                program,
                transpose,
                ..
            } = Voice::default();

            let name = voice_node.name;
            let channel = voice_node.channel.unwrap_or(channel);
            let program = voice_node.program.unwrap_or(program);
            let transpose = voice_node.transpose.unwrap_or(transpose);
            let volume = voice_node.volume.map(|vol| f64::from(vol) / 127.0);

            let divisions_per_bar = piece_node
                .plays
                .iter()
                .filter(|play| play.voice == Some(name))
                .flat_map(|play| {
                    play.staves.iter().flat_map(|stave| {
                        stave.bars.iter().map(|bar_type| match *bar_type {
                            BarTypeNode::Bar(ref bar) => {
                                bar.notes.iter().map(|note| note.length()).sum()
                            }
                            BarTypeNode::RepeatBar => 1,
                        })
                    })
                })
                .fold(1, lcm);

            let mut notes: Vec<Note> = Vec::new();
            let mut debug_bar_info: Vec<DebugBarInfo> = Vec::new();

            for play_node in &piece_node.plays {
                if play_node.voice != Some(name) {
                    continue;
                }

                for stave_node in &play_node.staves {
                    let mut previous_note_exists = false;

                    for (index, bar_node) in stave_node.bars.iter().enumerate() {
                        let mut cursor = index as u32 * divisions_per_bar;

                        let bar_node = match *bar_node {
                            BarTypeNode::Bar(ref bar) => bar,
                            BarTypeNode::RepeatBar => {
                                previous_note_exists = false;

                                let mut previous_bar = None;
                                let mut previous_index = index;
                                while previous_index > 0 {
                                    previous_index -= 1;
                                    match stave_node.bars[previous_index] {
                                        BarTypeNode::Bar(ref bar) => {
                                            previous_bar = Some(bar);
                                            break;
                                        }
                                        BarTypeNode::RepeatBar => (),
                                    }
                                }

                                previous_bar.ok_or_else(|| SequencingError {
                                    loc: stave_node.bar_locs[index].clone(),
                                    error: ErrorType::NothingToRepeat,
                                })?
                            }
                        };

                        let bar_node_length: u32 =
                            bar_node.notes.iter().map(|note| note.length()).sum();

                        let bar_info = DebugBarInfo {
                            loc: bar_node.note_locs[0].clone(),
                            divisions_in_source: bar_node_length,
                        };
                        debug_bar_info.push(bar_info);

                        assert!(divisions_per_bar % bar_node_length == 0);
                        let note_scale = divisions_per_bar / bar_node_length;

                        for (note_index, &note_node) in bar_node.notes.iter().enumerate() {
                            match note_node {
                                NoteNode::Rest { length } => {
                                    previous_note_exists = false;
                                    cursor += note_scale * u32::from(length);
                                }
                                NoteNode::Extension { length } => {
                                    if previous_note_exists {
                                        let previous_note = notes.last_mut().trust();
                                        previous_note.length += note_scale * u32::from(length);
                                    }

                                    cursor += note_scale * u32::from(length);
                                }
                                NoteNode::Note { midi, length } => {
                                    previous_note_exists = true;

                                    let midi =
                                        midi.transposed(transpose).ok_or(SequencingError {
                                            loc: bar_node.note_locs[note_index].clone(),
                                            error: ErrorType::InvalidNote {
                                                octave_offset: transpose / 12,
                                            },
                                        })?;

                                    let length = note_scale * u32::from(length);
                                    let position = cursor;
                                    let note = Note {
                                        midi,
                                        length,
                                        position,
                                    };

                                    notes.push(note);

                                    cursor += length;
                                }
                            }
                        }
                    }
                }

                notes.sort_by_key(|note| note.position);
            }

            let voice = Voice {
                name,
                channel,
                program,
                transpose,
                volume,
                divisions_per_bar,
                notes,
                debug_bar_info,
            };

            voices.push(voice);
        }

        let piece = Piece {
            title,
            composer,
            beats,
            tempo,
            voices,
        };

        pieces.push(piece);
    }

    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexing;
    use crate::parsing_old;
    use crate::test_helpers::midi;

    fn sequence_test(source: &str, expected: Piece) {
        let (tokens, source_map) = lexing::lex(source, None).expect("ERROR IN LEXER");
        let parse_tree = parsing_old::parse(&tokens, &source_map).expect("ERROR IN PARSER");
        let piece = &sequence_pieces(&parse_tree, &source_map).unwrap()[0];
        assert_eq!(piece, &expected);
    }

    fn sequence_test_fail(source: &str) {
        let (tokens, source_map) = lexing::lex(source, None).expect("ERROR IN LEXER");
        let parse_tree = parsing_old::parse(&tokens, &source_map).expect("ERROR IN PARSER");
        assert!(sequence_pieces(&parse_tree, &source_map).is_err());
    }

    fn voice_test(source: &str, expected_notes: Vec<Note>) {
        let (tokens, source_map) = lexing::lex(source, None).expect("ERROR IN LEXER");
        let parse_tree = parsing_old::parse(&tokens, &source_map).expect("ERROR IN PARSER");
        let piece = &sequence_pieces(&parse_tree, &source_map).unwrap()[0];
        assert_eq!(piece.voices[0].notes, expected_notes);
    }

    #[test]
    fn sequence_empty_piece() {
        sequence_test("", Piece::default());
    }

    #[test]
    fn piece_with_attributes() {
        sequence_test(
            "piece { title: One, composer: Two, tempo: 3, beats: 4 }",
            Piece {
                title: Some("One"),
                composer: Some("Two"),
                tempo: 3,
                beats: 4,
                ..Default::default()
            },
        );
    }

    #[test]
    fn piece_with_empty_voice() {
        sequence_test(
            "voice Empty { }",
            Piece {
                voices: vec![Voice {
                    name: "Empty",
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
    }

    #[test]
    fn voice_with_mismatched_play() {
        sequence_test_fail("voice OneNote { } play Different { :| C }");
    }

    #[test]
    fn voice_with_single_note() {
        voice_test(
            "voice OneNote { } play OneNote { :| C }",
            vec![Note {
                midi: midi(60),
                length: 1,
                position: 0,
            }],
        );
    }

    #[test]
    fn voice_with_two_notes() {
        voice_test(
            "voice TwoNote { } play TwoNote { :| C G }",
            vec![
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 0,
                },
                Note {
                    midi: midi(67),
                    length: 1,
                    position: 1,
                },
            ],
        );
    }

    #[test]
    fn voice_with_two_staves() {
        voice_test(
            "voice Diad { } play Diad { :| C ; :| G }",
            vec![
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 0,
                },
                Note {
                    midi: midi(67),
                    length: 1,
                    position: 0,
                },
            ],
        );
    }

    #[test]
    fn threes_against_twos() {
        voice_test(
            "voice Diad { } play Diad { :| C E G ; :| c g }",
            vec![
                Note {
                    midi: midi(60),
                    length: 2,
                    position: 0,
                },
                Note {
                    midi: midi(72),
                    length: 3,
                    position: 0,
                },
                Note {
                    midi: midi(64),
                    length: 2,
                    position: 2,
                },
                Note {
                    midi: midi(79),
                    length: 3,
                    position: 3,
                },
                Note {
                    midi: midi(67),
                    length: 2,
                    position: 4,
                },
            ],
        );
    }

    #[test]
    fn fail_when_notes_moved_out_of_range() {
        sequence_test_fail("voice V { octave: 1} play V { :| g#'''}");
    }

    #[test]
    fn voice_with_note_lengths() {
        voice_test(
            "voice A { } play A { :| C4 -2 G2 }",
            vec![
                Note {
                    midi: midi(60),
                    length: 4,
                    position: 0,
                },
                Note {
                    midi: midi(67),
                    length: 2,
                    position: 6,
                },
            ],
        );
    }

    #[test]
    fn voice_with_dots() {
        voice_test(
            "voice A { } play A { :| A..B C... .8 }",
            vec![
                Note {
                    midi: midi(57),
                    length: 3,
                    position: 0,
                },
                Note {
                    midi: midi(59),
                    length: 1,
                    position: 3,
                },
                Note {
                    midi: midi(60),
                    length: 12,
                    position: 4,
                },
            ],
        );
    }

    #[test]
    fn voice_with_leading_dots() {
        voice_test(
            "voice A { } play A { :| ...C E... G... -... }",
            vec![
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 3,
                },
                Note {
                    midi: midi(64),
                    length: 4,
                    position: 4,
                },
                Note {
                    midi: midi(67),
                    length: 4,
                    position: 8,
                },
            ],
        );
    }

    #[test]
    fn dots_do_not_carry_across_staves() {
        voice_test(
            "voice A { } play A { :| CEGc | ; :| ...g }",
            vec![
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 0,
                },
                Note {
                    midi: midi(64),
                    length: 1,
                    position: 1,
                },
                Note {
                    midi: midi(67),
                    length: 1,
                    position: 2,
                },
                Note {
                    midi: midi(72),
                    length: 1,
                    position: 3,
                },
                Note {
                    midi: midi(79),
                    length: 1,
                    position: 3,
                },
            ],
        );
    }

    #[test]
    fn notes_can_be_tied_across_bars() {
        voice_test(
            "voice A {} play A { :| CEG. | ..EC }",
            vec![
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 0,
                },
                Note {
                    midi: midi(64),
                    length: 1,
                    position: 1,
                },
                Note {
                    midi: midi(67),
                    length: 4,
                    position: 2,
                },
                Note {
                    midi: midi(64),
                    length: 1,
                    position: 6,
                },
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 7,
                },
            ],
        );
    }

    #[test]
    fn repeat_bars() {
        voice_test(
            "voice A {} play A { :| A C | % | }",
            vec![
                Note {
                    midi: midi(57),
                    length: 1,
                    position: 0,
                },
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 1,
                },
                Note {
                    midi: midi(57),
                    length: 1,
                    position: 2,
                },
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 3,
                },
            ],
        );
    }

    #[test]
    fn repeat_bars_twice() {
        voice_test(
            "voice A {} play A { :| A C | % | % | }",
            vec![
                Note {
                    midi: midi(57),
                    length: 1,
                    position: 0,
                },
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 1,
                },
                Note {
                    midi: midi(57),
                    length: 1,
                    position: 2,
                },
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 3,
                },
                Note {
                    midi: midi(57),
                    length: 1,
                    position: 4,
                },
                Note {
                    midi: midi(60),
                    length: 1,
                    position: 5,
                },
            ],
        );
    }

    #[test]
    fn fail_first_bar_repeat() {
        sequence_test_fail("voice A {} play A { :| % | }");
    }
}
