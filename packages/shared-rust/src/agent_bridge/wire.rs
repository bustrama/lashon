//! Frames on the bridge: a 4-byte big-endian length, then that many bytes.
//!
//! Every message has its own cap, and a length over it is refused before a
//! byte of the body is read or allocated, so a client can't make the
//! listener hold more than the cap. An empty frame is refused too: no
//! message is empty.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The client's hello: its nonce.
pub const MAX_HELLO: usize = 256;
/// The listener's challenge: its nonce and its proof.
pub const MAX_CHALLENGE: usize = 512;
/// The request: a MAC and the tool's name, input and folder. A request over
/// this (a very large file write, say) falls back to Claude Code's prompt.
pub const MAX_ASK: usize = 1 << 20;
/// The answer: a MAC and the decision.
pub const MAX_ANSWER: usize = 1024;

/// Why a frame couldn't be read or written.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("the other end closed the connection")]
    Closed,
    #[error("an empty frame")]
    Empty,
    #[error("a frame of {len} bytes, over the {max}-byte limit")]
    TooLarge { len: usize, max: usize },
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}

/// The bytes of one frame.
pub fn encode(payload: &[u8], max: usize) -> Result<Vec<u8>, FrameError> {
    let len = payload.len();
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if len > max {
        return Err(FrameError::TooLarge { len, max });
    }
    // `max` is at most MAX_ASK, far below u32::MAX.
    let prefix = u32::try_from(len).map_err(|_| FrameError::TooLarge { len, max })?;
    let mut frame = Vec::with_capacity(4 + len);
    frame.extend_from_slice(&prefix.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Write one frame of at most `max` bytes and flush it.
pub async fn write_frame<W>(writer: &mut W, payload: &[u8], max: usize) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
{
    let frame = encode(payload, max)?;
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

/// Read one frame of at most `max` bytes.
pub async fn read_frame<R>(reader: &mut R, max: usize) -> Result<Vec<u8>, FrameError>
where
    R: AsyncRead + Unpin,
{
    let mut prefix = [0u8; 4];
    match reader.read_exact(&mut prefix).await {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Err(FrameError::Closed),
        Err(err) => return Err(FrameError::Io(err)),
    }
    let len = u32::from_be_bytes(prefix) as usize;
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if len > max {
        return Err(FrameError::TooLarge { len, max });
    }
    let mut payload = vec![0u8; len];
    match reader.read_exact(&mut payload).await {
        Ok(_) => Ok(payload),
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Err(FrameError::Closed),
        Err(err) => Err(FrameError::Io(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn a_frame_round_trips_through_a_stream() {
        block_on(async {
            let (mut a, mut b) = tokio::io::duplex(64);
            let payload = "שלום, Claude".as_bytes().to_vec();
            let sent = payload.clone();
            let writer = tokio::spawn(async move { write_frame(&mut a, &sent, 64).await });
            assert_eq!(read_frame(&mut b, 64).await.unwrap(), payload);
            writer.await.unwrap().unwrap();
        });
    }

    #[test]
    fn the_length_is_four_bytes_big_endian() {
        let frame = encode(b"abc", 16).unwrap();
        assert_eq!(frame, [0, 0, 0, 3, b'a', b'b', b'c']);
    }

    #[test]
    fn a_frame_at_the_limit_passes_and_one_past_it_is_refused() {
        assert!(encode(&[7u8; 8], 8).is_ok());
        assert!(matches!(
            encode(&[7u8; 9], 8),
            Err(FrameError::TooLarge { len: 9, max: 8 })
        ));
    }

    #[test]
    fn an_empty_frame_is_refused_both_ways() {
        assert!(matches!(encode(b"", 8), Err(FrameError::Empty)));
        block_on(async {
            let mut input: &[u8] = &[0, 0, 0, 0];
            assert!(matches!(
                read_frame(&mut input, 8).await,
                Err(FrameError::Empty)
            ));
        });
    }

    #[test]
    fn an_oversized_length_is_refused_before_the_body_is_read() {
        block_on(async {
            // Claims 4 GiB and sends nothing more: the reader must not wait
            // for the body or allocate for it.
            let mut input: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF];
            match read_frame(&mut input, MAX_ASK).await {
                Err(FrameError::TooLarge { len, max }) => {
                    assert_eq!(len, u32::MAX as usize);
                    assert_eq!(max, MAX_ASK);
                }
                other => panic!("expected TooLarge, got {other:?}"),
            }
        });
    }

    #[test]
    fn a_closed_stream_reads_as_closed() {
        block_on(async {
            let mut nothing: &[u8] = &[];
            assert!(matches!(
                read_frame(&mut nothing, 8).await,
                Err(FrameError::Closed)
            ));
            // Cut off in the middle of the body.
            let mut cut: &[u8] = &[0, 0, 0, 5, b'a', b'b'];
            assert!(matches!(
                read_frame(&mut cut, 8).await,
                Err(FrameError::Closed)
            ));
        });
    }

    #[test]
    fn the_message_caps_hold_their_messages() {
        // A hello is a version and a 64-character hex nonce.
        assert!(MAX_HELLO >= r#"{"v":1,"nonce":""}"#.len() + 64);
        // A challenge adds a 64-character hex proof.
        assert!(MAX_CHALLENGE >= r#"{"v":1,"nonce":"","proof":""}"#.len() + 128);
        assert!(MAX_ANSWER >= 32 + r#"{"decision":"allow"}"#.len());
    }
}
