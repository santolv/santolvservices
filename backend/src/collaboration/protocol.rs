//! Collaboration wire protocol, version 1. Binary WebSocket frames: one tag byte, then fields.
//! Every client frame is untrusted and decoded with explicit bounds; anything else is rejected.
//!
//! | Tag | Direction | Body |
//! |---|---|---|
//! | `0x01` Hello | client → server | protocol version (u8), then the client's Yrs state vector (v1) |
//! | `0x02` Update | client → server | operation id (16 bytes, UUID), then a Yrs update (v1) |
//! | `0x03` Awareness | client → server | UTF-8 JSON object (cursor, selection…), at most 2 KiB |
//! | `0x04` Ping | client → server | empty |
//! | `0x81` Welcome | server → client | role (u8), connection id (16), then the update the client lacks |
//! | `0x82` Updates | server → client | count (u16, big endian), that many operation ids (16 each), then one Yrs update (v1) merging them |
//! | `0x83` Ack | server → client | operation id (16), then the persisted sequence (i64, big endian) |
//! | `0x84` Awareness | server → client | UTF-8 JSON stamped by the server with the sender's identity |
//! | `0x85` Pong | server → client | empty |
//! | `0x86` Error | server → client | ASCII error code |

use uuid::Uuid;

/// The only protocol version this server speaks.
pub const VERSION: u8 = 1;
/// Largest frame accepted from a client (also the WebSocket message limit).
pub const MAX_FRAME: usize = 256 * 1024;
/// Largest awareness (presence) state a client may publish.
pub const MAX_AWARENESS: usize = 2 * 1024;
/// Largest state vector a client may send: one entry per client id that ever edited.
pub const MAX_STATE_VECTOR: usize = 64 * 1024;

/// What a connection may do with a document, from Drive's permission levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Reader = 1,
    Editor = 2,
    Admin = 3,
}

#[derive(Debug, PartialEq)]
pub enum ClientFrame {
    Hello { state_vector: Vec<u8> },
    Update { op: Uuid, update: Vec<u8> },
    Awareness(serde_json::Map<String, serde_json::Value>),
    Ping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    Empty,
    TooLarge,
    UnknownTag,
    UnsupportedVersion,
    Truncated,
    InvalidAwareness,
    EmptyUpdate,
}

impl ProtocolError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Empty => "empty_frame",
            Self::TooLarge => "frame_too_large",
            Self::UnknownTag => "unknown_frame",
            Self::UnsupportedVersion => "unsupported_version",
            Self::Truncated => "truncated_frame",
            Self::InvalidAwareness => "invalid_awareness",
            Self::EmptyUpdate => "empty_update",
        }
    }
}

fn op_id(body: &[u8]) -> Result<(Uuid, &[u8]), ProtocolError> {
    let (id, rest) = body.split_at_checked(16).ok_or(ProtocolError::Truncated)?;
    let id = Uuid::from_slice(id).map_err(|_| ProtocolError::Truncated)?;
    Ok((id, rest))
}

/// Decode one client frame. The Yrs payloads are only length-checked here; the room decodes them.
pub fn decode(frame: &[u8]) -> Result<ClientFrame, ProtocolError> {
    if frame.len() > MAX_FRAME {
        return Err(ProtocolError::TooLarge);
    }
    let (&tag, body) = frame.split_first().ok_or(ProtocolError::Empty)?;
    match tag {
        0x01 => {
            let (&version, state_vector) = body.split_first().ok_or(ProtocolError::Truncated)?;
            if version != VERSION {
                return Err(ProtocolError::UnsupportedVersion);
            }
            if state_vector.len() > MAX_STATE_VECTOR {
                return Err(ProtocolError::TooLarge);
            }
            Ok(ClientFrame::Hello {
                state_vector: state_vector.to_vec(),
            })
        }
        0x02 => {
            let (op, update) = op_id(body)?;
            if op.is_nil() {
                return Err(ProtocolError::Truncated);
            }
            if update.is_empty() {
                return Err(ProtocolError::EmptyUpdate);
            }
            Ok(ClientFrame::Update {
                op,
                update: update.to_vec(),
            })
        }
        0x03 => {
            if body.len() > MAX_AWARENESS {
                return Err(ProtocolError::TooLarge);
            }
            match serde_json::from_slice::<serde_json::Value>(body) {
                Ok(serde_json::Value::Object(state)) => Ok(ClientFrame::Awareness(state)),
                _ => Err(ProtocolError::InvalidAwareness),
            }
        }
        0x04 if body.is_empty() => Ok(ClientFrame::Ping),
        0x04 => Err(ProtocolError::Truncated),
        _ => Err(ProtocolError::UnknownTag),
    }
}

pub fn welcome(role: Role, connection: Uuid, diff: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(18 + diff.len());
    frame.push(0x81);
    frame.push(role as u8);
    frame.extend_from_slice(connection.as_bytes());
    frame.extend_from_slice(diff);
    frame
}

/// Stored updates, broadcast once per committed batch: their operation ids and one merged update.
pub fn updates(ops: &[Uuid], merged: &[u8]) -> Vec<u8> {
    let count = u16::try_from(ops.len()).expect("a batch holds fewer than 65536 updates");
    let mut frame = Vec::with_capacity(3 + 16 * ops.len() + merged.len());
    frame.push(0x82);
    frame.extend_from_slice(&count.to_be_bytes());
    for op in ops {
        frame.extend_from_slice(op.as_bytes());
    }
    frame.extend_from_slice(merged);
    frame
}

pub fn ack(op: Uuid, seq: i64) -> Vec<u8> {
    let mut frame = Vec::with_capacity(25);
    frame.push(0x83);
    frame.extend_from_slice(op.as_bytes());
    frame.extend_from_slice(&seq.to_be_bytes());
    frame
}

pub fn awareness(state: &serde_json::Value) -> Vec<u8> {
    let mut frame = vec![0x84];
    frame.extend_from_slice(state.to_string().as_bytes());
    frame
}

pub fn pong() -> Vec<u8> {
    vec![0x85]
}

pub fn error(code: &str) -> Vec<u8> {
    let mut frame = vec![0x86];
    frame.extend_from_slice(code.as_bytes());
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_frames_are_rejected() {
        let op = Uuid::new_v4();
        assert_eq!(decode(&[]), Err(ProtocolError::Empty));
        assert_eq!(decode(&[0x7f]), Err(ProtocolError::UnknownTag));
        assert_eq!(decode(&[0x81, 1]), Err(ProtocolError::UnknownTag));
        assert_eq!(decode(&[0x01]), Err(ProtocolError::Truncated));
        assert_eq!(decode(&[0x01, 9]), Err(ProtocolError::UnsupportedVersion));
        assert_eq!(decode(&[0x02, 1, 2, 3]), Err(ProtocolError::Truncated));
        let mut nil = vec![0x02];
        nil.extend_from_slice(Uuid::nil().as_bytes());
        nil.push(0);
        assert_eq!(decode(&nil), Err(ProtocolError::Truncated));
        let mut empty = vec![0x02];
        empty.extend_from_slice(op.as_bytes());
        assert_eq!(decode(&empty), Err(ProtocolError::EmptyUpdate));
        assert_eq!(decode(b"\x03[1,2]"), Err(ProtocolError::InvalidAwareness));
        assert_eq!(
            decode(b"\x03{not json"),
            Err(ProtocolError::InvalidAwareness)
        );
        let mut big = vec![0x03];
        big.extend(std::iter::repeat_n(b' ', MAX_AWARENESS + 1));
        assert_eq!(decode(&big), Err(ProtocolError::TooLarge));
        assert_eq!(
            decode(&vec![0x02; MAX_FRAME + 1]),
            Err(ProtocolError::TooLarge)
        );
        assert_eq!(decode(&[0x04, 0]), Err(ProtocolError::Truncated));
    }

    #[test]
    fn valid_frames_decode() {
        let op = Uuid::new_v4();
        assert_eq!(
            decode(&[0x01, VERSION, 0]),
            Ok(ClientFrame::Hello {
                state_vector: vec![0]
            })
        );
        let mut frame = vec![0x02];
        frame.extend_from_slice(op.as_bytes());
        frame.extend_from_slice(&[1, 2, 3]);
        assert_eq!(
            decode(&frame),
            Ok(ClientFrame::Update {
                op,
                update: vec![1, 2, 3]
            })
        );
        assert!(matches!(
            decode(br#"{"cursor":3}"#.as_slice()),
            Err(ProtocolError::UnknownTag)
        ));
        assert!(matches!(
            decode(b"\x03{\"cursor\":3}"),
            Ok(ClientFrame::Awareness(_))
        ));
        assert_eq!(decode(&[0x04]), Ok(ClientFrame::Ping));
        assert_eq!(&ack(op, 7)[17..], &7i64.to_be_bytes());
        let batch = updates(&[op, op], &[9]);
        assert_eq!(
            (batch[0], &batch[1..3], batch.len()),
            (0x82, &[0, 2][..], 3 + 32 + 1)
        );
        assert_eq!(welcome(Role::Editor, op, &[5])[1], 2);
    }
}
