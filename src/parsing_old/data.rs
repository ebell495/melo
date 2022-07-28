use std::borrow::Cow;

use crate::{
    error::SourceLoc,
    notes::Midi,
};

#[derive(Debug, PartialEq, Eq)]
pub struct ParseTree<'a> {
    pub pieces: Vec<PieceNode<'a>>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PieceNode<'a> {
    pub title: Option<&'a str>,
    pub composer: Option<&'a str>,
    pub tempo: Option<u64>,
    pub beats: Option<u64>,

    pub voices: Vec<VoiceNode<'a>>,
    pub plays: Vec<PlayNode<'a>>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct VoiceNode<'a> {
    pub name: &'a str,
    pub program: Option<u8>,
    pub channel: Option<u8>,
    pub transpose: Option<i8>,
    pub volume: Option<u8>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PlayNode<'a> {
    pub voice: Option<&'a str>,
    pub staves: Vec<StaveNode<'a>>,
    pub error_loc: Option<SourceLoc>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StaveNode<'a> {
    pub prefix: Cow<'a, str>,
    pub bars: Vec<BarTypeNode>,
    pub bar_locs: Vec<SourceLoc>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BarTypeNode {
    Bar(BarNode),
    RepeatBar,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct BarNode {
    pub notes: Vec<NoteNode>,
    pub note_locs: Vec<SourceLoc>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NoteNode {
    Rest { length: u8 },
    Extension { length: u8 },
    Note { length: u8, midi: Midi },
}

impl NoteNode {
    pub fn length(self) -> u32 {
        match self {
            NoteNode::Rest { length }
            | NoteNode::Extension { length }
            | NoteNode::Note { length, .. } => u32::from(length),
        }
    }
}
